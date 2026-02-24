//! User settings persistence.
//!
//! Stores user preferences in ~/.enclagent/settings.json.
//! Settings are loaded with env var > settings.json > default priority.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Supported LLM backends for settings/config wiring.
///
/// `tinfoil` is runtime-supported but not offered by the onboarding wizard.
pub const SUPPORTED_LLM_BACKENDS: [&str; 6] = [
    "nearai",
    "anthropic",
    "openai",
    "ollama",
    "openai_compatible",
    "tinfoil",
];

/// Normalize backend aliases to canonical values used across config/runtime.
pub fn normalize_llm_backend(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");

    match normalized.as_str() {
        "nearai" | "near_ai" => Some("nearai".to_string()),
        "anthropic" => Some("anthropic".to_string()),
        "openai" => Some("openai".to_string()),
        "ollama" => Some("ollama".to_string()),
        "openai_compatible" | "openaicompat" | "openai_compat" => {
            Some("openai_compatible".to_string())
        }
        "tinfoil" => Some("tinfoil".to_string()),
        _ => None,
    }
}

/// User settings persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// Whether onboarding wizard has been completed.
    #[serde(default, alias = "setup_completed")]
    pub onboard_completed: bool,

    // === Step 1: Database ===
    /// Database backend: "postgres" or "libsql".
    #[serde(default)]
    pub database_backend: Option<String>,

    /// Database connection URL (postgres://...).
    #[serde(default)]
    pub database_url: Option<String>,

    /// Database pool size.
    #[serde(default)]
    pub database_pool_size: Option<usize>,

    /// Path to local libSQL database file.
    #[serde(default)]
    pub libsql_path: Option<String>,

    /// Turso cloud URL for remote replica sync.
    #[serde(default)]
    pub libsql_url: Option<String>,

    // === Step 2: Security ===
    /// Source for the secrets master key.
    #[serde(default)]
    pub secrets_master_key_source: KeySource,

    // === Step 3: Inference Provider ===
    /// LLM backend: "nearai", "anthropic", "openai", "ollama", "openai_compatible".
    #[serde(default, alias = "provider", alias = "llm_provider")]
    pub llm_backend: Option<String>,

    /// Ollama base URL (when llm_backend = "ollama").
    #[serde(default)]
    pub ollama_base_url: Option<String>,

    /// OpenAI-compatible endpoint base URL (when llm_backend = "openai_compatible").
    #[serde(default, alias = "llm_base_url")]
    pub openai_compatible_base_url: Option<String>,

    // === Step 4: Model Selection ===
    /// Currently selected model.
    #[serde(default)]
    pub selected_model: Option<String>,

    // === Step 5: Embeddings ===
    /// Embeddings configuration.
    #[serde(default)]
    pub embeddings: EmbeddingsSettings,

    // === Step 6: Channels ===
    /// Tunnel configuration for public webhook endpoints.
    #[serde(default)]
    pub tunnel: TunnelSettings,

    /// Channel configuration.
    #[serde(default)]
    pub channels: ChannelSettings,

    // === Step 7: Heartbeat ===
    /// Heartbeat configuration.
    #[serde(default)]
    pub heartbeat: HeartbeatSettings,

    // === Step 8: Hyperliquid Runtime Profile ===
    /// Hyperliquid runtime profile.
    #[serde(default)]
    pub hyperliquid_runtime: HyperliquidRuntimeSettings,

    // === Step 9: Wallet and Vault Policy ===
    /// Wallet and vault policy settings.
    #[serde(default)]
    pub wallet_vault_policy: WalletVaultPolicySettings,

    // === Step 10: Verification Backend ===
    /// Verification backend settings with signed fallback policy.
    #[serde(default)]
    pub verification_backend: VerificationBackendSettings,

    // === Step 11: Intent Runtime ===
    /// Deterministic intent/execution/verification artifact settings.
    #[serde(default)]
    pub intent_runtime: IntentRuntimeSettings,

    // === Step 12: Copytrading Profile ===
    /// Fixed copytrading initialization profile and information-sharing policy.
    #[serde(default)]
    pub copytrading: CopyTradingSettings,

    // === Advanced Settings (not asked during setup, editable via CLI) ===
    /// Agent behavior configuration.
    #[serde(default)]
    pub agent: AgentSettings,

    /// WASM sandbox configuration.
    #[serde(default)]
    pub wasm: WasmSettings,

    /// Docker sandbox configuration.
    #[serde(default)]
    pub sandbox: SandboxSettings,

    /// Safety configuration.
    #[serde(default)]
    pub safety: SafetySettings,

    /// Builder configuration.
    #[serde(default)]
    pub builder: BuilderSettings,
}

/// Source for the secrets master key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    /// Auto-generated key stored in OS keychain.
    Keychain,
    /// User provides via SECRETS_MASTER_KEY env var.
    Env,
    /// Not configured (secrets features disabled).
    #[default]
    None,
}

/// Embeddings configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsSettings {
    /// Whether embeddings are enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Provider to use: "openai" or "nearai".
    #[serde(default = "default_embeddings_provider")]
    pub provider: String,

    /// Model to use for embeddings.
    #[serde(default = "default_embeddings_model")]
    pub model: String,
}

fn default_embeddings_provider() -> String {
    "nearai".to_string()
}

fn default_embeddings_model() -> String {
    "text-embedding-3-small".to_string()
}

impl Default for EmbeddingsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_embeddings_provider(),
            model: default_embeddings_model(),
        }
    }
}

/// Tunnel settings for public webhook endpoints.
///
/// The tunnel URL is shared across all channels that need webhooks.
/// Two modes:
/// - **Static URL**: `public_url` set directly (manual tunnel management).
/// - **Managed provider**: `provider` is set and the agent starts/stops the
///   tunnel process automatically at boot/shutdown.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TunnelSettings {
    /// Public URL from tunnel provider (e.g., "https://abc123.ngrok.io").
    /// When set without a provider, treated as a static (externally managed) URL.
    #[serde(default)]
    pub public_url: Option<String>,

    /// Managed tunnel provider: "ngrok", "cloudflare", "tailscale", "custom".
    #[serde(default)]
    pub provider: Option<String>,

    /// Cloudflare tunnel token.
    #[serde(default)]
    pub cf_token: Option<String>,

    /// ngrok auth token.
    #[serde(default)]
    pub ngrok_token: Option<String>,

    /// ngrok custom domain (paid plans).
    #[serde(default)]
    pub ngrok_domain: Option<String>,

    /// Use Tailscale Funnel (public) instead of Serve (tailnet-only).
    #[serde(default)]
    pub ts_funnel: bool,

    /// Tailscale hostname override.
    #[serde(default)]
    pub ts_hostname: Option<String>,

    /// Shell command for custom tunnel (with `{port}` / `{host}` placeholders).
    #[serde(default)]
    pub custom_command: Option<String>,

    /// Health check URL for custom tunnel.
    #[serde(default)]
    pub custom_health_url: Option<String>,

    /// Substring pattern to extract URL from custom tunnel stdout.
    #[serde(default)]
    pub custom_url_pattern: Option<String>,
}

/// Channel-specific settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelSettings {
    /// Whether HTTP webhook channel is enabled.
    #[serde(default)]
    pub http_enabled: bool,

    /// HTTP webhook port (if enabled).
    #[serde(default)]
    pub http_port: Option<u16>,

    /// HTTP webhook host.
    #[serde(default)]
    pub http_host: Option<String>,

    /// Telegram owner user ID. When set, the bot only responds to this user.
    /// Captured during setup by having the user message the bot.
    #[serde(default)]
    pub telegram_owner_id: Option<i64>,

    /// Enabled WASM channels by name.
    /// Channels not in this list but present in the channels directory will still load.
    /// This is primarily used by the setup wizard to track which channels were configured.
    #[serde(default)]
    pub wasm_channels: Vec<String>,

    /// Whether WASM channels are enabled.
    #[serde(default = "default_true")]
    pub wasm_channels_enabled: bool,

    /// Directory containing WASM channel modules.
    #[serde(default)]
    pub wasm_channels_dir: Option<PathBuf>,
}

/// Heartbeat configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatSettings {
    /// Whether heartbeat is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Interval between heartbeat checks in seconds.
    #[serde(default = "default_heartbeat_interval")]
    pub interval_secs: u64,

    /// Channel to notify on heartbeat findings.
    #[serde(default)]
    pub notify_channel: Option<String>,

    /// User ID to notify on heartbeat findings.
    #[serde(default)]
    pub notify_user: Option<String>,
}

fn default_heartbeat_interval() -> u64 {
    1800 // 30 minutes
}

impl Default for HeartbeatSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: default_heartbeat_interval(),
            notify_channel: None,
            notify_user: None,
        }
    }
}

/// Hyperliquid runtime profile settings (Wizard Step 8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidRuntimeSettings {
    /// Hyperliquid network: "testnet" or "mainnet".
    #[serde(default = "default_hyperliquid_network")]
    pub network: String,

    /// Hyperliquid API base URL override.
    #[serde(default)]
    pub api_base_url: Option<String>,

    /// Hyperliquid WebSocket URL override.
    #[serde(default)]
    pub ws_url: Option<String>,

    /// Request timeout in milliseconds.
    #[serde(default = "default_hyperliquid_timeout_ms")]
    pub timeout_ms: u64,

    /// Maximum retry attempts for recoverable failures.
    #[serde(default = "default_hyperliquid_max_retries")]
    pub max_retries: u32,

    /// Backoff delay between retries in milliseconds.
    #[serde(default = "default_hyperliquid_retry_backoff_ms")]
    pub retry_backoff_ms: u64,

    /// Runtime policy: "paper_only", "paper_first", or "live_allowed".
    #[serde(default = "default_hyperliquid_paper_live_policy")]
    pub paper_live_policy: String,
}

fn default_hyperliquid_network() -> String {
    "testnet".to_string()
}

fn default_hyperliquid_timeout_ms() -> u64 {
    10_000
}

fn default_hyperliquid_max_retries() -> u32 {
    3
}

fn default_hyperliquid_retry_backoff_ms() -> u64 {
    500
}

fn default_hyperliquid_paper_live_policy() -> String {
    "paper_first".to_string()
}

const TRADING_ENDPOINT_HOST_ALLOWLIST: &[&str] =
    &["api.hyperliquid.xyz", "api.hyperliquid-testnet.xyz"];
const VERIFICATION_ENDPOINT_HOST_ALLOWLIST: &[&str] = &[
    "verify.eigencloud.xyz",
    "verify.eigencloud.example",
    "localhost",
    "127.0.0.1",
];

impl Default for HyperliquidRuntimeSettings {
    fn default() -> Self {
        Self {
            network: default_hyperliquid_network(),
            api_base_url: None,
            ws_url: None,
            timeout_ms: default_hyperliquid_timeout_ms(),
            max_retries: default_hyperliquid_max_retries(),
            retry_backoff_ms: default_hyperliquid_retry_backoff_ms(),
            paper_live_policy: default_hyperliquid_paper_live_policy(),
        }
    }
}

/// Wallet and vault policy settings (Wizard Step 9).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletVaultPolicySettings {
    /// Custody mode: "operator_wallet", "user_wallet", or "dual_mode".
    #[serde(default = "default_wallet_custody_mode")]
    pub custody_mode: String,

    /// Operator wallet address.
    #[serde(default)]
    pub operator_wallet_address: Option<String>,

    /// User wallet address.
    #[serde(default)]
    pub user_wallet_address: Option<String>,

    /// Vault address.
    #[serde(default)]
    pub vault_address: Option<String>,

    /// Maximum allowed position size in USD units.
    #[serde(default = "default_max_position_size_usd")]
    pub max_position_size_usd: u64,

    /// Maximum allowed leverage.
    #[serde(default = "default_leverage_cap")]
    pub leverage_cap: u32,

    /// Whether kill switch protections are enabled.
    #[serde(default = "default_true")]
    pub kill_switch_enabled: bool,

    /// Kill switch action policy: "pause_agent", "cancel_open_orders", "cancel_and_flatten".
    #[serde(default = "default_kill_switch_behavior")]
    pub kill_switch_behavior: String,
}

fn default_wallet_custody_mode() -> String {
    "operator_wallet".to_string()
}

fn default_max_position_size_usd() -> u64 {
    1_000
}

fn default_leverage_cap() -> u32 {
    2
}

fn default_kill_switch_behavior() -> String {
    "pause_agent".to_string()
}

impl Default for WalletVaultPolicySettings {
    fn default() -> Self {
        Self {
            custody_mode: default_wallet_custody_mode(),
            operator_wallet_address: None,
            user_wallet_address: None,
            vault_address: None,
            max_position_size_usd: default_max_position_size_usd(),
            leverage_cap: default_leverage_cap(),
            kill_switch_enabled: true,
            kill_switch_behavior: default_kill_switch_behavior(),
        }
    }
}

/// Verification backend settings (Wizard Step 10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationBackendSettings {
    /// Verification backend policy: "eigencloud_primary" or "fallback_only".
    #[serde(default = "default_verification_backend")]
    pub backend: String,

    /// EigenCloud endpoint URL.
    #[serde(default)]
    pub eigencloud_endpoint: Option<String>,

    /// EigenCloud auth scheme: "bearer" or "api_key".
    #[serde(default = "default_eigencloud_auth_scheme")]
    pub eigencloud_auth_scheme: String,

    /// EigenCloud auth token.
    #[serde(default)]
    pub eigencloud_auth_token: Option<String>,

    /// EigenCloud request timeout in milliseconds.
    #[serde(default = "default_verification_timeout_ms")]
    pub eigencloud_timeout_ms: u64,

    /// Enable signed local fallback receipt-chain.
    #[serde(default = "default_true")]
    pub fallback_enabled: bool,

    /// Signing key identifier for fallback receipts.
    #[serde(default)]
    pub fallback_signing_key_id: Option<String>,

    /// Storage path for fallback receipt-chain records.
    #[serde(default)]
    pub fallback_chain_path: Option<PathBuf>,

    /// Require signed fallback receipts.
    #[serde(default = "default_true")]
    pub fallback_require_signed_receipts: bool,
}

fn default_verification_backend() -> String {
    "eigencloud_primary".to_string()
}

fn default_eigencloud_auth_scheme() -> String {
    "bearer".to_string()
}

fn default_verification_timeout_ms() -> u64 {
    5_000
}

impl Default for VerificationBackendSettings {
    fn default() -> Self {
        Self {
            backend: default_verification_backend(),
            eigencloud_endpoint: None,
            eigencloud_auth_scheme: default_eigencloud_auth_scheme(),
            eigencloud_auth_token: None,
            eigencloud_timeout_ms: default_verification_timeout_ms(),
            fallback_enabled: true,
            fallback_signing_key_id: None,
            fallback_chain_path: None,
            fallback_require_signed_receipts: true,
        }
    }
}

/// Intent runtime settings for WS-3 typed artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentRuntimeSettings {
    /// Contract/schema version used by runtime artifacts.
    #[serde(default = "default_intent_contract_version")]
    pub contract_version: String,

    /// Hash algorithm used for artifact fingerprinting.
    #[serde(default = "default_intent_hash_algorithm")]
    pub hash_algorithm: String,

    /// Whether deterministic serialization is required before hashing.
    #[serde(default = "default_true")]
    pub deterministic_serialization: bool,

    /// Default execution mode for intents: "paper" or "live".
    #[serde(default = "default_intent_default_execution_mode")]
    pub default_execution_mode: String,

    /// Enforce runtime validation on typed artifacts before persistence/streaming.
    #[serde(default = "default_true")]
    pub enforce_artifact_validation: bool,

    /// Reject nil UUID identifiers in intent/execution/verification artifacts.
    #[serde(default = "default_true")]
    pub reject_nil_identifiers: bool,
}

fn default_intent_contract_version() -> String {
    "v1".to_string()
}

fn default_intent_hash_algorithm() -> String {
    "blake3".to_string()
}

fn default_intent_default_execution_mode() -> String {
    "paper".to_string()
}

impl Default for IntentRuntimeSettings {
    fn default() -> Self {
        Self {
            contract_version: default_intent_contract_version(),
            hash_algorithm: default_intent_hash_algorithm(),
            deterministic_serialization: true,
            default_execution_mode: default_intent_default_execution_mode(),
            enforce_artifact_validation: true,
            reject_nil_identifiers: true,
        }
    }
}

/// Copytrading initialization profile (WS-10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyTradingSettings {
    /// Maximum total allocation for mirrored positions in USD terms.
    #[serde(default = "default_copy_max_allocation_usd")]
    pub max_allocation_usd: u64,

    /// Per-trade cap in USD terms.
    #[serde(default = "default_copy_per_trade_notional_cap_usd")]
    pub per_trade_notional_cap_usd: u64,

    /// Maximum leverage multiplier.
    #[serde(default = "default_copy_max_leverage")]
    pub max_leverage: u32,

    /// Explicit allowlist of symbols to mirror.
    #[serde(default = "default_copy_symbol_allowlist")]
    pub symbol_allowlist: Vec<String>,

    /// Explicit denylist of symbols to block.
    #[serde(default)]
    pub symbol_denylist: Vec<String>,

    /// Maximum slippage in basis points.
    #[serde(default = "default_copy_max_slippage_bps")]
    pub max_slippage_bps: u32,

    /// Information-sharing scope: "none", "signals_only", "signals_and_execution", "full_audit".
    #[serde(default = "default_copy_information_sharing_scope")]
    pub information_sharing_scope: String,
}

fn default_copy_max_allocation_usd() -> u64 {
    5_000
}

fn default_copy_per_trade_notional_cap_usd() -> u64 {
    750
}

fn default_copy_max_leverage() -> u32 {
    2
}

fn default_copy_symbol_allowlist() -> Vec<String> {
    vec!["BTC-USD".to_string(), "ETH-USD".to_string()]
}

fn default_copy_max_slippage_bps() -> u32 {
    25
}

fn default_copy_information_sharing_scope() -> String {
    "signals_only".to_string()
}

impl Default for CopyTradingSettings {
    fn default() -> Self {
        Self {
            max_allocation_usd: default_copy_max_allocation_usd(),
            per_trade_notional_cap_usd: default_copy_per_trade_notional_cap_usd(),
            max_leverage: default_copy_max_leverage(),
            symbol_allowlist: default_copy_symbol_allowlist(),
            symbol_denylist: Vec::new(),
            max_slippage_bps: default_copy_max_slippage_bps(),
            information_sharing_scope: default_copy_information_sharing_scope(),
        }
    }
}

/// Agent behavior configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    /// Agent name.
    #[serde(default = "default_agent_name")]
    pub name: String,

    /// Maximum parallel jobs.
    #[serde(default = "default_max_parallel_jobs")]
    pub max_parallel_jobs: u32,

    /// Job timeout in seconds.
    #[serde(default = "default_job_timeout")]
    pub job_timeout_secs: u64,

    /// Stuck job threshold in seconds.
    #[serde(default = "default_stuck_threshold")]
    pub stuck_threshold_secs: u64,

    /// Whether to use planning before tool execution.
    #[serde(default = "default_true")]
    pub use_planning: bool,

    /// Self-repair check interval in seconds.
    #[serde(default = "default_repair_interval")]
    pub repair_check_interval_secs: u64,

    /// Maximum repair attempts.
    #[serde(default = "default_max_repair_attempts")]
    pub max_repair_attempts: u32,

    /// Session idle timeout in seconds (default: 7 days). Sessions inactive
    /// longer than this are pruned from memory.
    #[serde(default = "default_session_idle_timeout")]
    pub session_idle_timeout_secs: u64,
}

fn default_agent_name() -> String {
    "enclagent".to_string()
}

fn default_max_parallel_jobs() -> u32 {
    5
}

fn default_job_timeout() -> u64 {
    3600 // 1 hour
}

fn default_stuck_threshold() -> u64 {
    300 // 5 minutes
}

fn default_repair_interval() -> u64 {
    60 // 1 minute
}

fn default_session_idle_timeout() -> u64 {
    7 * 24 * 3600 // 7 days
}

fn default_max_repair_attempts() -> u32 {
    3
}

fn default_true() -> bool {
    true
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            name: default_agent_name(),
            max_parallel_jobs: default_max_parallel_jobs(),
            job_timeout_secs: default_job_timeout(),
            stuck_threshold_secs: default_stuck_threshold(),
            use_planning: true,
            repair_check_interval_secs: default_repair_interval(),
            max_repair_attempts: default_max_repair_attempts(),
            session_idle_timeout_secs: default_session_idle_timeout(),
        }
    }
}

/// WASM sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmSettings {
    /// Whether WASM tool execution is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Directory containing installed WASM tools.
    #[serde(default)]
    pub tools_dir: Option<PathBuf>,

    /// Default memory limit in bytes.
    #[serde(default = "default_wasm_memory_limit")]
    pub default_memory_limit: u64,

    /// Default execution timeout in seconds.
    #[serde(default = "default_wasm_timeout")]
    pub default_timeout_secs: u64,

    /// Default fuel limit for CPU metering.
    #[serde(default = "default_wasm_fuel_limit")]
    pub default_fuel_limit: u64,

    /// Whether to cache compiled modules.
    #[serde(default = "default_true")]
    pub cache_compiled: bool,

    /// Directory for compiled module cache.
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
}

fn default_wasm_memory_limit() -> u64 {
    10 * 1024 * 1024 // 10 MB
}

fn default_wasm_timeout() -> u64 {
    60
}

fn default_wasm_fuel_limit() -> u64 {
    10_000_000
}

impl Default for WasmSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            tools_dir: None,
            default_memory_limit: default_wasm_memory_limit(),
            default_timeout_secs: default_wasm_timeout(),
            default_fuel_limit: default_wasm_fuel_limit(),
            cache_compiled: true,
            cache_dir: None,
        }
    }
}

/// Docker sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSettings {
    /// Whether the Docker sandbox is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Sandbox policy: "readonly", "workspace_write", or "full_access".
    #[serde(default = "default_sandbox_policy")]
    pub policy: String,

    /// Command timeout in seconds.
    #[serde(default = "default_sandbox_timeout")]
    pub timeout_secs: u64,

    /// Memory limit in megabytes.
    #[serde(default = "default_sandbox_memory")]
    pub memory_limit_mb: u64,

    /// CPU shares (relative weight).
    #[serde(default = "default_sandbox_cpu_shares")]
    pub cpu_shares: u32,

    /// Docker image for the sandbox.
    #[serde(default = "default_sandbox_image")]
    pub image: String,

    /// Whether to auto-pull the image if not found.
    #[serde(default = "default_true")]
    pub auto_pull_image: bool,

    /// Additional domains to allow through the network proxy.
    #[serde(default)]
    pub extra_allowed_domains: Vec<String>,
}

fn default_sandbox_policy() -> String {
    "readonly".to_string()
}

fn default_sandbox_timeout() -> u64 {
    120
}

fn default_sandbox_memory() -> u64 {
    2048
}

fn default_sandbox_cpu_shares() -> u32 {
    1024
}

fn default_sandbox_image() -> String {
    "ghcr.io/nearai/sandbox:latest".to_string()
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            policy: default_sandbox_policy(),
            timeout_secs: default_sandbox_timeout(),
            memory_limit_mb: default_sandbox_memory(),
            cpu_shares: default_sandbox_cpu_shares(),
            image: default_sandbox_image(),
            auto_pull_image: true,
            extra_allowed_domains: Vec::new(),
        }
    }
}

/// Safety configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetySettings {
    /// Maximum output length in bytes.
    #[serde(default = "default_max_output_length")]
    pub max_output_length: usize,

    /// Whether injection check is enabled.
    #[serde(default = "default_true")]
    pub injection_check_enabled: bool,
}

fn default_max_output_length() -> usize {
    100_000
}

impl Default for SafetySettings {
    fn default() -> Self {
        Self {
            max_output_length: default_max_output_length(),
            injection_check_enabled: true,
        }
    }
}

/// Builder configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderSettings {
    /// Whether the software builder tool is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Directory for build artifacts.
    #[serde(default)]
    pub build_dir: Option<PathBuf>,

    /// Maximum iterations for the build loop.
    #[serde(default = "default_builder_max_iterations")]
    pub max_iterations: u32,

    /// Build timeout in seconds.
    #[serde(default = "default_builder_timeout")]
    pub timeout_secs: u64,

    /// Whether to automatically register built WASM tools.
    #[serde(default = "default_true")]
    pub auto_register: bool,
}

fn default_builder_max_iterations() -> u32 {
    20
}

fn default_builder_timeout() -> u64 {
    600
}

impl Default for BuilderSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            build_dir: None,
            max_iterations: default_builder_max_iterations(),
            timeout_secs: default_builder_timeout(),
            auto_register: true,
        }
    }
}

fn normalize_paper_live_policy(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "paper" | "paper_only" => Some("paper_only"),
        "paper_first" => Some("paper_first"),
        "live" | "live_allowed" => Some("live_allowed"),
        _ => None,
    }
}

fn endpoint_host_matches(host: &str, allowlist_entry: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    let entry = allowlist_entry.trim().to_ascii_lowercase();
    if host.is_empty() || entry.is_empty() {
        return false;
    }

    if let Some(suffix) = entry.strip_prefix("*.") {
        if suffix.is_empty() || host == suffix {
            return false;
        }
        host.ends_with(&format!(".{suffix}"))
    } else {
        host == entry
    }
}

fn endpoint_host_allowlisted(host: &str, host_allowlist: &[&str]) -> bool {
    host_allowlist
        .iter()
        .any(|entry| endpoint_host_matches(host, entry))
}

fn validate_allowlisted_endpoint(
    endpoint_url: &str,
    host_allowlist: &[&str],
    scheme_allowlist: &[&str],
) -> Result<(), String> {
    let parsed = url::Url::parse(endpoint_url).map_err(|e| format!("URL parse failed: {}", e))?;

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL contains userinfo (@), which is not allowed".to_string());
    }

    let scheme = parsed.scheme().to_ascii_lowercase();
    let scheme_allowed = scheme_allowlist
        .iter()
        .any(|allowed| scheme.eq_ignore_ascii_case(allowed));
    if !scheme_allowed {
        return Err(format!(
            "scheme '{}' is not allowed (allowed: {})",
            scheme,
            scheme_allowlist.join(", ")
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL is missing host".to_string())?
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();

    if !endpoint_host_allowlisted(&host, host_allowlist) {
        return Err(format!(
            "host '{}' is not allowlisted (allowed: {})",
            host,
            host_allowlist.join(", ")
        ));
    }

    Ok(())
}

fn decode_optional_setting_string(raw: &str) -> Result<Option<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return Ok(None);
    }

    if trimmed.starts_with('"') {
        let parsed: String = serde_json::from_str(trimmed)
            .map_err(|e| format!("invalid JSON string value '{}': {}", raw, e))?;
        let parsed = parsed.trim().to_string();
        if parsed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(parsed))
        }
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

impl Settings {
    fn sanitize_llm_backend(&mut self) {
        let Some(raw) = self.llm_backend.clone() else {
            return;
        };

        match normalize_llm_backend(&raw) {
            Some(normalized) => {
                self.llm_backend = Some(normalized);
            }
            None => {
                tracing::warn!(
                    "Invalid llm_backend '{}' in settings. Clearing value to enforce explicit provider selection.",
                    raw
                );
                self.llm_backend = None;
            }
        }
    }

    /// Reconstruct Settings from a flat key-value map (as stored in the DB).
    ///
    /// Each key is a dotted path (e.g., "agent.name"), value is a JSONB value.
    /// Missing keys get their default value.
    pub fn from_db_map(map: &std::collections::HashMap<String, serde_json::Value>) -> Self {
        // Start with defaults, then overlay each DB setting.
        //
        // The settings table stores both Settings struct fields and app-specific
        // data (e.g. nearai.session_token). Skip keys that don't correspond to
        // a known Settings path.
        let mut settings = Self::default();

        for (key, value) in map {
            // Convert the JSONB value to a string for the existing set() method
            let value_str = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Null => continue, // null means default, skip
                other => other.to_string(),
            };

            match settings.set(key, &value_str) {
                Ok(()) => {}
                // The settings table stores both Settings fields and app-specific
                // data (e.g. nearai.session_token). Silently skip unknown paths.
                Err(e) if e.starts_with("Path not found") => {}
                Err(e) => {
                    tracing::warn!(
                        "Failed to apply DB setting '{}' = '{}': {}",
                        key,
                        value_str,
                        e
                    );
                }
            }
        }

        settings.sanitize_llm_backend();
        settings
    }

    /// Flatten Settings into a key-value map suitable for DB storage.
    ///
    /// Each entry is a (dotted_path, JSONB value) pair.
    pub fn to_db_map(&self) -> std::collections::HashMap<String, serde_json::Value> {
        let json = match serde_json::to_value(self) {
            Ok(v) => v,
            Err(_) => return std::collections::HashMap::new(),
        };

        let mut map = std::collections::HashMap::new();
        collect_settings_json(&json, String::new(), &mut map);
        map
    }

    /// Get the default settings file path (~/.enclagent/settings.json).
    pub fn default_path() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".enclagent")
            .join("settings.json")
    }

    /// Load settings from disk, returning default if not found.
    pub fn load() -> Self {
        Self::load_from(&Self::default_path())
    }

    /// Load settings from a specific path (used by bootstrap legacy migration).
    pub fn load_from(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(data) => {
                let mut settings: Self = serde_json::from_str(&data).unwrap_or_default();
                settings.sanitize_llm_backend();
                settings
            }
            Err(_) => Self::default(),
        }
    }

    /// Default TOML config file path (~/.enclagent/config.toml).
    pub fn default_toml_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".enclagent")
            .join("config.toml")
    }

    /// Load settings from a TOML file.
    ///
    /// Returns `None` if the file doesn't exist. Returns an error only
    /// if the file exists but can't be parsed.
    pub fn load_toml(path: &std::path::Path) -> Result<Option<Self>, String> {
        let data = match std::fs::read_to_string(path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("failed to read {}: {}", path.display(), e)),
        };

        let mut settings: Self = toml::from_str(&data)
            .map_err(|e| format!("invalid TOML in {}: {}", path.display(), e))?;
        settings.sanitize_llm_backend();
        Ok(Some(settings))
    }

    /// Write a well-commented TOML config file with current settings.
    pub fn save_toml(&self, path: &std::path::Path) -> Result<(), String> {
        let raw = toml::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize settings: {}", e))?;

        let content = format!(
            "# Enclagent configuration file.\n\
             #\n\
             # Priority: env var > this file > database settings > defaults.\n\
             # Uncomment and edit values to override defaults.\n\
             # Run `enclagent config init` to regenerate this file.\n\
             #\n\
             # Documentation: https://github.com/halo-labs-xyz/Enclagent\n\
             \n\
             {raw}"
        );

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {}", parent.display(), e))?;
        }

        std::fs::write(path, content)
            .map_err(|e| format!("failed to write {}: {}", path.display(), e))
    }

    /// Merge values from `other` into `self`, preferring `other` for
    /// fields that differ from the default.
    ///
    /// This enables layering: load DB/JSON settings as the base, then
    /// overlay TOML values on top. Only fields that the TOML file
    /// explicitly changed (i.e. differ from Default) are applied.
    pub fn merge_from(&mut self, other: &Self) {
        let default_json = match serde_json::to_value(Self::default()) {
            Ok(v) => v,
            Err(_) => return,
        };
        let other_json = match serde_json::to_value(other) {
            Ok(v) => v,
            Err(_) => return,
        };
        let mut self_json = match serde_json::to_value(&*self) {
            Ok(v) => v,
            Err(_) => return,
        };

        merge_non_default(&mut self_json, &other_json, &default_json);

        if let Ok(merged) = serde_json::from_value(self_json) {
            *self = merged;
            self.sanitize_llm_backend();
        }
    }

    /// Get a setting value by dotted path (e.g., "agent.max_parallel_jobs").
    pub fn get(&self, path: &str) -> Option<String> {
        let json = serde_json::to_value(self).ok()?;
        let mut current = &json;

        for part in path.split('.') {
            current = current.get(part)?;
        }

        match current {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            serde_json::Value::Null => Some("null".to_string()),
            serde_json::Value::Array(arr) => Some(serde_json::to_string(arr).unwrap_or_default()),
            serde_json::Value::Object(obj) => Some(serde_json::to_string(obj).unwrap_or_default()),
        }
    }

    /// Set a setting value by dotted path.
    ///
    /// Returns error if path is invalid or value cannot be parsed.
    pub fn set(&mut self, path: &str, value: &str) -> Result<(), String> {
        if path == "hyperliquid_runtime.paper_live_policy" {
            let normalized = normalize_paper_live_policy(value).ok_or_else(|| {
                format!(
                    "Invalid hyperliquid_runtime.paper_live_policy '{}'. Expected one of: paper_only, paper_first, live_allowed",
                    value
                )
            })?;
            self.hyperliquid_runtime.paper_live_policy = normalized.to_string();
            return Ok(());
        }

        if path == "hyperliquid_runtime.api_base_url" {
            let endpoint = decode_optional_setting_string(value)?;
            if let Some(ref endpoint) = endpoint {
                validate_allowlisted_endpoint(
                    endpoint,
                    TRADING_ENDPOINT_HOST_ALLOWLIST,
                    &["https"],
                )
                .map_err(|e| format!("Invalid hyperliquid_runtime.api_base_url: {}", e))?;
            }
            self.hyperliquid_runtime.api_base_url = endpoint;
            return Ok(());
        }

        if path == "hyperliquid_runtime.ws_url" {
            let endpoint = decode_optional_setting_string(value)?;
            if let Some(ref endpoint) = endpoint {
                validate_allowlisted_endpoint(endpoint, TRADING_ENDPOINT_HOST_ALLOWLIST, &["wss"])
                    .map_err(|e| format!("Invalid hyperliquid_runtime.ws_url: {}", e))?;
            }
            self.hyperliquid_runtime.ws_url = endpoint;
            return Ok(());
        }

        if path == "verification_backend.eigencloud_endpoint" {
            let endpoint = decode_optional_setting_string(value)?;
            if let Some(ref endpoint) = endpoint {
                validate_allowlisted_endpoint(
                    endpoint,
                    VERIFICATION_ENDPOINT_HOST_ALLOWLIST,
                    &["https"],
                )
                .map_err(|e| format!("Invalid verification_backend.eigencloud_endpoint: {}", e))?;
            }
            self.verification_backend.eigencloud_endpoint = endpoint;
            return Ok(());
        }

        if path == "llm_backend" {
            let raw = value.trim();
            if raw.is_empty() || raw.eq_ignore_ascii_case("null") {
                self.llm_backend = None;
                return Ok(());
            }

            let normalized = normalize_llm_backend(raw).ok_or_else(|| {
                format!(
                    "Invalid llm_backend '{}'. Expected one of: {}",
                    value,
                    SUPPORTED_LLM_BACKENDS.join(", ")
                )
            })?;
            self.llm_backend = Some(normalized);
            return Ok(());
        }

        let mut json = serde_json::to_value(&self)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;

        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return Err("Empty path".to_string());
        }

        // Navigate to parent and set the final key
        let mut current = &mut json;
        for part in &parts[..parts.len() - 1] {
            current = current
                .get_mut(*part)
                .ok_or_else(|| format!("Path not found: {}", path))?;
        }

        let final_key = parts.last().unwrap();
        let obj = current
            .as_object_mut()
            .ok_or_else(|| format!("Parent is not an object: {}", path))?;

        // Try to infer the type from the existing value
        let new_value = if let Some(existing) = obj.get(*final_key) {
            match existing {
                serde_json::Value::Bool(_) => {
                    let b = value
                        .parse::<bool>()
                        .map_err(|_| format!("Expected boolean for {}, got '{}'", path, value))?;
                    serde_json::Value::Bool(b)
                }
                serde_json::Value::Number(n) => {
                    if n.is_u64() {
                        let n = value.parse::<u64>().map_err(|_| {
                            format!("Expected integer for {}, got '{}'", path, value)
                        })?;
                        serde_json::Value::Number(n.into())
                    } else if n.is_i64() {
                        let n = value.parse::<i64>().map_err(|_| {
                            format!("Expected integer for {}, got '{}'", path, value)
                        })?;
                        serde_json::Value::Number(n.into())
                    } else {
                        let n = value.parse::<f64>().map_err(|_| {
                            format!("Expected number for {}, got '{}'", path, value)
                        })?;
                        serde_json::Number::from_f64(n)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::String(value.to_string()))
                    }
                }
                serde_json::Value::Null => {
                    // Could be Option<T>, try to parse as JSON or use string
                    serde_json::from_str(value)
                        .unwrap_or(serde_json::Value::String(value.to_string()))
                }
                serde_json::Value::Array(_) => serde_json::from_str(value)
                    .map_err(|e| format!("Invalid JSON array for {}: {}", path, e))?,
                serde_json::Value::Object(_) => serde_json::from_str(value)
                    .map_err(|e| format!("Invalid JSON object for {}: {}", path, e))?,
                serde_json::Value::String(_) => serde_json::Value::String(value.to_string()),
            }
        } else {
            // Key doesn't exist, try to parse as JSON or use string
            serde_json::from_str(value).unwrap_or(serde_json::Value::String(value.to_string()))
        };

        obj.insert((*final_key).to_string(), new_value);

        // Deserialize back to Settings
        *self =
            serde_json::from_value(json).map_err(|e| format!("Failed to apply setting: {}", e))?;

        Ok(())
    }

    /// Reset a setting to its default value.
    pub fn reset(&mut self, path: &str) -> Result<(), String> {
        let default = Self::default();
        let default_value = default
            .get(path)
            .ok_or_else(|| format!("Unknown setting: {}", path))?;

        self.set(path, &default_value)
    }

    /// List all settings as (path, value) pairs.
    pub fn list(&self) -> Vec<(String, String)> {
        let json = match serde_json::to_value(self) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        let mut results = Vec::new();
        collect_settings(&json, String::new(), &mut results);
        results.sort_by(|a, b| a.0.cmp(&b.0));
        results
    }
}

/// Recursively collect settings paths with their JSON values (for DB storage).
fn collect_settings_json(
    value: &serde_json::Value,
    prefix: String,
    results: &mut std::collections::HashMap<String, serde_json::Value>,
) {
    match value {
        serde_json::Value::Object(obj) => {
            for (key, val) in obj {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                collect_settings_json(val, path, results);
            }
        }
        other => {
            results.insert(prefix, other.clone());
        }
    }
}

/// Recursively collect settings paths and values.
fn collect_settings(
    value: &serde_json::Value,
    prefix: String,
    results: &mut Vec<(String, String)>,
) {
    match value {
        serde_json::Value::Object(obj) => {
            for (key, val) in obj {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                collect_settings(val, path, results);
            }
        }
        serde_json::Value::Array(arr) => {
            let display = serde_json::to_string(arr).unwrap_or_default();
            results.push((prefix, display));
        }
        serde_json::Value::String(s) => {
            results.push((prefix, s.clone()));
        }
        serde_json::Value::Number(n) => {
            results.push((prefix, n.to_string()));
        }
        serde_json::Value::Bool(b) => {
            results.push((prefix, b.to_string()));
        }
        serde_json::Value::Null => {
            results.push((prefix, "null".to_string()));
        }
    }
}

/// Recursively merge `other` into `target`, but only for fields where
/// `other` differs from `defaults`. This means only explicitly-set values
/// in the TOML file override the base settings.
fn merge_non_default(
    target: &mut serde_json::Value,
    other: &serde_json::Value,
    defaults: &serde_json::Value,
) {
    match (target, other, defaults) {
        (
            serde_json::Value::Object(t),
            serde_json::Value::Object(o),
            serde_json::Value::Object(d),
        ) => {
            for (key, other_val) in o {
                let default_val = d.get(key).cloned().unwrap_or(serde_json::Value::Null);
                if let Some(target_val) = t.get_mut(key) {
                    merge_non_default(target_val, other_val, &default_val);
                } else if other_val != &default_val {
                    t.insert(key.clone(), other_val.clone());
                }
            }
        }
        (target, other, defaults) => {
            if other != defaults {
                *target = other.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::*;

    #[test]
    fn test_db_map_round_trip() {
        let settings = Settings {
            selected_model: Some("claude-3-5-sonnet-20241022".to_string()),
            ..Default::default()
        };

        let map = settings.to_db_map();
        let restored = Settings::from_db_map(&map);
        assert_eq!(
            restored.selected_model,
            Some("claude-3-5-sonnet-20241022".to_string())
        );
    }

    #[test]
    fn test_get_setting() {
        let settings = Settings::default();

        assert_eq!(settings.get("agent.name"), Some("enclagent".to_string()));
        assert_eq!(
            settings.get("agent.max_parallel_jobs"),
            Some("5".to_string())
        );
        assert_eq!(settings.get("heartbeat.enabled"), Some("false".to_string()));
        assert_eq!(settings.get("nonexistent"), None);
    }

    #[test]
    fn test_set_setting() {
        let mut settings = Settings::default();

        settings.set("agent.name", "mybot").unwrap();
        assert_eq!(settings.agent.name, "mybot");

        settings.set("agent.max_parallel_jobs", "10").unwrap();
        assert_eq!(settings.agent.max_parallel_jobs, 10);

        settings.set("heartbeat.enabled", "true").unwrap();
        assert!(settings.heartbeat.enabled);
    }

    #[test]
    fn test_reset_setting() {
        let mut settings = Settings::default();

        settings.agent.name = "custom".to_string();
        settings.reset("agent.name").unwrap();
        assert_eq!(settings.agent.name, "enclagent");
    }

    #[test]
    fn test_list_settings() {
        let settings = Settings::default();
        let list = settings.list();

        // Check some expected entries
        assert!(list.iter().any(|(k, _)| k == "agent.name"));
        assert!(list.iter().any(|(k, _)| k == "heartbeat.enabled"));
        assert!(list.iter().any(|(k, _)| k == "onboard_completed"));
    }

    #[test]
    fn test_key_source_serialization() {
        let settings = Settings {
            secrets_master_key_source: KeySource::Keychain,
            ..Default::default()
        };

        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"keychain\""));

        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.secrets_master_key_source, KeySource::Keychain);
    }

    #[test]
    fn test_embeddings_defaults() {
        let settings = Settings::default();
        assert!(!settings.embeddings.enabled);
        assert_eq!(settings.embeddings.provider, "nearai");
        assert_eq!(settings.embeddings.model, "text-embedding-3-small");
    }

    #[test]
    fn test_intent_runtime_defaults() {
        let settings = Settings::default();
        assert_eq!(settings.intent_runtime.contract_version, "v1");
        assert_eq!(settings.intent_runtime.hash_algorithm, "blake3");
        assert!(settings.intent_runtime.deterministic_serialization);
        assert_eq!(settings.intent_runtime.default_execution_mode, "paper");
        assert!(settings.intent_runtime.enforce_artifact_validation);
        assert!(settings.intent_runtime.reject_nil_identifiers);
    }

    #[test]
    fn test_intent_runtime_round_trip() {
        let settings = Settings {
            intent_runtime: IntentRuntimeSettings {
                contract_version: "v2".to_string(),
                hash_algorithm: "sha256".to_string(),
                deterministic_serialization: false,
                default_execution_mode: "live".to_string(),
                enforce_artifact_validation: false,
                reject_nil_identifiers: false,
            },
            ..Default::default()
        };

        let map = settings.to_db_map();
        let restored = Settings::from_db_map(&map);
        assert_eq!(restored.intent_runtime.contract_version, "v2");
        assert_eq!(restored.intent_runtime.hash_algorithm, "sha256");
        assert!(!restored.intent_runtime.deterministic_serialization);
        assert_eq!(restored.intent_runtime.default_execution_mode, "live");
        assert!(!restored.intent_runtime.enforce_artifact_validation);
        assert!(!restored.intent_runtime.reject_nil_identifiers);
    }

    #[test]
    fn test_telegram_owner_id_db_round_trip() {
        let mut settings = Settings::default();
        settings.channels.telegram_owner_id = Some(123456789);

        let map = settings.to_db_map();
        let restored = Settings::from_db_map(&map);
        assert_eq!(restored.channels.telegram_owner_id, Some(123456789));
    }

    #[test]
    fn test_telegram_owner_id_default_none() {
        let settings = Settings::default();
        assert_eq!(settings.channels.telegram_owner_id, None);
    }

    #[test]
    fn test_telegram_owner_id_via_set() {
        let mut settings = Settings::default();
        settings
            .set("channels.telegram_owner_id", "987654321")
            .unwrap();
        assert_eq!(settings.channels.telegram_owner_id, Some(987654321));
    }

    #[test]
    fn test_llm_backend_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        let settings = Settings {
            llm_backend: Some("anthropic".to_string()),
            ollama_base_url: Some("http://localhost:11434".to_string()),
            openai_compatible_base_url: Some("http://my-vllm:8000/v1".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&settings).unwrap();
        std::fs::write(&path, json).unwrap();

        let loaded = Settings::load_from(&path);
        assert_eq!(loaded.llm_backend, Some("anthropic".to_string()));
        assert_eq!(
            loaded.ollama_base_url,
            Some("http://localhost:11434".to_string())
        );
        assert_eq!(
            loaded.openai_compatible_base_url,
            Some("http://my-vllm:8000/v1".to_string())
        );
    }

    #[test]
    fn test_normalize_llm_backend_aliases() {
        assert_eq!(
            normalize_llm_backend("OpenAI-Compatible"),
            Some("openai_compatible".to_string())
        );
        assert_eq!(normalize_llm_backend("NEAR AI"), Some("nearai".to_string()));
        assert_eq!(normalize_llm_backend("unknown"), None);
    }

    #[test]
    fn test_set_llm_backend_validates_and_normalizes() {
        let mut settings = Settings::default();

        settings.set("llm_backend", "OpenAI-Compatible").unwrap();
        assert_eq!(settings.llm_backend, Some("openai_compatible".to_string()));

        let err = settings.set("llm_backend", "made_up_backend").unwrap_err();
        assert!(err.contains("Invalid llm_backend"));
    }

    #[test]
    fn test_load_from_invalid_llm_backend_clears_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"llm_backend":"not_real"}"#).unwrap();

        let loaded = Settings::load_from(&path);
        assert_eq!(loaded.llm_backend, None);
    }

    #[test]
    fn test_load_from_legacy_provider_alias() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"provider":"openai-compatible"}"#).unwrap();

        let loaded = Settings::load_from(&path);
        assert_eq!(loaded.llm_backend, Some("openai_compatible".to_string()));
    }

    #[test]
    fn test_openai_compatible_db_map_round_trip() {
        let settings = Settings {
            llm_backend: Some("openai_compatible".to_string()),
            openai_compatible_base_url: Some("http://my-vllm:8000/v1".to_string()),
            embeddings: EmbeddingsSettings {
                enabled: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let map = settings.to_db_map();
        let restored = Settings::from_db_map(&map);

        assert_eq!(
            restored.llm_backend,
            Some("openai_compatible".to_string()),
            "llm_backend must survive DB round-trip"
        );
        assert_eq!(
            restored.openai_compatible_base_url,
            Some("http://my-vllm:8000/v1".to_string()),
            "openai_compatible_base_url must survive DB round-trip"
        );
        assert!(
            !restored.embeddings.enabled,
            "embeddings.enabled=false must survive DB round-trip"
        );
    }

    #[test]
    fn toml_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut settings = Settings::default();
        settings.agent.name = "toml-bot".to_string();
        settings.heartbeat.enabled = true;
        settings.heartbeat.interval_secs = 900;

        settings.save_toml(&path).unwrap();
        let loaded = Settings::load_toml(&path).unwrap().unwrap();

        assert_eq!(loaded.agent.name, "toml-bot");
        assert!(loaded.heartbeat.enabled);
        assert_eq!(loaded.heartbeat.interval_secs, 900);
    }

    #[test]
    fn toml_missing_file_returns_none() {
        let result = Settings::load_toml(std::path::Path::new("/tmp/nonexistent_config.toml"));
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn toml_invalid_content_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is not valid toml [[[").unwrap();

        let result = Settings::load_toml(&path);
        assert!(result.is_err());
    }

    #[test]
    fn toml_partial_config_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("partial.toml");

        // Only set agent name, everything else should be default
        std::fs::write(&path, "[agent]\nname = \"partial-bot\"\n").unwrap();

        let loaded = Settings::load_toml(&path).unwrap().unwrap();
        assert_eq!(loaded.agent.name, "partial-bot");
        // Defaults preserved
        assert_eq!(loaded.agent.max_parallel_jobs, 5);
        assert!(!loaded.heartbeat.enabled);
    }

    #[test]
    fn toml_header_comment_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        Settings::default().save_toml(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(content.starts_with("# Enclagent configuration file."));
        assert!(content.contains("[agent]"));
        assert!(content.contains("[heartbeat]"));
    }

    #[test]
    fn merge_only_overrides_non_default_values() {
        let mut base = Settings::default();
        base.agent.name = "from-db".to_string();
        base.heartbeat.interval_secs = 600;

        let mut toml_overlay = Settings::default();
        toml_overlay.agent.name = "from-toml".to_string();

        base.merge_from(&toml_overlay);

        assert_eq!(base.agent.name, "from-toml");
        assert_eq!(base.heartbeat.interval_secs, 600);
    }

    #[test]
    fn merge_preserves_base_when_overlay_is_default() {
        let mut base = Settings::default();
        base.agent.name = "custom-name".to_string();
        base.heartbeat.enabled = true;

        let overlay = Settings::default();
        base.merge_from(&overlay);

        assert_eq!(base.agent.name, "custom-name");
        assert!(base.heartbeat.enabled);
    }

    #[test]
    fn toml_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("config.toml");

        Settings::default().save_toml(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn default_toml_path_under_enclagent() {
        let path = Settings::default_toml_path();
        assert!(path.to_string_lossy().contains(".enclagent"));
        assert!(path.to_string_lossy().ends_with("config.toml"));
    }

    #[test]
    fn tunnel_settings_round_trip() {
        let settings = Settings {
            tunnel: TunnelSettings {
                provider: Some("ngrok".to_string()),
                ngrok_token: Some("tok_abc123".to_string()),
                ngrok_domain: Some("my.ngrok.dev".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        // JSON round-trip
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.tunnel.provider, Some("ngrok".to_string()));
        assert_eq!(restored.tunnel.ngrok_token, Some("tok_abc123".to_string()));
        assert_eq!(
            restored.tunnel.ngrok_domain,
            Some("my.ngrok.dev".to_string())
        );
        assert!(restored.tunnel.public_url.is_none());

        // DB map round-trip
        let map = settings.to_db_map();
        let from_db = Settings::from_db_map(&map);
        assert_eq!(from_db.tunnel.provider, Some("ngrok".to_string()));
        assert_eq!(from_db.tunnel.ngrok_token, Some("tok_abc123".to_string()));

        // get/set round-trip
        let mut s = Settings::default();
        s.set("tunnel.provider", "cloudflare").unwrap();
        s.set("tunnel.cf_token", "cf_tok_xyz").unwrap();
        s.set("tunnel.ts_funnel", "true").unwrap();
        assert_eq!(s.tunnel.provider, Some("cloudflare".to_string()));
        assert_eq!(s.tunnel.cf_token, Some("cf_tok_xyz".to_string()));
        assert!(s.tunnel.ts_funnel);
    }

    #[test]
    fn hyperliquid_wallet_verification_defaults_are_conservative() {
        let settings = Settings::default();

        assert_eq!(settings.hyperliquid_runtime.network, "testnet");
        assert_eq!(
            settings.hyperliquid_runtime.paper_live_policy,
            "paper_first"
        );
        assert_eq!(settings.wallet_vault_policy.custody_mode, "operator_wallet");
        assert_eq!(settings.wallet_vault_policy.max_position_size_usd, 1_000);
        assert_eq!(settings.wallet_vault_policy.leverage_cap, 2);
        assert!(settings.wallet_vault_policy.kill_switch_enabled);
        assert_eq!(
            settings.wallet_vault_policy.kill_switch_behavior,
            "pause_agent"
        );
        assert_eq!(settings.verification_backend.backend, "eigencloud_primary");
        assert!(settings.verification_backend.fallback_enabled);
        assert!(
            settings
                .verification_backend
                .fallback_require_signed_receipts
        );
        assert_eq!(settings.copytrading.max_allocation_usd, 5_000);
        assert_eq!(settings.copytrading.per_trade_notional_cap_usd, 750);
        assert_eq!(settings.copytrading.max_leverage, 2);
        assert_eq!(settings.copytrading.max_slippage_bps, 25);
        assert_eq!(
            settings.copytrading.information_sharing_scope,
            "signals_only"
        );
    }

    #[test]
    fn hyperliquid_wallet_verification_db_round_trip() {
        let settings = Settings {
            hyperliquid_runtime: HyperliquidRuntimeSettings {
                network: "mainnet".to_string(),
                api_base_url: Some("https://api.hyperliquid.xyz".to_string()),
                ws_url: Some("wss://api.hyperliquid.xyz/ws".to_string()),
                timeout_ms: 12_000,
                max_retries: 6,
                retry_backoff_ms: 750,
                paper_live_policy: "live_allowed".to_string(),
            },
            wallet_vault_policy: WalletVaultPolicySettings {
                custody_mode: "dual_mode".to_string(),
                operator_wallet_address: Some("0xoperator".to_string()),
                user_wallet_address: Some("0xuser".to_string()),
                vault_address: Some("0xvault".to_string()),
                max_position_size_usd: 25_000,
                leverage_cap: 4,
                kill_switch_enabled: true,
                kill_switch_behavior: "cancel_and_flatten".to_string(),
            },
            verification_backend: VerificationBackendSettings {
                backend: "fallback_only".to_string(),
                eigencloud_endpoint: Some("https://verify.eigencloud.example/v1".to_string()),
                eigencloud_auth_scheme: "api_key".to_string(),
                eigencloud_auth_token: Some("token_abc".to_string()),
                eigencloud_timeout_ms: 7_500,
                fallback_enabled: true,
                fallback_signing_key_id: Some("receipt-key-1".to_string()),
                fallback_chain_path: Some(PathBuf::from("/tmp/receipt-chain.ndjson")),
                fallback_require_signed_receipts: true,
            },
            copytrading: CopyTradingSettings {
                max_allocation_usd: 20_000,
                per_trade_notional_cap_usd: 1_500,
                max_leverage: 3,
                symbol_allowlist: vec!["BTC-USD".to_string(), "SOL-USD".to_string()],
                symbol_denylist: vec!["DOGE-USD".to_string()],
                max_slippage_bps: 15,
                information_sharing_scope: "signals_and_execution".to_string(),
            },
            ..Default::default()
        };

        let map = settings.to_db_map();
        let restored = Settings::from_db_map(&map);

        assert_eq!(restored.hyperliquid_runtime.network, "mainnet");
        assert_eq!(
            restored.hyperliquid_runtime.api_base_url,
            Some("https://api.hyperliquid.xyz".to_string())
        );
        assert_eq!(restored.wallet_vault_policy.custody_mode, "dual_mode");
        assert_eq!(
            restored.wallet_vault_policy.vault_address,
            Some("0xvault".to_string())
        );
        assert_eq!(restored.verification_backend.backend, "fallback_only");
        assert_eq!(
            restored.verification_backend.fallback_chain_path,
            Some(PathBuf::from("/tmp/receipt-chain.ndjson"))
        );
        assert_eq!(restored.copytrading.max_allocation_usd, 20_000);
        assert_eq!(restored.copytrading.max_slippage_bps, 15);
        assert_eq!(
            restored.copytrading.information_sharing_scope,
            "signals_and_execution"
        );
    }

    #[test]
    fn hyperliquid_wallet_verification_set_paths() {
        let mut settings = Settings::default();

        settings
            .set("hyperliquid_runtime.paper_live_policy", "paper_only")
            .unwrap();
        settings
            .set("wallet_vault_policy.max_position_size_usd", "2500")
            .unwrap();
        settings
            .set(
                "verification_backend.eigencloud_endpoint",
                "https://verify.eigencloud.example/v1",
            )
            .unwrap();
        settings
            .set("verification_backend.fallback_enabled", "false")
            .unwrap();
        settings
            .set("copytrading.per_trade_notional_cap_usd", "600")
            .unwrap();
        settings
            .set("copytrading.information_sharing_scope", "full_audit")
            .unwrap();

        assert_eq!(settings.hyperliquid_runtime.paper_live_policy, "paper_only");
        assert_eq!(settings.wallet_vault_policy.max_position_size_usd, 2500);
        assert_eq!(
            settings.verification_backend.eigencloud_endpoint,
            Some("https://verify.eigencloud.example/v1".to_string())
        );
        assert!(!settings.verification_backend.fallback_enabled);
        assert_eq!(settings.copytrading.per_trade_notional_cap_usd, 600);
        assert_eq!(settings.copytrading.information_sharing_scope, "full_audit");
    }

    #[test]
    fn hyperliquid_policy_set_normalizes_alias() {
        let mut settings = Settings::default();
        settings
            .set("hyperliquid_runtime.paper_live_policy", "live")
            .unwrap();
        assert_eq!(
            settings.hyperliquid_runtime.paper_live_policy,
            "live_allowed"
        );
    }

    #[test]
    fn hyperliquid_api_and_verification_endpoints_enforce_allowlists() {
        let mut settings = Settings::default();
        assert!(
            settings
                .set(
                    "hyperliquid_runtime.api_base_url",
                    "https://api.hyperliquid.xyz"
                )
                .is_ok()
        );
        assert!(
            settings
                .set(
                    "verification_backend.eigencloud_endpoint",
                    "https://verify.eigencloud.xyz/v1"
                )
                .is_ok()
        );

        let bad_api = settings
            .set(
                "hyperliquid_runtime.api_base_url",
                "https://evil.example/trade",
            )
            .unwrap_err();
        assert!(bad_api.contains("Invalid hyperliquid_runtime.api_base_url"));

        let bad_verify = settings
            .set(
                "verification_backend.eigencloud_endpoint",
                "https://evil.example/verify",
            )
            .unwrap_err();
        assert!(bad_verify.contains("Invalid verification_backend.eigencloud_endpoint"));
    }
}
