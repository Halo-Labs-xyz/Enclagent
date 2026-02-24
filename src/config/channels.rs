use std::path::PathBuf;

use secrecy::SecretString;

use crate::config::helpers::optional_env;
use crate::error::ConfigError;
use crate::settings::Settings;

/// Channel configurations.
#[derive(Debug, Clone)]
pub struct ChannelsConfig {
    pub cli: CliConfig,
    pub http: Option<HttpConfig>,
    pub gateway: Option<GatewayConfig>,
    /// Directory containing WASM channel modules (default: ~/.enclagent/channels/).
    pub wasm_channels_dir: std::path::PathBuf,
    /// Whether WASM channels are enabled.
    pub wasm_channels_enabled: bool,
    /// Telegram owner user ID. When set, the bot only responds to this user.
    pub telegram_owner_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct CliConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub host: String,
    pub port: u16,
    pub webhook_secret: Option<SecretString>,
    pub user_id: String,
}

/// Web gateway configuration.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub host: String,
    pub port: u16,
    /// Bearer token for authentication. Random hex generated at startup if unset.
    pub auth_token: Option<String>,
    pub user_id: String,
    pub frontdoor: Option<GatewayFrontdoorConfig>,
}

/// Frontdoor provisioning configuration for user-facing enclave onboarding.
#[derive(Debug, Clone)]
pub struct GatewayFrontdoorConfig {
    pub require_privy: bool,
    pub privy_app_id: Option<String>,
    pub privy_client_id: Option<String>,
    pub provision_command: Option<String>,
    pub default_instance_url: Option<String>,
    pub verify_app_base_url: Option<String>,
    pub session_ttl_secs: u64,
    pub poll_interval_ms: u64,
}

impl ChannelsConfig {
    pub(crate) fn resolve(settings: &Settings) -> Result<Self, ConfigError> {
        let http = if optional_env("HTTP_PORT")?.is_some() || optional_env("HTTP_HOST")?.is_some() {
            Some(HttpConfig {
                host: optional_env("HTTP_HOST")?.unwrap_or_else(|| "0.0.0.0".to_string()),
                port: optional_env("HTTP_PORT")?
                    .map(|s| s.parse())
                    .transpose()
                    .map_err(|e| ConfigError::InvalidValue {
                        key: "HTTP_PORT".to_string(),
                        message: format!("must be a valid port number: {e}"),
                    })?
                    .unwrap_or(8080),
                webhook_secret: optional_env("HTTP_WEBHOOK_SECRET")?.map(SecretString::from),
                user_id: optional_env("HTTP_USER_ID")?.unwrap_or_else(|| "http".to_string()),
            })
        } else {
            None
        };

        let gateway = if optional_env("GATEWAY_ENABLED")?
            .map(|s| s.to_lowercase() == "true" || s == "1")
            .unwrap_or(true)
        {
            let frontdoor_enabled = optional_env("GATEWAY_FRONTDOOR_ENABLED")?
                .map(|s| s.to_lowercase() == "true" || s == "1")
                .unwrap_or(false);
            let frontdoor = if frontdoor_enabled {
                Some(GatewayFrontdoorConfig {
                    require_privy: optional_env("GATEWAY_FRONTDOOR_REQUIRE_PRIVY")?
                        .map(|s| s.to_lowercase() != "false" && s != "0")
                        .unwrap_or(true),
                    privy_app_id: optional_env("GATEWAY_FRONTDOOR_PRIVY_APP_ID")?,
                    privy_client_id: optional_env("GATEWAY_FRONTDOOR_PRIVY_CLIENT_ID")?,
                    provision_command: optional_env("GATEWAY_FRONTDOOR_PROVISION_COMMAND")?,
                    default_instance_url: optional_env("GATEWAY_FRONTDOOR_DEFAULT_INSTANCE_URL")?,
                    verify_app_base_url: optional_env("GATEWAY_FRONTDOOR_VERIFY_APP_BASE_URL")?,
                    session_ttl_secs: optional_env("GATEWAY_FRONTDOOR_SESSION_TTL_SECS")?
                        .map(|s| s.parse())
                        .transpose()
                        .map_err(|e| ConfigError::InvalidValue {
                            key: "GATEWAY_FRONTDOOR_SESSION_TTL_SECS".to_string(),
                            message: format!("must be a valid integer: {e}"),
                        })?
                        .unwrap_or(900),
                    poll_interval_ms: optional_env("GATEWAY_FRONTDOOR_POLL_INTERVAL_MS")?
                        .map(|s| s.parse())
                        .transpose()
                        .map_err(|e| ConfigError::InvalidValue {
                            key: "GATEWAY_FRONTDOOR_POLL_INTERVAL_MS".to_string(),
                            message: format!("must be a valid integer: {e}"),
                        })?
                        .unwrap_or(1500),
                })
            } else {
                None
            };

            Some(GatewayConfig {
                host: optional_env("GATEWAY_HOST")?.unwrap_or_else(|| "127.0.0.1".to_string()),
                port: optional_env("GATEWAY_PORT")?
                    .map(|s| s.parse())
                    .transpose()
                    .map_err(|e| ConfigError::InvalidValue {
                        key: "GATEWAY_PORT".to_string(),
                        message: format!("must be a valid port number: {e}"),
                    })?
                    .unwrap_or(3000),
                auth_token: optional_env("GATEWAY_AUTH_TOKEN")?,
                user_id: optional_env("GATEWAY_USER_ID")?.unwrap_or_else(|| "default".to_string()),
                frontdoor,
            })
        } else {
            None
        };

        let cli_enabled = optional_env("CLI_ENABLED")?
            .map(|s| s.to_lowercase() != "false" && s != "0")
            .unwrap_or(true);

        Ok(Self {
            cli: CliConfig {
                enabled: cli_enabled,
            },
            http,
            gateway,
            wasm_channels_dir: optional_env("WASM_CHANNELS_DIR")?
                .map(PathBuf::from)
                .unwrap_or_else(default_channels_dir),
            wasm_channels_enabled: optional_env("WASM_CHANNELS_ENABLED")?
                .map(|s| s.parse())
                .transpose()
                .map_err(|e| ConfigError::InvalidValue {
                    key: "WASM_CHANNELS_ENABLED".to_string(),
                    message: format!("must be 'true' or 'false': {e}"),
                })?
                .unwrap_or(true),
            telegram_owner_id: optional_env("TELEGRAM_OWNER_ID")?
                .map(|s| s.parse())
                .transpose()
                .map_err(|e| ConfigError::InvalidValue {
                    key: "TELEGRAM_OWNER_ID".to_string(),
                    message: format!("must be an integer: {e}"),
                })?
                .or(settings.channels.telegram_owner_id),
        })
    }
}

/// Get the default channels directory (~/.enclagent/channels/).
fn default_channels_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".enclagent")
        .join("channels")
}
