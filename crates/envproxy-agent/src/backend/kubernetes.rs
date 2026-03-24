//! Kubernetes Secret backend for resolving environment variables.
//!
//! Reads key-value pairs from a Kubernetes Secret and serves them
//! as environment variables. Watches the Secret for changes and
//! reloads automatically when the Secret is updated.
//!
//! # Configuration
//!
//! ```toml
//! [backend]
//! type = "kubernetes"
//! namespace = "default"
//! secret_name = "app-secrets"
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures::TryStreamExt;
use k8s_openapi::api::core::v1::Secret;
use kube::api::Api;
use kube::runtime::watcher;
use kube::Client;
use tokio::sync::RwLock;

use super::{Backend, BackendError};

/// Configuration for the Kubernetes Secret backend.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct KubernetesBackendConfig {
    /// Namespace of the Secret.
    pub namespace: String,
    /// Name of the Secret to read.
    pub secret_name: String,
}

/// Internal cache shared between the watcher task and resolve calls.
struct SecretCache {
    data: HashMap<String, String>,
}

/// A backend that reads key-value pairs from a Kubernetes Secret.
///
/// Starts a background watcher task that keeps the cache in sync
/// with the Secret. When the Secret is updated (via `kubectl edit`,
/// Helm upgrade, external-secrets operator, etc.), the cache is
/// refreshed automatically.
pub struct KubernetesBackend {
    cache: Arc<RwLock<SecretCache>>,
}

impl KubernetesBackend {
    /// Create a new Kubernetes backend and start the Secret watcher.
    ///
    /// Uses in-cluster config by default (service account token).
    /// Falls back to kubeconfig for local development.
    ///
    /// # Errors
    ///
    /// Returns an error if the Kubernetes client cannot be created
    /// or the initial Secret cannot be loaded.
    pub async fn new(config: &KubernetesBackendConfig) -> Result<Self, BackendError> {
        let client = Client::try_default()
            .await
            .map_err(|e| BackendError::Unavailable(format!("k8s client error: {e}")))?;

        let secrets: Api<Secret> = Api::namespaced(client, &config.namespace);

        // Initial load — non-fatal if the secret doesn't exist yet.
        // The watcher will pick it up when it's created.
        let data = match secrets.get(&config.secret_name).await {
            Ok(secret) => {
                let data = extract_secret_data(&secret);
                tracing::info!(
                    namespace = %config.namespace,
                    secret = %config.secret_name,
                    keys = data.len(),
                    "loaded Kubernetes Secret"
                );
                data
            }
            Err(e) => {
                tracing::warn!(
                    namespace = %config.namespace,
                    secret = %config.secret_name,
                    error = %e,
                    "Kubernetes Secret not found, starting with empty cache (will reload when created)"
                );
                HashMap::new()
            }
        };

        let cache = Arc::new(RwLock::new(SecretCache { data }));

        // Start background watcher.
        let watcher_cache = Arc::clone(&cache);
        let secret_name = config.secret_name.clone();
        let watcher_config =
            watcher::Config::default().fields(&format!("metadata.name={secret_name}"));

        tokio::spawn(async move {
            let stream = watcher(secrets, watcher_config);
            futures::pin_mut!(stream);

            while let Ok(Some(event)) = stream.try_next().await {
                match event {
                    watcher::Event::Apply(secret) | watcher::Event::InitApply(secret) => {
                        let new_data = extract_secret_data(&secret);
                        let key_count = new_data.len();
                        let mut c = watcher_cache.write().await;
                        c.data = new_data;
                        tracing::info!(keys = key_count, "Kubernetes Secret reloaded");
                    }
                    watcher::Event::Delete(_) => {
                        let mut c = watcher_cache.write().await;
                        c.data.clear();
                        tracing::warn!("Kubernetes Secret deleted, cache cleared");
                    }
                    watcher::Event::Init | watcher::Event::InitDone => {}
                }
            }

            tracing::warn!("Kubernetes Secret watcher stream ended");
        });

        Ok(Self { cache })
    }
}

/// Extract string key-value pairs from a Kubernetes Secret.
///
/// Secret data is base64-encoded bytes; we decode and convert to UTF-8 strings.
/// Non-UTF-8 values are skipped with a warning.
fn extract_secret_data(secret: &Secret) -> HashMap<String, String> {
    let mut result = HashMap::new();

    if let Some(data) = &secret.data {
        for (key, value) in data {
            match String::from_utf8(value.0.clone()) {
                Ok(s) => {
                    result.insert(key.clone(), s);
                }
                Err(_) => {
                    tracing::warn!(key = %key, "skipping non-UTF-8 Secret key");
                }
            }
        }
    }

    // Also check stringData (though it's rarely present in fetched Secrets).
    if let Some(string_data) = &secret.string_data {
        for (key, value) in string_data {
            result.insert(key.clone(), value.clone());
        }
    }

    result
}

impl Backend for KubernetesBackend {
    fn resolve(
        &self,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, BackendError>> + Send + 'static>> {
        let key = key.to_owned();
        let cache = Arc::clone(&self.cache);
        Box::pin(async move {
            let c = cache.read().await;
            Ok(c.data.get(&key).cloned())
        })
    }
}
