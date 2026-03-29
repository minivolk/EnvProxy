//! Unix socket server for the envproxy agent.
//!
//! Listens for incoming connections from `libenvproxy.so` clients,
//! decodes requests using the wire protocol, resolves keys via the
//! configured backend, and sends responses.
//!
//! Supports both v1 (key only) and v2 (key + value) protocol requests.
//! v2 requests include the current env var value, enabling `vault:` prefix
//! resolution where the agent parses the Vault path from the value.

use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

use envproxy_proto::{encode_response, Status, PROTOCOL_V1, PROTOCOL_V2, VAULT_PREFIX};

use crate::backend::Backend;

/// The Unix socket server that handles requests from `libenvproxy.so`.
pub struct Server {
    listener: UnixListener,
    backend: Arc<dyn Backend>,
}

impl Server {
    /// Create a new server bound to the given socket path.
    ///
    /// Creates the parent directory if it does not exist.
    /// Removes any stale socket file at the path.
    ///
    /// # Errors
    ///
    /// Returns an error if the socket cannot be bound.
    pub async fn bind(
        socket_path: &Path,
        backend: Arc<dyn Backend>,
    ) -> Result<Self, std::io::Error> {
        // Ensure parent directory exists.
        if let Some(parent) = socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Remove stale socket file if it exists.
        if socket_path.exists() {
            tokio::fs::remove_file(socket_path).await?;
        }

        let listener = UnixListener::bind(socket_path)?;

        tracing::info!(path = %socket_path.display(), "agent listening");

        Ok(Self { listener, backend })
    }

    /// Run the server, accepting connections in a loop.
    ///
    /// Each connection is handled in a separate Tokio task.
    /// This method runs forever unless cancelled.
    pub async fn run(&self) -> Result<(), std::io::Error> {
        loop {
            let (stream, _addr) = self.listener.accept().await?;
            let backend = Arc::clone(&self.backend);

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, &*backend).await {
                    tracing::warn!(error = %e, "connection handler error");
                }
            });
        }
    }
}

/// Handle a single client connection.
///
/// Reads one request (v1 or v2), resolves the key via the backend, and sends
/// the response. For v2 requests where the value starts with `vault:`, the
/// backend's `resolve_with_value()` method is called instead of `resolve()`.
async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    backend: &dyn Backend,
) -> Result<(), std::io::Error> {
    // Read header: 1 byte version + 2 bytes key length
    let mut header = [0u8; 3];
    stream.read_exact(&mut header).await?;

    let version = header[0];
    if version != PROTOCOL_V1 && version != PROTOCOL_V2 {
        tracing::warn!(version, "unsupported protocol version");
        let response = encode_response(
            Status::Error,
            format!("unsupported protocol version: {version}").as_bytes(),
        );
        if let Some(resp) = response {
            stream.write_all(&resp).await?;
        }
        return Ok(());
    }

    let key_len = u16::from_be_bytes([header[1], header[2]]) as usize;

    // Read the key.
    let mut key_buf = vec![0u8; key_len];
    if key_len > 0 {
        stream.read_exact(&mut key_buf).await?;
    }

    // For v2, also read the value.
    let value_buf = if version == PROTOCOL_V2 {
        let mut val_header = [0u8; 2];
        stream.read_exact(&mut val_header).await?;
        let val_len = u16::from_be_bytes(val_header) as usize;
        let mut buf = vec![0u8; val_len];
        if val_len > 0 {
            stream.read_exact(&mut buf).await?;
        }
        Some(buf)
    } else {
        None
    };

    let key_str = String::from_utf8_lossy(&key_buf);

    // Determine which resolve method to call.
    let resolve_result = match &value_buf {
        Some(val) if !val.is_empty() => {
            let val_str = String::from_utf8_lossy(val);
            if val_str.starts_with(VAULT_PREFIX) {
                tracing::debug!(key = %key_str, vault_ref = %val_str, "resolving vault reference");
                backend.resolve_with_value(&key_str, &val_str).await
            } else {
                tracing::debug!(key = %key_str, "resolving key (v2, non-vault value)");
                backend.resolve(&key_str).await
            }
        }
        _ => {
            tracing::debug!(key = %key_str, "resolving key (v1)");
            backend.resolve(&key_str).await
        }
    };

    let response = match resolve_result {
        Ok(Some(value)) => {
            tracing::debug!(key = %key_str, value_len = value.len(), "key resolved");
            encode_response(Status::Found, value.as_bytes())
        }
        Ok(None) => {
            tracing::debug!(key = %key_str, "key not found");
            encode_response(Status::NotFound, b"")
        }
        Err(e) => {
            tracing::warn!(key = %key_str, error = %e, "backend error");
            encode_response(Status::Error, e.to_string().as_bytes())
        }
    };

    if let Some(resp) = response {
        stream.write_all(&resp).await?;
    }

    Ok(())
}
