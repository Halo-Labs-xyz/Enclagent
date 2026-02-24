//! Configuration for Enclagent.
//!
//! Settings are loaded with priority: env var > database > default.
//! `DATABASE_URL` lives in `~/.enclagent/.env` (loaded via dotenvy early
//! in startup). Everything else comes from env vars, the DB settings
//! table, or auto-detection.

mod agent;
mod builder;
mod channels;
mod database;
mod embeddings;
mod heartbeat;
pub(crate) mod helpers;
mod llm;
mod routines;
mod safety;
mod sandbox;
mod secrets;
mod skills;
mod tunnel;
mod wasm;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::error::ConfigError;
use crate::settings::Settings;

// Re-export all public types so `crate::config::FooConfig` continues to work.
pub use self::agent::AgentConfig;
pub use self::builder::BuilderModeConfig;
pub use self::channels::{ChannelsConfig, CliConfig, GatewayConfig, HttpConfig};
pub use self::database::{DatabaseBackend, DatabaseConfig, default_libsql_path};
pub use self::embeddings::EmbeddingsConfig;
pub use self::heartbeat::HeartbeatConfig;
pub use self::llm::{
    AnthropicDirectConfig, LlmBackend, LlmConfig, NearAiApiMode, NearAiConfig, OllamaConfig,
    OpenAiCompatibleConfig, OpenAiDirectConfig, TinfoilConfig,
};
pub use self::routines::RoutineConfig;
pub use self::safety::SafetyConfig;
pub use self::sandbox::{ClaudeCodeConfig, SandboxModeConfig};
pub use self::secrets::SecretsConfig;
pub use self::skills::SkillsConfig;
pub use self::tunnel::TunnelConfig;
pub use self::wasm::WasmConfig;

/// Thread-safe overlay for injected env vars (secrets loaded from DB).
///
/// Used by `inject_llm_keys_from_secrets()` to make API keys available to
/// `optional_env()` without unsafe `set_var` calls. `optional_env()` checks
/// real env vars first, then falls back to this overlay.
static INJECTED_VARS: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Main configuration for the agent.
#[derive(Debug, Clone)]
pub struct Config {
    pub database: DatabaseConfig,
    pub llm: LlmConfig,
    pub embeddings: EmbeddingsConfig,
    pub tunnel: TunnelConfig,
    pub channels: ChannelsConfig,
    pub agent: AgentConfig,
    pub safety: SafetyConfig,
    pub wasm: WasmConfig,
    pub secrets: SecretsConfig,
    pub builder: BuilderModeConfig,
    pub heartbeat: HeartbeatConfig,
    pub hyperliquid_runtime: HyperliquidRuntimeConfig,
    pub wallet_vault_policy: WalletVaultPolicyConfig,
    pub verification_backend: VerificationBackendConfig,
    pub routines: RoutineConfig,
    pub sandbox: SandboxModeConfig,
    pub claude_code: ClaudeCodeConfig,
    pub skills: SkillsConfig,
    pub observability: crate::observability::ObservabilityConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HyperliquidNetwork {
    Mainnet,
    Testnet,
}

impl HyperliquidNetwork {
    fn parse(value: &str, key: &str) -> Result<Self, ConfigError> {
        match normalize_variant(value).as_str() {
            "mainnet" => Ok(Self::Mainnet),
            "testnet" => Ok(Self::Testnet),
            _ => Err(ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!("expected 'mainnet' or 'testnet', got '{value}'"),
            }),
        }
    }

    fn default_api_base_url(self) -> &'static str {
        match self {
            Self::Mainnet => "https://api.hyperliquid.xyz",
            Self::Testnet => "https://api.hyperliquid-testnet.xyz",
        }
    }

    fn default_ws_url(self) -> &'static str {
        match self {
            Self::Mainnet => "wss://api.hyperliquid.xyz/ws",
            Self::Testnet => "wss://api.hyperliquid-testnet.xyz/ws",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperLivePolicy {
    PaperOnly,
    PaperFirst,
    LiveAllowed,
}

impl PaperLivePolicy {
    fn parse(value: &str, key: &str) -> Result<Self, ConfigError> {
        match normalize_variant(value).as_str() {
            "paper" | "paper_only" => Ok(Self::PaperOnly),
            "paper_first" => Ok(Self::PaperFirst),
            "live" | "live_allowed" => Ok(Self::LiveAllowed),
            _ => Err(ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!(
                    "expected 'paper_only', 'paper_first', or 'live_allowed', got '{value}'"
                ),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustodyMode {
    OperatorWallet,
    UserWallet,
    DualMode,
}

impl CustodyMode {
    fn parse(value: &str, key: &str) -> Result<Self, ConfigError> {
        match normalize_variant(value).as_str() {
            "operator" | "operator_wallet" => Ok(Self::OperatorWallet),
            "user" | "user_wallet" => Ok(Self::UserWallet),
            "dual" | "dual_mode" => Ok(Self::DualMode),
            _ => Err(ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!(
                    "expected 'operator_wallet', 'user_wallet', or 'dual_mode', got '{value}'"
                ),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillSwitchBehavior {
    PauseAgent,
    CancelOpenOrders,
    CancelAndFlatten,
}

impl KillSwitchBehavior {
    fn parse(value: &str, key: &str) -> Result<Self, ConfigError> {
        match normalize_variant(value).as_str() {
            "pause" | "pause_agent" => Ok(Self::PauseAgent),
            "cancel_open_orders" | "cancel_orders" => Ok(Self::CancelOpenOrders),
            "cancel_and_flatten" | "flatten" => Ok(Self::CancelAndFlatten),
            _ => Err(ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!(
                    "expected 'pause_agent', 'cancel_open_orders', or 'cancel_and_flatten', got '{value}'"
                ),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationBackendKind {
    EigenCloudPrimary,
    FallbackOnly,
}

impl VerificationBackendKind {
    fn parse(value: &str, key: &str) -> Result<Self, ConfigError> {
        match normalize_variant(value).as_str() {
            "eigencloud" | "eigencloud_primary" => Ok(Self::EigenCloudPrimary),
            "fallback" | "fallback_only" => Ok(Self::FallbackOnly),
            _ => Err(ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!("expected 'eigencloud_primary' or 'fallback_only', got '{value}'"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EigenCloudAuthScheme {
    Bearer,
    ApiKey,
}

impl EigenCloudAuthScheme {
    fn parse(value: &str, key: &str) -> Result<Self, ConfigError> {
        match normalize_variant(value).as_str() {
            "bearer" => Ok(Self::Bearer),
            "api_key" | "apikey" => Ok(Self::ApiKey),
            _ => Err(ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!("expected 'bearer' or 'api_key', got '{value}'"),
            }),
        }
    }
}

/// Hyperliquid runtime profile resolved config (Wizard Step 8).
#[derive(Debug, Clone)]
pub struct HyperliquidRuntimeConfig {
    pub network: HyperliquidNetwork,
    pub api_base_url: String,
    pub ws_url: String,
    pub timeout_ms: u64,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
    pub paper_live_policy: PaperLivePolicy,
}

impl HyperliquidRuntimeConfig {
    pub(crate) fn resolve(settings: &Settings) -> Result<Self, ConfigError> {
        let network = HyperliquidNetwork::parse(
            &helpers::optional_env("HYPERLIQUID_NETWORK")?
                .unwrap_or_else(|| settings.hyperliquid_runtime.network.clone()),
            "HYPERLIQUID_NETWORK",
        )?;

        let api_base_url = helpers::optional_env("HYPERLIQUID_API_BASE_URL")?
            .or_else(|| settings.hyperliquid_runtime.api_base_url.clone())
            .unwrap_or_else(|| network.default_api_base_url().to_string());

        let ws_url = helpers::optional_env("HYPERLIQUID_WS_URL")?
            .or_else(|| settings.hyperliquid_runtime.ws_url.clone())
            .unwrap_or_else(|| network.default_ws_url().to_string());

        let timeout_ms = helpers::optional_env("HYPERLIQUID_TIMEOUT_MS")?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| ConfigError::InvalidValue {
                key: "HYPERLIQUID_TIMEOUT_MS".to_string(),
                message: format!("must be a positive integer: {e}"),
            })?
            .unwrap_or(settings.hyperliquid_runtime.timeout_ms);
        if timeout_ms == 0 {
            return Err(ConfigError::InvalidValue {
                key: "HYPERLIQUID_TIMEOUT_MS".to_string(),
                message: "must be > 0".to_string(),
            });
        }

        let max_retries = helpers::optional_env("HYPERLIQUID_MAX_RETRIES")?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| ConfigError::InvalidValue {
                key: "HYPERLIQUID_MAX_RETRIES".to_string(),
                message: format!("must be a non-negative integer: {e}"),
            })?
            .unwrap_or(settings.hyperliquid_runtime.max_retries);

        let retry_backoff_ms = helpers::optional_env("HYPERLIQUID_RETRY_BACKOFF_MS")?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| ConfigError::InvalidValue {
                key: "HYPERLIQUID_RETRY_BACKOFF_MS".to_string(),
                message: format!("must be a non-negative integer: {e}"),
            })?
            .unwrap_or(settings.hyperliquid_runtime.retry_backoff_ms);

        let paper_live_policy = PaperLivePolicy::parse(
            &helpers::optional_env("HYPERLIQUID_PAPER_LIVE_POLICY")?
                .unwrap_or_else(|| settings.hyperliquid_runtime.paper_live_policy.clone()),
            "HYPERLIQUID_PAPER_LIVE_POLICY",
        )?;

        Ok(Self {
            network,
            api_base_url,
            ws_url,
            timeout_ms,
            max_retries,
            retry_backoff_ms,
            paper_live_policy,
        })
    }
}

/// Wallet and vault policy resolved config (Wizard Step 9).
#[derive(Debug, Clone)]
pub struct WalletVaultPolicyConfig {
    pub custody_mode: CustodyMode,
    pub operator_wallet_address: Option<String>,
    pub user_wallet_address: Option<String>,
    pub vault_address: Option<String>,
    pub max_position_size_usd: u64,
    pub leverage_cap: u32,
    pub kill_switch_enabled: bool,
    pub kill_switch_behavior: KillSwitchBehavior,
}

impl WalletVaultPolicyConfig {
    pub(crate) fn resolve(settings: &Settings) -> Result<Self, ConfigError> {
        let custody_mode = CustodyMode::parse(
            &helpers::optional_env("HYPERLIQUID_CUSTODY_MODE")?
                .unwrap_or_else(|| settings.wallet_vault_policy.custody_mode.clone()),
            "HYPERLIQUID_CUSTODY_MODE",
        )?;

        let max_position_size_usd = helpers::optional_env("HYPERLIQUID_MAX_POSITION_SIZE_USD")?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| ConfigError::InvalidValue {
                key: "HYPERLIQUID_MAX_POSITION_SIZE_USD".to_string(),
                message: format!("must be a positive integer: {e}"),
            })?
            .unwrap_or(settings.wallet_vault_policy.max_position_size_usd);
        if max_position_size_usd == 0 {
            return Err(ConfigError::InvalidValue {
                key: "HYPERLIQUID_MAX_POSITION_SIZE_USD".to_string(),
                message: "must be > 0".to_string(),
            });
        }

        let leverage_cap = helpers::optional_env("HYPERLIQUID_LEVERAGE_CAP")?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| ConfigError::InvalidValue {
                key: "HYPERLIQUID_LEVERAGE_CAP".to_string(),
                message: format!("must be a positive integer: {e}"),
            })?
            .unwrap_or(settings.wallet_vault_policy.leverage_cap);
        if leverage_cap == 0 {
            return Err(ConfigError::InvalidValue {
                key: "HYPERLIQUID_LEVERAGE_CAP".to_string(),
                message: "must be > 0".to_string(),
            });
        }

        let kill_switch_enabled = helpers::optional_env("HYPERLIQUID_KILL_SWITCH_ENABLED")?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| ConfigError::InvalidValue {
                key: "HYPERLIQUID_KILL_SWITCH_ENABLED".to_string(),
                message: format!("must be 'true' or 'false': {e}"),
            })?
            .unwrap_or(settings.wallet_vault_policy.kill_switch_enabled);

        let kill_switch_behavior = KillSwitchBehavior::parse(
            &helpers::optional_env("HYPERLIQUID_KILL_SWITCH_BEHAVIOR")?
                .unwrap_or_else(|| settings.wallet_vault_policy.kill_switch_behavior.clone()),
            "HYPERLIQUID_KILL_SWITCH_BEHAVIOR",
        )?;

        Ok(Self {
            custody_mode,
            operator_wallet_address: helpers::optional_env("HYPERLIQUID_OPERATOR_WALLET_ADDRESS")?
                .or_else(|| settings.wallet_vault_policy.operator_wallet_address.clone()),
            user_wallet_address: helpers::optional_env("HYPERLIQUID_USER_WALLET_ADDRESS")?
                .or_else(|| settings.wallet_vault_policy.user_wallet_address.clone()),
            vault_address: helpers::optional_env("HYPERLIQUID_VAULT_ADDRESS")?
                .or_else(|| settings.wallet_vault_policy.vault_address.clone()),
            max_position_size_usd,
            leverage_cap,
            kill_switch_enabled,
            kill_switch_behavior,
        })
    }
}

/// Verification backend resolved config (Wizard Step 10).
#[derive(Debug, Clone)]
pub struct VerificationBackendConfig {
    pub backend: VerificationBackendKind,
    pub eigencloud: EigenCloudConfig,
    pub fallback: SignedFallbackReceiptChainConfig,
}

#[derive(Debug, Clone)]
pub struct EigenCloudConfig {
    pub endpoint: Option<String>,
    pub auth_scheme: EigenCloudAuthScheme,
    pub auth_token: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SignedFallbackReceiptChainConfig {
    pub enabled: bool,
    pub signing_key_id: Option<String>,
    pub chain_path: PathBuf,
    pub require_signed_receipts: bool,
}

impl VerificationBackendConfig {
    pub(crate) fn resolve(settings: &Settings) -> Result<Self, ConfigError> {
        let backend = VerificationBackendKind::parse(
            &helpers::optional_env("VERIFICATION_BACKEND")?
                .unwrap_or_else(|| settings.verification_backend.backend.clone()),
            "VERIFICATION_BACKEND",
        )?;

        let auth_scheme = EigenCloudAuthScheme::parse(
            &helpers::optional_env("EIGENCLOUD_AUTH_SCHEME")?
                .unwrap_or_else(|| settings.verification_backend.eigencloud_auth_scheme.clone()),
            "EIGENCLOUD_AUTH_SCHEME",
        )?;

        let timeout_ms = helpers::optional_env("EIGENCLOUD_TIMEOUT_MS")?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| ConfigError::InvalidValue {
                key: "EIGENCLOUD_TIMEOUT_MS".to_string(),
                message: format!("must be a positive integer: {e}"),
            })?
            .unwrap_or(settings.verification_backend.eigencloud_timeout_ms);
        if timeout_ms == 0 {
            return Err(ConfigError::InvalidValue {
                key: "EIGENCLOUD_TIMEOUT_MS".to_string(),
                message: "must be > 0".to_string(),
            });
        }

        let fallback_enabled = helpers::optional_env("VERIFICATION_FALLBACK_ENABLED")?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| ConfigError::InvalidValue {
                key: "VERIFICATION_FALLBACK_ENABLED".to_string(),
                message: format!("must be 'true' or 'false': {e}"),
            })?
            .unwrap_or(settings.verification_backend.fallback_enabled);

        if backend == VerificationBackendKind::FallbackOnly && !fallback_enabled {
            return Err(ConfigError::InvalidValue {
                key: "VERIFICATION_FALLBACK_ENABLED".to_string(),
                message: "must be enabled when VERIFICATION_BACKEND=fallback_only".to_string(),
            });
        }

        let chain_path = helpers::optional_env("VERIFICATION_FALLBACK_CHAIN_PATH")?
            .map(PathBuf::from)
            .or_else(|| settings.verification_backend.fallback_chain_path.clone())
            .unwrap_or_else(default_fallback_receipt_chain_path);

        let require_signed_receipts =
            helpers::optional_env("VERIFICATION_FALLBACK_REQUIRE_SIGNED_RECEIPTS")?
                .map(|s| s.parse())
                .transpose()
                .map_err(|e| ConfigError::InvalidValue {
                    key: "VERIFICATION_FALLBACK_REQUIRE_SIGNED_RECEIPTS".to_string(),
                    message: format!("must be 'true' or 'false': {e}"),
                })?
                .unwrap_or(
                    settings
                        .verification_backend
                        .fallback_require_signed_receipts,
                );

        Ok(Self {
            backend,
            eigencloud: EigenCloudConfig {
                endpoint: helpers::optional_env("EIGENCLOUD_ENDPOINT")?
                    .or_else(|| settings.verification_backend.eigencloud_endpoint.clone()),
                auth_scheme,
                auth_token: helpers::optional_env("EIGENCLOUD_AUTH_TOKEN")?
                    .or_else(|| settings.verification_backend.eigencloud_auth_token.clone()),
                timeout_ms,
            },
            fallback: SignedFallbackReceiptChainConfig {
                enabled: fallback_enabled,
                signing_key_id: helpers::optional_env("VERIFICATION_FALLBACK_SIGNING_KEY_ID")?
                    .or_else(|| {
                        settings
                            .verification_backend
                            .fallback_signing_key_id
                            .clone()
                    }),
                chain_path,
                require_signed_receipts,
            },
        })
    }
}

fn normalize_variant(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn default_fallback_receipt_chain_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".enclagent")
        .join("receipts")
        .join("receipt-chain.ndjson")
}

impl Config {
    /// Load configuration from environment variables and the database.
    ///
    /// Priority: env var > TOML config file > DB settings > default.
    /// This is the primary way to load config after DB is connected.
    pub async fn from_db(
        store: &(dyn crate::db::SettingsStore + Sync),
        user_id: &str,
    ) -> Result<Self, ConfigError> {
        Self::from_db_with_toml(store, user_id, None).await
    }

    /// Load from DB with an optional TOML config file overlay.
    pub async fn from_db_with_toml(
        store: &(dyn crate::db::SettingsStore + Sync),
        user_id: &str,
        toml_path: Option<&std::path::Path>,
    ) -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
        crate::bootstrap::load_enclagent_env();

        // Load all settings from DB into a Settings struct
        let mut db_settings = match store.get_all_settings(user_id).await {
            Ok(map) => Settings::from_db_map(&map),
            Err(e) => {
                tracing::warn!("Failed to load settings from DB, using defaults: {}", e);
                Settings::default()
            }
        };

        // Overlay TOML config file (values win over DB settings)
        Self::apply_toml_overlay(&mut db_settings, toml_path)?;

        Self::build(&db_settings).await
    }

    /// Load configuration from environment variables only (no database).
    ///
    /// Used during early startup before the database is connected,
    /// and by CLI commands that don't have DB access.
    /// Falls back to legacy `settings.json` on disk if present.
    ///
    /// Loads both `./.env` (standard, higher priority) and `~/.enclagent/.env`
    /// (lower priority) via dotenvy, which never overwrites existing vars.
    pub async fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_with_toml(None).await
    }

    /// Load from env with an optional TOML config file overlay.
    pub async fn from_env_with_toml(
        toml_path: Option<&std::path::Path>,
    ) -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
        crate::bootstrap::load_enclagent_env();
        let mut settings = Settings::load();

        // Overlay TOML config file (values win over JSON settings)
        Self::apply_toml_overlay(&mut settings, toml_path)?;

        Self::build(&settings).await
    }

    /// Load and merge a TOML config file into settings.
    ///
    /// If `explicit_path` is `Some`, loads from that path (errors are fatal).
    /// If `None`, tries the default path `~/.enclagent/config.toml` (missing
    /// file is silently ignored).
    fn apply_toml_overlay(
        settings: &mut Settings,
        explicit_path: Option<&std::path::Path>,
    ) -> Result<(), ConfigError> {
        let path = explicit_path
            .map(std::path::PathBuf::from)
            .unwrap_or_else(Settings::default_toml_path);

        match Settings::load_toml(&path) {
            Ok(Some(toml_settings)) => {
                settings.merge_from(&toml_settings);
                tracing::debug!("Loaded TOML config from {}", path.display());
            }
            Ok(None) => {
                if explicit_path.is_some() {
                    return Err(ConfigError::ParseError(format!(
                        "Config file not found: {}",
                        path.display()
                    )));
                }
            }
            Err(e) => {
                if explicit_path.is_some() {
                    return Err(ConfigError::ParseError(format!(
                        "Failed to load config file {}: {}",
                        path.display(),
                        e
                    )));
                }
                tracing::warn!("Failed to load default config file: {}", e);
            }
        }
        Ok(())
    }

    /// Build config from settings (shared by from_env and from_db).
    async fn build(settings: &Settings) -> Result<Self, ConfigError> {
        Ok(Self {
            database: DatabaseConfig::resolve()?,
            llm: LlmConfig::resolve(settings)?,
            embeddings: EmbeddingsConfig::resolve(settings)?,
            tunnel: TunnelConfig::resolve(settings)?,
            channels: ChannelsConfig::resolve(settings)?,
            agent: AgentConfig::resolve(settings)?,
            safety: SafetyConfig::resolve()?,
            wasm: WasmConfig::resolve()?,
            secrets: SecretsConfig::resolve().await?,
            builder: BuilderModeConfig::resolve()?,
            heartbeat: HeartbeatConfig::resolve(settings)?,
            hyperliquid_runtime: HyperliquidRuntimeConfig::resolve(settings)?,
            wallet_vault_policy: WalletVaultPolicyConfig::resolve(settings)?,
            verification_backend: VerificationBackendConfig::resolve(settings)?,
            routines: RoutineConfig::resolve()?,
            sandbox: SandboxModeConfig::resolve()?,
            claude_code: ClaudeCodeConfig::resolve()?,
            skills: SkillsConfig::resolve()?,
            observability: crate::observability::ObservabilityConfig {
                backend: std::env::var("OBSERVABILITY_BACKEND").unwrap_or_else(|_| "none".into()),
            },
        })
    }
}

/// Load API keys from the encrypted secrets store into a thread-safe overlay.
///
/// This bridges the gap between secrets stored during onboarding and the
/// env-var-first resolution in `LlmConfig::resolve()`. Keys in the overlay
/// are read by `optional_env()` before falling back to `std::env::var()`,
/// so explicit env vars always win.
pub async fn inject_llm_keys_from_secrets(
    secrets: &dyn crate::secrets::SecretsStore,
    user_id: &str,
) {
    let mappings = [
        ("llm_openai_api_key", "OPENAI_API_KEY"),
        ("llm_anthropic_api_key", "ANTHROPIC_API_KEY"),
        ("llm_compatible_api_key", "LLM_API_KEY"),
    ];

    let mut injected = HashMap::new();

    for (secret_name, env_var) in mappings {
        match std::env::var(env_var) {
            Ok(val) if !val.is_empty() => continue,
            _ => {}
        }
        match secrets.get_decrypted(user_id, secret_name).await {
            Ok(decrypted) => {
                injected.insert(env_var.to_string(), decrypted.expose().to_string());
                tracing::debug!("Loaded secret '{}' for env var '{}'", secret_name, env_var);
            }
            Err(_) => {
                // Secret doesn't exist, that's fine
            }
        }
    }

    let _ = INJECTED_VARS.set(injected);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ConfigError;
    use crate::settings::Settings;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn clear_hl_policy_env() {
        // SAFETY: Guarded by ENV_MUTEX in tests.
        unsafe {
            std::env::remove_var("HYPERLIQUID_NETWORK");
            std::env::remove_var("HYPERLIQUID_API_BASE_URL");
            std::env::remove_var("HYPERLIQUID_WS_URL");
            std::env::remove_var("HYPERLIQUID_TIMEOUT_MS");
            std::env::remove_var("HYPERLIQUID_MAX_RETRIES");
            std::env::remove_var("HYPERLIQUID_RETRY_BACKOFF_MS");
            std::env::remove_var("HYPERLIQUID_PAPER_LIVE_POLICY");
            std::env::remove_var("HYPERLIQUID_CUSTODY_MODE");
            std::env::remove_var("HYPERLIQUID_OPERATOR_WALLET_ADDRESS");
            std::env::remove_var("HYPERLIQUID_USER_WALLET_ADDRESS");
            std::env::remove_var("HYPERLIQUID_VAULT_ADDRESS");
            std::env::remove_var("HYPERLIQUID_MAX_POSITION_SIZE_USD");
            std::env::remove_var("HYPERLIQUID_LEVERAGE_CAP");
            std::env::remove_var("HYPERLIQUID_KILL_SWITCH_ENABLED");
            std::env::remove_var("HYPERLIQUID_KILL_SWITCH_BEHAVIOR");
            std::env::remove_var("VERIFICATION_BACKEND");
            std::env::remove_var("EIGENCLOUD_ENDPOINT");
            std::env::remove_var("EIGENCLOUD_AUTH_SCHEME");
            std::env::remove_var("EIGENCLOUD_AUTH_TOKEN");
            std::env::remove_var("EIGENCLOUD_TIMEOUT_MS");
            std::env::remove_var("VERIFICATION_FALLBACK_ENABLED");
            std::env::remove_var("VERIFICATION_FALLBACK_SIGNING_KEY_ID");
            std::env::remove_var("VERIFICATION_FALLBACK_CHAIN_PATH");
            std::env::remove_var("VERIFICATION_FALLBACK_REQUIRE_SIGNED_RECEIPTS");
        }
    }

    #[test]
    fn ws1_resolvers_use_safe_defaults() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_hl_policy_env();

        let settings = Settings::default();

        let runtime = HyperliquidRuntimeConfig::resolve(&settings).expect("runtime resolve");
        assert_eq!(runtime.network, HyperliquidNetwork::Testnet);
        assert_eq!(runtime.api_base_url, "https://api.hyperliquid-testnet.xyz");
        assert_eq!(runtime.ws_url, "wss://api.hyperliquid-testnet.xyz/ws");
        assert_eq!(runtime.paper_live_policy, PaperLivePolicy::PaperFirst);

        let wallet = WalletVaultPolicyConfig::resolve(&settings).expect("wallet resolve");
        assert_eq!(wallet.custody_mode, CustodyMode::OperatorWallet);
        assert_eq!(wallet.max_position_size_usd, 1_000);
        assert_eq!(wallet.leverage_cap, 2);
        assert!(wallet.kill_switch_enabled);

        let verification = VerificationBackendConfig::resolve(&settings).expect("verify resolve");
        assert_eq!(
            verification.backend,
            VerificationBackendKind::EigenCloudPrimary
        );
        assert!(verification.fallback.enabled);
        assert!(verification.fallback.require_signed_receipts);
        assert!(
            verification
                .fallback
                .chain_path
                .to_string_lossy()
                .contains(".enclagent/receipts")
        );
    }

    #[test]
    fn ws1_resolvers_apply_env_overrides() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_hl_policy_env();

        // SAFETY: Guarded by ENV_MUTEX in tests.
        unsafe {
            std::env::set_var("HYPERLIQUID_NETWORK", "mainnet");
            std::env::set_var("HYPERLIQUID_PAPER_LIVE_POLICY", "paper_only");
            std::env::set_var("HYPERLIQUID_MAX_POSITION_SIZE_USD", "500");
            std::env::set_var("VERIFICATION_BACKEND", "fallback_only");
            std::env::set_var(
                "VERIFICATION_FALLBACK_CHAIN_PATH",
                "/tmp/ws1-receipt-chain.ndjson",
            );
        }

        let mut settings = Settings::default();
        settings.wallet_vault_policy.max_position_size_usd = 20_000;

        let runtime = HyperliquidRuntimeConfig::resolve(&settings).expect("runtime resolve");
        assert_eq!(runtime.network, HyperliquidNetwork::Mainnet);
        assert_eq!(runtime.api_base_url, "https://api.hyperliquid.xyz");
        assert_eq!(runtime.paper_live_policy, PaperLivePolicy::PaperOnly);

        let wallet = WalletVaultPolicyConfig::resolve(&settings).expect("wallet resolve");
        assert_eq!(wallet.max_position_size_usd, 500);

        let verification = VerificationBackendConfig::resolve(&settings).expect("verify resolve");
        assert_eq!(verification.backend, VerificationBackendKind::FallbackOnly);
        assert_eq!(
            verification.fallback.chain_path,
            PathBuf::from("/tmp/ws1-receipt-chain.ndjson")
        );

        clear_hl_policy_env();
    }

    #[test]
    fn ws1_resolvers_reject_invalid_policy_values() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_hl_policy_env();

        // SAFETY: Guarded by ENV_MUTEX in tests.
        unsafe {
            std::env::set_var("HYPERLIQUID_PAPER_LIVE_POLICY", "unsafe_mode");
        }

        let err = HyperliquidRuntimeConfig::resolve(&Settings::default()).unwrap_err();
        match err {
            ConfigError::InvalidValue { key, .. } => {
                assert_eq!(key, "HYPERLIQUID_PAPER_LIVE_POLICY")
            }
            other => panic!("unexpected error: {other}"),
        }

        clear_hl_policy_env();
    }
}
