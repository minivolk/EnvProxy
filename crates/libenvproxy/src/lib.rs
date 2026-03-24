//! `libenvproxy` — `LD_PRELOAD` shared library that intercepts `getenv()` calls.
//!
//! When loaded via `LD_PRELOAD`, this library overrides the standard C `getenv` function.
//! It communicates with the `envproxy-agent` daemon over a Unix socket to dynamically
//! resolve environment variable values from remote secret sources.
//!
//! # Architecture
//!
//! ```text
//! Application calls getenv("FOO")
//!     │
//!     ▼
//! libenvproxy.so intercepts the call
//!     │
//!     ├─ If key matches interception rules → query agent via Unix socket
//!     │                                       └─ return resolved value
//!     └─ Otherwise → call real getenv via dlsym(RTLD_NEXT)
//! ```
//!
//! # Configuration
//!
//! - `ENVPROXY_SOCKET`: Path to the agent's Unix socket (default: `/tmp/envproxy/agent.sock`)
//! - `ENVPROXY_ENABLED`: Set to `0` to disable interception entirely
//! - `ENVPROXY_DEBUG`: Set to `1` to enable debug output to stderr

use std::ffi::{CStr, CString};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::OnceLock;

use envproxy_proto::{encode_request, Status, DEFAULT_SOCKET_PATH};

/// Library constructor — runs before `main()` when the `.so` is loaded via `LD_PRELOAD`.
///
/// Sets up runtime hooks for languages that cache the environment at startup:
/// - **Python**: Seeds `PYTHONPATH` so `sitecustomize.py` patches `os.environ`
/// - **Java**: Seeds `JAVA_TOOL_OPTIONS` so a javaagent patches `System.getenv()`
#[ctor::ctor]
fn _envproxy_init() {
    // SAFETY: We call setenv (via std::env::set_var) to modify the environment
    // before main() runs. This is safe because no other threads exist yet.

    setup_python_hook();
    setup_java_hook();
}

/// Seed `PYTHONPATH` so Python auto-loads our `sitecustomize.py` hook.
fn setup_python_hook() {
    if std::env::var("ENVPROXY_NO_PYTHON").unwrap_or_default() == "1" {
        return;
    }

    let Some(python_dir) = find_support_dir("python", "sitecustomize.py") else {
        return;
    };

    let new_pythonpath = match std::env::var("PYTHONPATH") {
        Ok(existing) if !existing.is_empty() => format!("{python_dir}:{existing}"),
        _ => python_dir,
    };

    std::env::set_var("PYTHONPATH", &new_pythonpath);
}

/// Seed `JAVA_TOOL_OPTIONS` so Java auto-loads our javaagent that patches `System.getenv()`.
fn setup_java_hook() {
    if std::env::var("ENVPROXY_NO_JAVA").unwrap_or_default() == "1" {
        return;
    }

    let Some(jar_path) = find_support_file("java", "envproxy-agent.jar") else {
        return;
    };

    // Build the javaagent flag + required --add-opens for reflection into java.lang.
    let agent_opts = format!("--add-opens=java.base/java.lang=ALL-UNNAMED -javaagent:{jar_path}");

    let new_opts = match std::env::var("JAVA_TOOL_OPTIONS") {
        Ok(existing) if !existing.is_empty() => format!("{existing} {agent_opts}"),
        _ => agent_opts,
    };

    std::env::set_var("JAVA_TOOL_OPTIONS", &new_opts);
}

/// Locate a support directory (e.g., `python/`, `java/`) relative to this `.so` file.
///
/// Checks:
/// 1. Explicit env var override (`ENVPROXY_PYTHON_PATH`, `ENVPROXY_JAVA_PATH`)
/// 2. Sibling to the `.so`: `<so_dir>/envproxy/<subdir>/`
/// 3. Share directory: `<so_dir>/../share/envproxy/<subdir>/`
/// 4. Development layout: `<so_dir>/../../support/<subdir>/`
///
/// Returns the directory path if `marker_file` exists inside it.
fn find_support_dir(subdir: &str, marker_file: &str) -> Option<String> {
    // Check explicit override (e.g., ENVPROXY_PYTHON_PATH).
    let env_key = format!("ENVPROXY_{}_PATH", subdir.to_uppercase());
    if let Ok(path) = std::env::var(&env_key) {
        if std::path::Path::new(&path).join(marker_file).exists() {
            return Some(path);
        }
    }

    let so_path = get_self_path()?;
    let so_dir = std::path::Path::new(&so_path).parent()?;

    let candidates = [
        so_dir.join(format!("envproxy/{subdir}")), // /usr/lib/envproxy/<subdir>/
        so_dir.join(format!("../share/envproxy/{subdir}")), // /usr/share/envproxy/<subdir>/
        so_dir.join(format!("../{subdir}")),       // <prefix>/<subdir>/ (flat install)
        so_dir.join(format!("../../support/{subdir}")), // dev: target/release/../../support/<subdir>/
    ];

    for candidate in &candidates {
        if candidate.join(marker_file).exists() {
            return candidate.canonicalize().ok()?.to_str().map(String::from);
        }
    }

    None
}

/// Locate a specific support file, returning its full path.
fn find_support_file(subdir: &str, filename: &str) -> Option<String> {
    let dir = find_support_dir(subdir, filename)?;
    let full_path = std::path::Path::new(&dir).join(filename);
    if full_path.exists() {
        full_path.to_str().map(String::from)
    } else {
        None
    }
}

/// Get the filesystem path of this `.so` using `dladdr`.
fn get_self_path() -> Option<String> {
    // SAFETY: dladdr is a standard POSIX function that returns info about
    // the shared object containing the given address. We pass the address
    // of our own function, which is always valid.
    unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();
        let self_addr = get_self_path as *const std::ffi::c_void;
        if libc::dladdr(self_addr, &mut info) == 0 {
            return None;
        }
        if info.dli_fname.is_null() {
            return None;
        }
        let path = CStr::from_ptr(info.dli_fname);
        Some(path.to_string_lossy().into_owned())
    }
}

/// Type alias for the real `getenv` function signature.
type GetenvFn = unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char;

/// Cached resolved values. Since `getenv` must return a `*mut c_char` that remains valid,
/// we leak `CString` allocations intentionally — they live for the process lifetime.
/// This is the standard approach for `LD_PRELOAD` `getenv` overrides.
static SOCKET_PATH: OnceLock<String> = OnceLock::new();

/// Resolve the real `getenv` from libc via `dlsym(RTLD_NEXT, "getenv")`.
///
/// # Safety
///
/// This function calls `dlsym` which is unsafe FFI. The returned pointer is
/// cast to a function pointer matching the `getenv` signature. This is safe
/// because `getenv` always exists in libc and has a stable ABI.
unsafe fn real_getenv() -> GetenvFn {
    // SAFETY: RTLD_NEXT is a well-defined dlsym pseudo-handle that resolves
    // to the next occurrence of the symbol in the dynamic linker search order.
    // `getenv` is guaranteed to exist in libc.
    static REAL: OnceLock<GetenvFn> = OnceLock::new();
    *REAL.get_or_init(|| {
        let symbol = CString::new("getenv").expect("CString::new for 'getenv' cannot fail");
        // SAFETY: dlsym with RTLD_NEXT finds the real getenv in libc.
        // The cast is valid because getenv has a stable C ABI signature.
        let ptr = libc::dlsym(libc::RTLD_NEXT, symbol.as_ptr());
        assert!(!ptr.is_null(), "dlsym failed to find getenv in libc");
        std::mem::transmute::<*mut std::ffi::c_void, GetenvFn>(ptr)
    })
}

/// Get the socket path, reading from `ENVPROXY_SOCKET` env var on first call.
///
/// Uses the real `getenv` to avoid infinite recursion.
fn get_socket_path() -> &'static str {
    SOCKET_PATH.get_or_init(|| {
        // SAFETY: We call the real getenv to read our own config, avoiding recursion.
        let ptr = unsafe { real_getenv()(b"ENVPROXY_SOCKET\0".as_ptr().cast()) };
        if ptr.is_null() {
            DEFAULT_SOCKET_PATH.to_owned()
        } else {
            // SAFETY: The real getenv returns a valid C string from the environment block.
            let cstr = unsafe { CStr::from_ptr(ptr) };
            cstr.to_string_lossy().into_owned()
        }
    })
}

/// Check if envproxy is enabled (defaults to true).
///
/// Set `ENVPROXY_ENABLED=0` to disable.
fn is_enabled() -> bool {
    // SAFETY: We call the real getenv to read our own config, avoiding recursion.
    let ptr = unsafe { real_getenv()(b"ENVPROXY_ENABLED\0".as_ptr().cast()) };
    if ptr.is_null() {
        return true;
    }
    // SAFETY: The real getenv returns a valid C string.
    let cstr = unsafe { CStr::from_ptr(ptr) };
    cstr.to_bytes() != b"0"
}

/// Check if debug logging is enabled.
fn is_debug() -> bool {
    // SAFETY: We call the real getenv to read our own config, avoiding recursion.
    let ptr = unsafe { real_getenv()(b"ENVPROXY_DEBUG\0".as_ptr().cast()) };
    if ptr.is_null() {
        return false;
    }
    // SAFETY: The real getenv returns a valid C string.
    let cstr = unsafe { CStr::from_ptr(ptr) };
    cstr.to_bytes() == b"1"
}

/// Log a debug message to stderr (only when `ENVPROXY_DEBUG=1`).
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if is_debug() {
            eprintln!("[envproxy] {}", format!($($arg)*));
        }
    };
}

/// Query the envproxy-agent for a key via Unix socket.
///
/// Returns `Some(CString)` if the key was resolved, `None` if the agent
/// is unavailable or the key should be passed through to real `getenv`.
fn query_agent(key: &[u8]) -> Option<CString> {
    let socket_path = get_socket_path();

    let encoded = encode_request(key)?;

    let mut stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(e) => {
            debug_log!("failed to connect to agent at {socket_path}: {e}");
            return None;
        }
    };

    // Set a timeout to avoid blocking the application indefinitely.
    let timeout = std::time::Duration::from_millis(500);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    if let Err(e) = stream.write_all(&encoded) {
        debug_log!("failed to write request: {e}");
        return None;
    }

    // Read response: 1 byte status + 2 bytes length + up to 64KiB value
    let mut header = [0u8; 3];
    if let Err(e) = stream.read_exact(&mut header) {
        debug_log!("failed to read response header: {e}");
        return None;
    }

    let status = envproxy_proto::Status::from_byte(header[0])?;
    let val_len = u16::from_be_bytes([header[1], header[2]]) as usize;

    let mut value_buf = vec![0u8; val_len];
    if val_len > 0 {
        if let Err(e) = stream.read_exact(&mut value_buf) {
            debug_log!("failed to read response value: {e}");
            return None;
        }
    }

    match status {
        Status::Found => {
            debug_log!("resolved key '{}' ({} bytes)", String::from_utf8_lossy(key), val_len);
            // The value might contain interior NUL bytes, which would be invalid
            // for a C string. Use CString::new which checks for this.
            CString::new(value_buf).ok()
        }
        Status::NotFound => {
            debug_log!("key '{}' not found in backends", String::from_utf8_lossy(key));
            None
        }
        Status::Error => {
            debug_log!(
                "agent error for key '{}': {}",
                String::from_utf8_lossy(key),
                String::from_utf8_lossy(&value_buf)
            );
            None
        }
        Status::Passthrough => None,
    }
}

/// Cached flag for whether envproxy is enabled.
static ENABLED: OnceLock<bool> = OnceLock::new();

/// Override for the C `getenv` function.
///
/// This function is called by the dynamic linker instead of libc's `getenv`
/// when this library is loaded via `LD_PRELOAD`.
///
/// # Safety
///
/// This function is called from C code. The `name` parameter must be a valid,
/// NUL-terminated C string. The returned pointer is either:
/// - A pointer from the real `getenv` (managed by libc), or
/// - A leaked `CString` allocation that lives for the process duration.
///
/// Both cases satisfy C's expectation that `getenv` returns a pointer valid
/// until the environment is modified.
#[no_mangle]
pub unsafe extern "C" fn getenv(name: *const std::ffi::c_char) -> *mut std::ffi::c_char {
    // Guard: if name is null, delegate to real getenv (which handles it).
    if name.is_null() {
        // SAFETY: Delegating null to real getenv, which handles it per POSIX.
        return real_getenv()(name);
    }

    // SAFETY: The caller guarantees name is a valid NUL-terminated C string.
    // This is the contract of getenv() in C.
    let key_cstr = CStr::from_ptr(name);
    let key_bytes = key_cstr.to_bytes();

    // Never intercept our own configuration variables to avoid infinite recursion.
    if key_bytes.starts_with(b"ENVPROXY_") {
        // SAFETY: Delegating to real getenv for our own config vars.
        return real_getenv()(name);
    }

    // Check if envproxy is enabled (cached after first call).
    let enabled = *ENABLED.get_or_init(is_enabled);
    if !enabled {
        // SAFETY: Envproxy disabled, delegate to real getenv.
        return real_getenv()(name);
    }

    // Try to resolve via the agent.
    if let Some(resolved) = query_agent(key_bytes) {
        // Leak the CString so the pointer remains valid for the caller.
        // This is the standard pattern for `LD_PRELOAD` getenv overrides:
        // the returned pointer must outlive the call, and C code expects
        // getenv's return to be valid until the next setenv/putenv/unsetenv.
        return resolved.into_raw();
    }

    // Fallback: call the real getenv.
    // SAFETY: Delegating to real getenv with the original, valid name pointer.
    real_getenv()(name)
}

/// Override for `secure_getenv` (glibc extension).
///
/// `secure_getenv` is like `getenv` but returns NULL for setuid/setgid programs.
/// We intercept it with the same logic as `getenv`.
///
/// # Safety
///
/// Same safety requirements as [`getenv`].
#[no_mangle]
pub unsafe extern "C" fn secure_getenv(name: *const std::ffi::c_char) -> *mut std::ffi::c_char {
    // Delegate to our getenv override, which handles all the logic.
    getenv(name)
}
