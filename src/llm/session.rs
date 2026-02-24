//! Session management for NEAR AI authentication.
//!
//! Handles session token persistence, expiration detection, and renewal via
//! OAuth flow. Tokens are stored in `~/.enclagent/session.json` and refreshed
//! automatically when expired.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use regex::Regex;
use reqwest::Client;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::error::LlmError;

/// Session data persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub session_token: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub auth_provider: Option<String>,
}

/// Configuration for session management.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Base URL for auth endpoints (e.g., https://private.near.ai).
    pub auth_base_url: String,
    /// Path to session file (e.g., ~/.enclagent/session.json).
    pub session_path: PathBuf,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            auth_base_url: "https://private.near.ai".to_string(),
            session_path: default_session_path(),
        }
    }
}

/// Get the default session file path (~/.enclagent/session.json).
pub fn default_session_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".enclagent")
        .join("session.json")
}

const SESSION_DB_KEY: &str = "nearai.session_token";
const LEGACY_SESSION_DB_KEY: &str = "nearai.session";
const AUTH_ERR_VALIDATION_REQUEST: &str = "auth.validation_request_failed";
const AUTH_ERR_VALIDATION_FAILED: &str = "auth.validation_failed";
const AUTH_ERR_CALLBACK_BIND: &str = "auth.callback_bind_failed";
const AUTH_ERR_STDIN_READ: &str = "auth.stdin_read_failed";
const AUTH_ERR_CHOICE_INVALID: &str = "auth.invalid_choice";
const AUTH_ERR_CALLBACK_FAILED: &str = "auth.callback_failed";
const AUTH_ERR_WALLET_UNSUPPORTED: &str = "auth.near_wallet_unsupported";

fn coded_auth_reason(code: &str, detail: impl std::fmt::Display) -> String {
    format!("{}: {}", code, redact_sensitive_detail(&detail.to_string()))
}

fn redact_sensitive_detail(raw: &str) -> String {
    let mut value = raw.to_string();
    let patterns = [
        (r"(?i)\b(bearer)\s+[a-z0-9._\-~+/]+=*", "$1 [REDACTED]"),
        (
            r"(?i)\b(token|api[_\-]?key|secret|password)\b(\s*[:=]\s*)([^,\s]+)",
            "$1$2[REDACTED]",
        ),
        (r"(?i)\bsk-[a-z0-9\-]{10,}\b", "sk-[REDACTED]"),
    ];

    for (pattern, replacement) in patterns {
        if let Ok(re) = Regex::new(pattern) {
            value = re.replace_all(&value, replacement).to_string();
        }
    }

    value
}

fn normalize_session_token(raw: &str) -> Result<String, String> {
    let token = raw.trim();
    if token.is_empty() {
        return Err("session token is empty".to_string());
    }
    Ok(token.to_string())
}

fn parse_created_at(value: Option<&serde_json::Value>) -> DateTime<Utc> {
    value
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

fn parse_session_data_from_value(value: serde_json::Value) -> Result<SessionData, String> {
    if let Ok(mut session) = serde_json::from_value::<SessionData>(value.clone()) {
        session.session_token = normalize_session_token(&session.session_token)?;
        return Ok(session);
    }

    if let Some(token) = value.as_str() {
        return Ok(SessionData {
            session_token: normalize_session_token(token)?,
            created_at: Utc::now(),
            auth_provider: None,
        });
    }

    if let Some(obj) = value.as_object() {
        let token = obj
            .get("session_token")
            .or_else(|| obj.get("token"))
            .or_else(|| obj.get("sessionToken"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "session JSON object missing session_token/token field".to_string())?;

        return Ok(SessionData {
            session_token: normalize_session_token(token)?,
            created_at: parse_created_at(obj.get("created_at")),
            auth_provider: obj
                .get("auth_provider")
                .and_then(|v| v.as_str())
                .map(String::from),
        });
    }

    Err("unsupported session JSON format".to_string())
}

fn parse_session_data_from_json(data: &str) -> Result<SessionData, String> {
    let value: serde_json::Value =
        serde_json::from_str(data).map_err(|e| format!("invalid JSON: {}", e))?;
    parse_session_data_from_value(value)
}

/// Manages NEAR AI session tokens with persistence and automatic renewal.
pub struct SessionManager {
    config: SessionConfig,
    client: Client,
    /// Current token in memory.
    token: RwLock<Option<SecretString>>,
    /// Prevents thundering herd during concurrent 401s.
    renewal_lock: Mutex<()>,
    /// Optional database store for persisting session to the settings table.
    store: RwLock<Option<Arc<dyn crate::db::Database>>>,
    /// User ID for DB settings (default: "default").
    user_id: RwLock<String>,
}

impl SessionManager {
    /// Create a new session manager and load any existing token from disk.
    pub fn new(config: SessionConfig) -> Self {
        let manager = Self {
            config,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
            token: RwLock::new(None),
            renewal_lock: Mutex::new(()),
            store: RwLock::new(None),
            user_id: RwLock::new("default".to_string()),
        };

        // Try to load existing session synchronously during construction
        if let Ok(data) = std::fs::read_to_string(&manager.config.session_path)
            && let Ok(session) = parse_session_data_from_json(&data)
        {
            // We can't await here, so we use try_write
            if let Ok(mut guard) = manager.token.try_write() {
                *guard = Some(SecretString::from(session.session_token));
                tracing::info!(
                    "Loaded session token from {}",
                    manager.config.session_path.display()
                );
            }
        }

        manager
    }

    /// Create a session manager and load token asynchronously.
    pub async fn new_async(config: SessionConfig) -> Self {
        let manager = Self {
            config,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
            token: RwLock::new(None),
            renewal_lock: Mutex::new(()),
            store: RwLock::new(None),
            user_id: RwLock::new("default".to_string()),
        };

        if let Err(e) = manager.load_session().await {
            tracing::debug!("No existing session found: {}", e);
        }

        manager
    }

    /// Attach a database store for persisting session tokens.
    ///
    /// When a store is attached, session tokens are saved to the `settings`
    /// table (key: `nearai.session_token`) in addition to the disk file.
    /// On load, DB is preferred over disk.
    pub async fn attach_store(&self, store: Arc<dyn crate::db::Database>, user_id: &str) {
        *self.store.write().await = Some(store);
        *self.user_id.write().await = user_id.to_string();

        // Try to load from DB (may have been saved by a previous run)
        if let Err(e) = self.load_session_from_db().await {
            tracing::debug!("No session in DB: {}", e);
            self.reconcile_db_from_memory().await;
        }
    }

    /// Get the current session token, returning an error if not authenticated.
    pub async fn get_token(&self) -> Result<SecretString, LlmError> {
        let guard = self.token.read().await;
        guard.clone().ok_or_else(|| LlmError::AuthFailed {
            provider: "nearai".to_string(),
        })
    }

    /// Check if we have a valid token (doesn't verify with server).
    pub async fn has_token(&self) -> bool {
        self.token.read().await.is_some()
    }

    /// Ensure we have a valid session, triggering login flow if needed.
    ///
    /// If no token exists, triggers the OAuth login flow. If a token exists,
    /// validates it by making a test API call. If validation fails, triggers
    /// the login flow.
    pub async fn ensure_authenticated(&self) -> Result<(), LlmError> {
        if !self.has_token().await {
            // No token, need to authenticate
            return self.initiate_login().await;
        }

        // Token exists, validate it by calling /v1/users/me
        println!("Validating session...");
        match self.validate_token().await {
            Ok(()) => {
                println!("Session valid.");
                Ok(())
            }
            Err(e) => {
                let safe_error = redact_sensitive_detail(&e.to_string());
                println!("Session expired or invalid: {}", safe_error);
                self.initiate_login().await
            }
        }
    }

    /// Validate the current token by calling the /v1/users/me endpoint.
    async fn validate_token(&self) -> Result<(), LlmError> {
        use secrecy::ExposeSecret;

        let token = self.get_token().await?;
        let url = format!("{}/v1/users/me", self.config.auth_base_url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token.expose_secret()))
            .send()
            .await
            .map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: coded_auth_reason(AUTH_ERR_VALIDATION_REQUEST, e),
            })?;

        if response.status().is_success() {
            return Ok(());
        }

        if response.status().as_u16() == 401 {
            return Err(LlmError::SessionExpired {
                provider: "nearai".to_string(),
            });
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: coded_auth_reason(
                AUTH_ERR_VALIDATION_FAILED,
                format!("HTTP {}: {}", status, body),
            ),
        })
    }

    /// Handle an authentication failure (401 response).
    ///
    /// Triggers the OAuth login flow to get a new session token.
    pub async fn handle_auth_failure(&self) -> Result<(), LlmError> {
        // Acquire renewal lock to prevent thundering herd
        let _guard = self.renewal_lock.lock().await;

        tracing::info!("Session expired or invalid, re-authenticating...");
        self.initiate_login().await
    }

    /// Start the OAuth login flow.
    ///
    /// 1. Bind the fixed callback port
    /// 2. Print the auth URL and attempt to open browser
    /// 3. Wait for OAuth callback with session token
    /// 4. Save and return the token
    async fn initiate_login(&self) -> Result<(), LlmError> {
        use crate::cli::oauth_defaults::{self, OAUTH_CALLBACK_PORT};

        let listener = oauth_defaults::bind_callback_listener()
            .await
            .map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: coded_auth_reason(AUTH_ERR_CALLBACK_BIND, e),
            })?;

        let callback_url = format!("http://127.0.0.1:{}", OAUTH_CALLBACK_PORT);

        // Show auth provider menu
        println!();
        println!("╔════════════════════════════════════════════════════════════════╗");
        println!("║                    NEAR AI Authentication                      ║");
        println!("╠════════════════════════════════════════════════════════════════╣");
        println!("║  Choose an authentication method:                              ║");
        println!("║                                                                ║");
        println!("║    [1] GitHub                                                  ║");
        println!("║    [2] Google                                                  ║");
        println!("║    [3] NEAR Wallet (coming soon)                               ║");
        println!("║                                                                ║");
        println!("╚════════════════════════════════════════════════════════════════╝");
        println!();
        print!("Enter choice [1-3]: ");

        // Flush stdout to ensure prompt is displayed
        use std::io::Write;
        std::io::stdout().flush().ok();

        // Read user choice
        let mut choice = String::new();
        std::io::stdin()
            .read_line(&mut choice)
            .map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: coded_auth_reason(AUTH_ERR_STDIN_READ, e),
            })?;

        let (auth_provider, auth_url) = match choice.trim() {
            "1" | "" => {
                let url = format!(
                    "{}/v1/auth/github?frontend_callback={}",
                    self.config.auth_base_url,
                    urlencoding::encode(&callback_url)
                );
                ("github", url)
            }
            "2" => {
                let url = format!(
                    "{}/v1/auth/google?frontend_callback={}",
                    self.config.auth_base_url,
                    urlencoding::encode(&callback_url)
                );
                ("google", url)
            }
            "3" => {
                println!();
                println!("NEAR Wallet authentication is not yet implemented.");
                println!("Please use GitHub or Google for now.");
                return Err(LlmError::SessionRenewalFailed {
                    provider: "nearai".to_string(),
                    reason: coded_auth_reason(
                        AUTH_ERR_WALLET_UNSUPPORTED,
                        "NEAR Wallet auth not yet implemented",
                    ),
                });
            }
            _ => {
                return Err(LlmError::SessionRenewalFailed {
                    provider: "nearai".to_string(),
                    reason: coded_auth_reason(
                        AUTH_ERR_CHOICE_INVALID,
                        format!("Invalid choice: {}", choice.trim()),
                    ),
                });
            }
        };

        println!();
        println!("Opening {} authentication...", auth_provider);
        println!();
        println!("  {}", auth_url);
        println!();

        // Try to open browser automatically
        if let Err(e) = open::that(&auth_url) {
            tracing::debug!("Could not open browser automatically: {}", e);
            println!("(Could not open browser automatically, please copy the URL above)");
        } else {
            println!("(Opening browser...)");
        }
        println!();
        println!("Waiting for authentication...");

        // The NEAR AI API redirects to: {frontend_callback}/auth/callback?token=X&...
        let session_token =
            oauth_defaults::wait_for_callback(listener, "/auth/callback", "token", "NEAR AI")
                .await
                .map_err(|e| LlmError::SessionRenewalFailed {
                    provider: "nearai".to_string(),
                    reason: coded_auth_reason(AUTH_ERR_CALLBACK_FAILED, e),
                })?;

        let auth_provider = Some(auth_provider.to_string());

        // Save the token
        self.save_session(&session_token, auth_provider.as_deref())
            .await?;

        // Update in-memory token
        {
            let mut guard = self.token.write().await;
            *guard = Some(SecretString::from(session_token));
        }

        println!();
        println!("✓ Authentication successful!");
        println!();

        Ok(())
    }

    /// Save session data to disk and (if available) to the database.
    async fn save_session(&self, token: &str, auth_provider: Option<&str>) -> Result<(), LlmError> {
        let session = SessionData {
            session_token: token.to_string(),
            created_at: Utc::now(),
            auth_provider: auth_provider.map(String::from),
        };

        // Save to disk (always, as bootstrap fallback)
        if let Some(parent) = self.config.session_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                LlmError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to create session directory: {}", e),
                ))
            })?;
        }

        let json =
            serde_json::to_string_pretty(&session).map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: format!("Failed to serialize session: {}", e),
            })?;

        tokio::fs::write(&self.config.session_path, json)
            .await
            .map_err(|e| {
                LlmError::Io(std::io::Error::new(
                    e.kind(),
                    format!(
                        "Failed to write session file {}: {}",
                        self.config.session_path.display(),
                        e
                    ),
                ))
            })?;

        // Restrictive permissions: session file contains a secret token
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(&self.config.session_path, perms)
                .await
                .map_err(|e| {
                    LlmError::Io(std::io::Error::new(
                        e.kind(),
                        format!(
                            "Failed to set permissions on {}: {}",
                            self.config.session_path.display(),
                            e
                        ),
                    ))
                })?;
        }

        tracing::debug!("Session saved to {}", self.config.session_path.display());

        // Also save to DB if a store is attached
        if let Some(ref store) = *self.store.read().await {
            let user_id = self.user_id.read().await.clone();
            let session_json = serde_json::to_value(&session)
                .unwrap_or(serde_json::Value::String(token.to_string()));
            if let Err(e) = store
                .set_setting(&user_id, SESSION_DB_KEY, &session_json)
                .await
            {
                tracing::warn!("Failed to save session to DB: {}", e);
            } else {
                tracing::debug!("Session also saved to DB settings");
            }
        }

        Ok(())
    }

    /// If memory already has a token (disk/env), sync it to DB as fallback.
    async fn reconcile_db_from_memory(&self) {
        use secrecy::ExposeSecret;

        let token = {
            let guard = self.token.read().await;
            guard.clone()
        };

        let Some(token) = token else {
            return;
        };

        let store = {
            let store_guard = self.store.read().await;
            store_guard.clone()
        };
        let Some(store) = store else {
            return;
        };

        let user_id = self.user_id.read().await.clone();
        let session = SessionData {
            session_token: token.expose_secret().to_string(),
            created_at: Utc::now(),
            auth_provider: None,
        };
        let session_json = serde_json::to_value(&session)
            .unwrap_or(serde_json::Value::String(session.session_token.clone()));

        if let Err(e) = store
            .set_setting(&user_id, SESSION_DB_KEY, &session_json)
            .await
        {
            tracing::warn!("Failed to reconcile in-memory session to DB: {}", e);
        } else {
            tracing::info!(
                "Reconciled in-memory session token to DB key '{}'",
                SESSION_DB_KEY
            );
        }
    }

    /// Try to load session from the database.
    async fn load_session_from_db(&self) -> Result<(), LlmError> {
        let store = {
            let store_guard = self.store.read().await;
            store_guard.clone()
        }
        .ok_or_else(|| LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: "No DB store attached".to_string(),
        })?;

        let user_id = self.user_id.read().await.clone();
        let mut parse_errors = Vec::new();

        for key in [SESSION_DB_KEY, LEGACY_SESSION_DB_KEY] {
            let Some(raw_value) = store.get_setting(&user_id, key).await.map_err(|e| {
                LlmError::SessionRenewalFailed {
                    provider: "nearai".to_string(),
                    reason: format!("DB query failed: {}", e),
                }
            })?
            else {
                continue;
            };

            match parse_session_data_from_value(raw_value.clone()) {
                Ok(session) => {
                    {
                        let mut guard = self.token.write().await;
                        *guard = Some(SecretString::from(session.session_token.clone()));
                    }

                    tracing::info!("Loaded session from DB key '{}'", key);

                    if key != SESSION_DB_KEY || !raw_value.is_object() {
                        let canonical = serde_json::to_value(&session)
                            .unwrap_or(serde_json::Value::String(session.session_token.clone()));
                        if let Err(e) = store
                            .set_setting(&user_id, SESSION_DB_KEY, &canonical)
                            .await
                        {
                            tracing::warn!(
                                "Failed to normalize session under '{}' after loading '{}': {}",
                                SESSION_DB_KEY,
                                key,
                                e
                            );
                        } else {
                            tracing::debug!(
                                "Normalized session into canonical DB key '{}'",
                                SESSION_DB_KEY
                            );
                        }
                    }

                    return Ok(());
                }
                Err(parse_err) => {
                    parse_errors.push(format!("{}: {}", key, parse_err));
                }
            }
        }

        if parse_errors.is_empty() {
            return Err(LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: "No session in DB".to_string(),
            });
        }

        Err(LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: format!(
                "Failed to parse session from DB keys [{}]",
                parse_errors.join(" | ")
            ),
        })
    }

    /// Load session data from disk.
    async fn load_session(&self) -> Result<(), LlmError> {
        let data = tokio::fs::read_to_string(&self.config.session_path)
            .await
            .map_err(|e| {
                LlmError::Io(std::io::Error::new(
                    e.kind(),
                    format!(
                        "Failed to read session file {}: {}",
                        self.config.session_path.display(),
                        e
                    ),
                ))
            })?;

        let session =
            parse_session_data_from_json(&data).map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: format!("Failed to parse session file: {}", e),
            })?;

        {
            let mut guard = self.token.write().await;
            *guard = Some(SecretString::from(session.session_token));
        }

        tracing::info!(
            "Loaded session from {} (created: {})",
            self.config.session_path.display(),
            session.created_at
        );

        Ok(())
    }

    /// Set token directly (useful for testing or migration from env var).
    pub async fn set_token(&self, token: SecretString) {
        let mut guard = self.token.write().await;
        *guard = Some(token);
    }
}

/// Create a session manager from a config, migrating from env var if present.
pub async fn create_session_manager(config: SessionConfig) -> Arc<SessionManager> {
    let manager = SessionManager::new_async(config).await;

    // Check for legacy env var and migrate if present and no file token
    if !manager.has_token().await
        && let Ok(token) = std::env::var("NEARAI_SESSION_TOKEN")
        && !token.trim().is_empty()
    {
        tracing::info!("Migrating session token from NEARAI_SESSION_TOKEN env var to file");
        let token = token.trim().to_string();
        manager.set_token(SecretString::from(token.clone())).await;
        if let Err(e) = manager.save_session(&token, None).await {
            tracing::warn!("Failed to save migrated session: {}", e);
        }
    }

    Arc::new(manager)
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use tempfile::tempdir;
    use tokio::sync::Mutex as AsyncMutex;

    /// Serializes env-mutating tests to prevent parallel races.
    static ENV_MUTEX: AsyncMutex<()> = AsyncMutex::const_new(());

    #[tokio::test]
    async fn test_session_save_load() {
        let dir = tempdir().unwrap();
        let session_path = dir.path().join("session.json");

        let config = SessionConfig {
            auth_base_url: "https://example.com".to_string(),
            session_path: session_path.clone(),
        };

        let manager = SessionManager::new_async(config.clone()).await;

        // No token initially
        assert!(!manager.has_token().await);

        // Save a token
        manager
            .save_session("test_token_123", Some("near"))
            .await
            .unwrap();
        manager
            .set_token(SecretString::from("test_token_123"))
            .await;

        // Verify it's set
        assert!(manager.has_token().await);
        let token = manager.get_token().await.unwrap();
        assert_eq!(token.expose_secret(), "test_token_123");

        // Create new manager and verify it loads the token
        let manager2 = SessionManager::new_async(config).await;
        assert!(manager2.has_token().await);
        let token2 = manager2.get_token().await.unwrap();
        assert_eq!(token2.expose_secret(), "test_token_123");

        // Verify file contents
        let data: SessionData =
            serde_json::from_str(&std::fs::read_to_string(&session_path).unwrap()).unwrap();
        assert_eq!(data.session_token, "test_token_123");
        assert_eq!(data.auth_provider, Some("near".to_string()));
    }

    #[tokio::test]
    async fn test_get_token_without_auth_fails() {
        let dir = tempdir().unwrap();
        let config = SessionConfig {
            auth_base_url: "https://example.com".to_string(),
            session_path: dir.path().join("nonexistent.json"),
        };

        let manager = SessionManager::new_async(config).await;
        let result = manager.get_token().await;
        assert!(result.is_err());
        assert!(matches!(result, Err(LlmError::AuthFailed { .. })));
    }

    #[test]
    fn test_default_session_path() {
        let path = default_session_path();
        assert!(path.ends_with("session.json"));
        assert!(path.to_string_lossy().contains(".enclagent"));
    }

    #[tokio::test]
    async fn test_load_session_from_legacy_string_file() {
        let dir = tempdir().unwrap();
        let session_path = dir.path().join("session.json");
        std::fs::write(&session_path, "\"legacy_token_abc\"").unwrap();

        let config = SessionConfig {
            auth_base_url: "https://example.com".to_string(),
            session_path,
        };

        let manager = SessionManager::new_async(config).await;
        let token = manager.get_token().await.unwrap();
        assert_eq!(token.expose_secret(), "legacy_token_abc");
    }

    #[test]
    fn test_parse_session_data_accepts_legacy_object_token_field() {
        let value = serde_json::json!({
            "token": "legacy_token_xyz",
            "auth_provider": "github"
        });

        let parsed = parse_session_data_from_value(value).unwrap();
        assert_eq!(parsed.session_token, "legacy_token_xyz");
        assert_eq!(parsed.auth_provider.as_deref(), Some("github"));
    }

    #[test]
    fn test_redact_sensitive_detail_masks_tokens() {
        let message = "request failed bearer abc.def token=abc123 api_key: xyz987";
        let redacted = redact_sensitive_detail(message);
        assert!(!redacted.contains("abc.def"));
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("xyz987"));
    }

    #[tokio::test]
    async fn test_create_session_manager_trims_legacy_env_token() {
        let _guard = ENV_MUTEX.lock().await;
        // SAFETY: Serialized by ENV_MUTEX in tests.
        unsafe {
            std::env::set_var("NEARAI_SESSION_TOKEN", "  env_token_trim_me  ");
        }

        let dir = tempdir().unwrap();
        let config = SessionConfig {
            auth_base_url: "https://example.com".to_string(),
            session_path: dir.path().join("session.json"),
        };

        let manager = create_session_manager(config).await;
        let token = manager.get_token().await.unwrap();
        assert_eq!(token.expose_secret(), "env_token_trim_me");

        // SAFETY: Serialized by ENV_MUTEX in tests.
        unsafe {
            std::env::remove_var("NEARAI_SESSION_TOKEN");
        }
    }
}
