//! Vault backend for resolving environment variables from HashiCorp Vault.
//!
//! Supports the `vault:` prefix convention (bank-vaults style):
//! ```text
//! vault:secret/data/myapp/config#DATABASE_URL
//! vault:secret/data/myapp/config#DATABASE_URL#3   (specific version)
//! ```
//!
//! The agent receives the `vault:` prefixed value via the v2 wire protocol,
//! parses the mount/path/key, authenticates to Vault, fetches the secret,
//! caches it with a TTL, and returns the resolved value.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use vaultrs::client::{VaultClient, VaultClientSettingsBuilder};
use vaultrs::kv2;

use envproxy_proto::{parse_vault_ref, VaultRef};

use super::{Backend, BackendError};

/// Configuration for the Vault backend.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct VaultBackendConfig {
    /// Vault server address (e.g., `https://vault.internal:8200`).
    pub address: String,

    /// Authentication method: `"kubernetes"`, `"token"`.
    #[serde(default = "default_auth_method")]
    pub auth_method: String,

    /// Vault auth mount path (e.g., `"kubernetes"` for `auth/kubernetes`).
    #[serde(default = "default_auth_mount")]
    pub auth_mount: String,

    /// Vault role name for authentication.
    #[serde(default)]
    pub role: String,

    /// Direct Vault token (for `auth_method = "token"`).
    #[serde(default)]
    pub token: Option<String>,

    /// Path to a file containing the Vault token.
    #[serde(default)]
    pub token_file: Option<String>,

    /// Path to CA certificate for Vault TLS.
    #[serde(default)]
    pub ca_cert: Option<String>,

    /// Skip TLS verification (development only).
    #[serde(default)]
    pub skip_verify: bool,

    /// Cache TTL for resolved secrets (e.g., `"5m"`, `"30s"`).
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl: String,
}

fn default_auth_method() -> String {
    "kubernetes".to_owned()
}

fn default_auth_mount() -> String {
    "kubernetes".to_owned()
}

fn default_cache_ttl() -> String {
    "5m".to_owned()
}

/// A cached secret value with expiry time.
struct CacheEntry {
    value: String,
    expires_at: Instant,
}

/// Vault backend that resolves `vault:path#key` references.
pub struct VaultBackend {
    client: Arc<RwLock<VaultClient>>,
    config: VaultBackendConfig,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    cache_ttl: Duration,
}

impl VaultBackend {
    /// Create a new Vault backend and authenticate.
    ///
    /// # Errors
    ///
    /// Returns an error if the Vault client cannot be created or authentication fails.
    pub async fn new(config: &VaultBackendConfig) -> Result<Self, BackendError> {
        let cache_ttl = parse_duration(&config.cache_ttl).ok_or_else(|| {
            BackendError::Parse(format!("invalid cache_ttl: {}", config.cache_ttl))
        })?;

        let mut settings_builder = VaultClientSettingsBuilder::default();
        settings_builder.address(&config.address);

        // Set token if using token auth, otherwise use empty token (will auth later).
        let initial_token = match &config.auth_method[..] {
            "token" => {
                if let Some(ref token) = config.token {
                    token.clone()
                } else if let Some(ref token_file) = config.token_file {
                    std::fs::read_to_string(token_file)
                        .map_err(|e| BackendError::Io(e))?
                        .trim()
                        .to_owned()
                } else if let Ok(token) = std::env::var("VAULT_TOKEN") {
                    token
                } else {
                    return Err(BackendError::Unavailable(
                        "token auth requires token, token_file, or VAULT_TOKEN env var".into(),
                    ));
                }
            }
            _ => String::new(), // Will authenticate below.
        };

        settings_builder.token(&initial_token);

        let settings = settings_builder
            .build()
            .map_err(|e| BackendError::Unavailable(format!("vault client settings: {e}")))?;

        let client = VaultClient::new(settings)
            .map_err(|e| BackendError::Unavailable(format!("vault client: {e}")))?;

        let client = Arc::new(RwLock::new(client));

        let backend = Self {
            client: Arc::clone(&client),
            config: config.clone(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl,
        };

        // Authenticate if not using token auth.
        if config.auth_method != "token" {
            backend.authenticate().await?;
        }

        tracing::info!(
            address = %config.address,
            auth_method = %config.auth_method,
            role = %config.role,
            cache_ttl = ?cache_ttl,
            "Vault backend initialized"
        );

        Ok(backend)
    }

    /// Authenticate to Vault using the configured auth method.
    async fn authenticate(&self) -> Result<(), BackendError> {
        match self.config.auth_method.as_str() {
            "kubernetes" => self.auth_kubernetes().await,
            "token" => Ok(()), // Already has token.
            other => Err(BackendError::Unavailable(format!("unsupported auth method: {other}"))),
        }
    }

    /// Authenticate using the Kubernetes auth method.
    ///
    /// Reads the service account JWT token from the standard path
    /// and posts it to Vault's `auth/kubernetes/login` endpoint.
    async fn auth_kubernetes(&self) -> Result<(), BackendError> {
        let jwt = tokio::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/token")
            .await
            .map_err(|e| BackendError::Unavailable(format!("failed to read SA token: {e}")))?;

        // Use the current client (with empty token) to authenticate.
        let current_client = self.client.read().await;

        let auth_info = vaultrs::auth::kubernetes::login(
            &*current_client,
            &self.config.auth_mount,
            &self.config.role,
            &jwt,
        )
        .await
        .map_err(|e| BackendError::Unavailable(format!("vault kubernetes auth: {e}")))?;

        drop(current_client);

        // Create a new client with the authenticated token.
        let settings = VaultClientSettingsBuilder::default()
            .address(&self.config.address)
            .token(&auth_info.client_token)
            .build()
            .map_err(|e| BackendError::Unavailable(format!("vault client settings: {e}")))?;

        let new_client = VaultClient::new(settings)
            .map_err(|e| BackendError::Unavailable(format!("vault client: {e}")))?;

        // Replace the client.
        let mut client = self.client.write().await;
        *client = new_client;
        drop(client);

        tracing::info!(
            role = %self.config.role,
            lease_duration = auth_info.lease_duration,
            "authenticated to Vault via Kubernetes auth"
        );

        // Spawn token renewal task.
        let renewal_client = Arc::clone(&self.client);
        let config_addr = self.config.address.clone();
        let lease_duration = auth_info.lease_duration;
        tokio::spawn(async move {
            token_renewal_loop(renewal_client, config_addr, lease_duration).await;
        });

        Ok(())
    }
}

/// Background task that renews the Vault token before it expires.
async fn token_renewal_loop(
    client: Arc<RwLock<VaultClient>>,
    _address: String,
    lease_duration: u64,
) {
    // Renew at 2/3 of the lease duration.
    let renew_interval = Duration::from_secs(lease_duration * 2 / 3);
    let min_interval = Duration::from_secs(10);
    let interval = renew_interval.max(min_interval);

    loop {
        tokio::time::sleep(interval).await;

        let client = client.read().await;
        match vaultrs::token::renew_self(&*client, None).await {
            Ok(auth) => {
                tracing::debug!(lease_duration = auth.lease_duration, "Vault token renewed");
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to renew Vault token");
                // Token renewal failed — the next secret fetch will fail too,
                // and the caller should see a BackendError. A full re-auth
                // would require the auth method config, which this loop doesn't have.
                // For now, log the error and continue trying.
            }
        }
    }
}

/// Parse a duration string like "5m", "30s", "1h" into a `Duration`.
fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, suffix) = if s.ends_with('s') {
        (&s[..s.len() - 1], "s")
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], "m")
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], "h")
    } else {
        // Assume seconds if no suffix.
        (s, "s")
    };

    let num: u64 = num_str.parse().ok()?;
    let secs = match suffix {
        "s" => num,
        "m" => num * 60,
        "h" => num * 3600,
        _ => return None,
    };

    Some(Duration::from_secs(secs))
}

impl Backend for VaultBackend {
    fn resolve(
        &self,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, BackendError>> + Send + 'static>> {
        // v1 protocol: key only. For Vault, we need the vault: prefixed value.
        // Without it, we can't resolve.
        let key = key.to_owned();
        Box::pin(async move {
            tracing::debug!(key = %key, "vault backend: key-only resolve (no vault: ref), returning None");
            Ok(None)
        })
    }

    fn resolve_with_value(
        &self,
        _key: &str,
        value: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, BackendError>> + Send + 'static>> {
        let vault_value = value.to_owned();
        let this_client = Arc::clone(&self.client);
        let this_cache = Arc::clone(&self.cache);
        let this_cache_ttl = self.cache_ttl;

        Box::pin(async move {
            let vault_ref = parse_vault_ref(&vault_value).ok_or_else(|| {
                BackendError::Parse(format!("invalid vault reference: {vault_value}"))
            })?;

            // Check cache.
            {
                let cache = this_cache.read().await;
                if let Some(entry) = cache.get(&vault_value) {
                    if entry.expires_at > Instant::now() {
                        return Ok(Some(entry.value.clone()));
                    }
                }
            }

            // Fetch from Vault.
            let client = this_client.read().await;
            let secret: HashMap<String, String> = match vault_ref.version {
                Some(ver) => kv2::read_version(&*client, &vault_ref.mount, &vault_ref.path, ver)
                    .await
                    .map_err(|e| BackendError::Http(format!("vault read: {e}")))?,
                None => kv2::read(&*client, &vault_ref.mount, &vault_ref.path)
                    .await
                    .map_err(|e| BackendError::Http(format!("vault read: {e}")))?,
            };
            drop(client);

            tracing::debug!(
                mount = %vault_ref.mount,
                path = %vault_ref.path,
                key = %vault_ref.key,
                keys_in_secret = secret.len(),
                "fetched Vault secret"
            );

            // Cache all keys from this path.
            {
                let mut cache = this_cache.write().await;
                let expires_at = Instant::now() + this_cache_ttl;
                for (k, v) in &secret {
                    let ref_str = if vault_ref.version.is_some() {
                        format!(
                            "vault:{}/data/{}#{}#{}",
                            vault_ref.mount,
                            vault_ref.path,
                            k,
                            vault_ref.version.unwrap_or(0)
                        )
                    } else {
                        format!("vault:{}/data/{}#{}", vault_ref.mount, vault_ref.path, k)
                    };
                    cache.insert(ref_str, CacheEntry { value: v.clone(), expires_at });
                }
            }

            Ok(secret.get(&vault_ref.key).cloned())
        })
    }
}
