//! File-based backend for resolving environment variables from a JSON file.
//!
//! This backend reads a file containing key-value pairs and resolves
//! environment variable lookups against it. The file is automatically
//! reloaded when its modification time changes, enabling dynamic secret
//! rotation without restarting the agent.
//!
//! # File Format (JSON)
//!
//! ```json
//! {
//!   "DATABASE_URL": "postgres://localhost/mydb",
//!   "API_KEY": "sk-1234567890"
//! }
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::SystemTime;

use tokio::sync::RwLock;

use super::{Backend, BackendError};

/// Internal state shared between resolve calls.
struct FileCache {
    /// Cached key-value pairs from the secrets file.
    data: HashMap<String, String>,
    /// Last known modification time of the file (for change detection).
    last_modified: Option<SystemTime>,
}

/// A backend that reads key-value pairs from a JSON file on disk.
///
/// On each `resolve()` call, the backend checks the file's modification time.
/// If the file has changed since the last load, it is re-read and the cache
/// is updated. This makes secret rotation seamless.
pub struct FileBackend {
    /// Path to the secrets file.
    path: PathBuf,
    /// Cached state protected by a read-write lock.
    cache: Arc<RwLock<FileCache>>,
}

impl FileBackend {
    /// Create a new `FileBackend` that reads from the given path.
    ///
    /// The file is loaded immediately. If the file does not exist, the backend
    /// starts empty (keys will not be found until the file is created).
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be parsed.
    pub async fn new(path: &Path) -> Result<Self, BackendError> {
        let backend = Self {
            path: path.to_path_buf(),
            cache: Arc::new(RwLock::new(FileCache { data: HashMap::new(), last_modified: None })),
        };
        backend.reload().await?;
        Ok(backend)
    }

    /// Reload the secrets file from disk unconditionally.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub async fn reload(&self) -> Result<(), BackendError> {
        let (contents, mtime) = match tokio::fs::read_to_string(&self.path).await {
            Ok(c) => {
                let mtime =
                    tokio::fs::metadata(&self.path).await.ok().and_then(|m| m.modified().ok());
                (c, mtime)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(
                    path = %self.path.display(),
                    "secrets file not found, starting empty"
                );
                let mut cache = self.cache.write().await;
                cache.data.clear();
                cache.last_modified = None;
                return Ok(());
            }
            Err(e) => return Err(BackendError::Io(e)),
        };

        let data: HashMap<String, String> = serde_json::from_str(&contents).map_err(|e| {
            BackendError::Parse(format!("failed to parse {}: {e}", self.path.display()))
        })?;

        let mut cache = self.cache.write().await;
        let key_count = data.len();
        cache.data = data;
        cache.last_modified = mtime;

        tracing::info!(
            path = %self.path.display(),
            keys = key_count,
            "loaded secrets file"
        );

        Ok(())
    }
}

impl Backend for FileBackend {
    fn resolve(
        &self,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, BackendError>> + Send + 'static>> {
        let key = key.to_owned();
        let cache = Arc::clone(&self.cache);
        let path = self.path.clone();
        // We need a second Arc for reload_if_changed — clone the whole backend state.
        let backend_cache = Arc::clone(&self.cache);

        Box::pin(async move {
            // Check mtime and reload if the file changed.
            let current_mtime =
                tokio::fs::metadata(&path).await.ok().and_then(|m| m.modified().ok());

            let needs_reload = {
                let c = backend_cache.read().await;
                match (c.last_modified, current_mtime) {
                    (None, Some(_)) | (Some(_), None) => true,
                    (Some(cached), Some(current)) => cached != current,
                    (None, None) => false,
                }
            };

            if needs_reload {
                tracing::debug!(path = %path.display(), "file changed, reloading");

                let contents = match tokio::fs::read_to_string(&path).await {
                    Ok(c) => c,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        let mut c = backend_cache.write().await;
                        c.data.clear();
                        c.last_modified = None;
                        return Ok(None);
                    }
                    Err(e) => return Err(BackendError::Io(e)),
                };

                let data: HashMap<String, String> =
                    serde_json::from_str(&contents).map_err(|e| {
                        BackendError::Parse(format!("failed to parse {}: {e}", path.display()))
                    })?;

                let mut c = backend_cache.write().await;
                let key_count = data.len();
                c.data = data;
                c.last_modified = current_mtime;
                tracing::info!(
                    path = %path.display(),
                    keys = key_count,
                    "reloaded secrets file"
                );
            }

            let c = cache.read().await;
            Ok(c.data.get(&key).cloned())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_file(data: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("failed to create temp file");
        file.write_all(data.as_bytes()).expect("failed to write temp file");
        file
    }

    #[tokio::test]
    async fn resolve_should_return_existing_key() {
        let file = create_temp_file(r#"{"DATABASE_URL": "postgres://localhost/mydb"}"#);
        let backend = FileBackend::new(file.path()).await.expect("failed to create backend");

        let result: Result<Option<String>, BackendError> = backend.resolve("DATABASE_URL").await;
        assert_eq!(
            result.expect("resolve should not error"),
            Some("postgres://localhost/mydb".to_owned())
        );
    }

    #[tokio::test]
    async fn resolve_should_return_none_for_missing_key() {
        let file = create_temp_file(r#"{"FOO": "bar"}"#);
        let backend = FileBackend::new(file.path()).await.expect("failed to create backend");

        let result: Result<Option<String>, BackendError> = backend.resolve("MISSING_KEY").await;
        assert_eq!(result.expect("resolve should not error"), None);
    }

    #[tokio::test]
    async fn new_should_handle_missing_file() {
        let path = Path::new("/tmp/envproxy-test-nonexistent-file.json");
        let backend = FileBackend::new(path).await;
        assert!(backend.is_ok(), "should handle missing file gracefully");
    }

    #[tokio::test]
    async fn new_should_reject_invalid_json() {
        let file = create_temp_file("not valid json");
        let backend = FileBackend {
            path: file.path().to_path_buf(),
            cache: Arc::new(RwLock::new(FileCache { data: HashMap::new(), last_modified: None })),
        };
        let result = backend.reload().await;
        assert!(result.is_err(), "should reject invalid JSON");
    }

    #[tokio::test]
    async fn resolve_should_pick_up_file_changes() {
        let file = create_temp_file(r#"{"KEY": "original"}"#);
        let backend = FileBackend::new(file.path()).await.expect("failed to create backend");

        // Verify original value.
        let result: Result<Option<String>, BackendError> = backend.resolve("KEY").await;
        assert_eq!(result.expect("should resolve"), Some("original".to_owned()));

        // Small delay to ensure filesystem mtime granularity.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Overwrite the file with a new value.
        std::fs::write(file.path(), r#"{"KEY": "rotated"}"#).expect("failed to write updated file");

        // Resolve again — should pick up the new value.
        let result: Result<Option<String>, BackendError> = backend.resolve("KEY").await;
        assert_eq!(result.expect("should resolve"), Some("rotated".to_owned()));
    }
}
