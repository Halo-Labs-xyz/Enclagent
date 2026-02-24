//! Request and response DTOs for the web gateway API.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use crate::platform::{
    InferenceRouteDecision, ModuleCapability, ModuleManifest, ModuleState, OrgMembership,
    OrgWorkspace,
};

// --- Chat ---

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    pub thread_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub message_id: Uuid,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ThreadInfo {
    pub id: Uuid,
    pub state: String,
    pub turn_count: usize,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ThreadListResponse {
    /// The pinned assistant thread (always present after first load).
    pub assistant_thread: Option<ThreadInfo>,
    /// Regular conversation threads.
    pub threads: Vec<ThreadInfo>,
    pub active_thread: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct TurnInfo {
    pub turn_number: usize,
    pub user_input: String,
    pub response: Option<String>,
    pub state: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
}

#[derive(Debug, Serialize)]
pub struct ToolCallInfo {
    pub name: String,
    pub has_result: bool,
    pub has_error: bool,
}

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub thread_id: Uuid,
    pub turns: Vec<TurnInfo>,
    /// Whether there are older messages available.
    #[serde(default)]
    pub has_more: bool,
    /// Cursor for the next page (ISO8601 timestamp of the oldest message returned).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_timestamp: Option<String>,
}

// --- Approval ---

#[derive(Debug, Deserialize)]
pub struct ApprovalRequest {
    pub request_id: String,
    /// "approve", "always", or "deny"
    pub action: String,
    /// Thread that owns the pending approval (so the agent loop finds the right session).
    pub thread_id: Option<String>,
}

// --- SSE Event Types ---

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum SseEvent {
    #[serde(rename = "response")]
    Response { content: String, thread_id: String },
    #[serde(rename = "thinking")]
    Thinking {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    #[serde(rename = "tool_started")]
    ToolStarted {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    #[serde(rename = "tool_completed")]
    ToolCompleted {
        name: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        name: String,
        preview: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    #[serde(rename = "stream_chunk")]
    StreamChunk {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    #[serde(rename = "status")]
    Status {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    #[serde(rename = "job_started")]
    JobStarted {
        job_id: String,
        title: String,
        browse_url: String,
    },
    #[serde(rename = "approval_needed")]
    ApprovalNeeded {
        request_id: String,
        tool_name: String,
        description: String,
        parameters: String,
    },
    #[serde(rename = "auth_required")]
    AuthRequired {
        extension_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        auth_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        setup_url: Option<String>,
    },
    #[serde(rename = "auth_completed")]
    AuthCompleted {
        extension_name: String,
        success: bool,
        message: String,
    },
    #[serde(rename = "error")]
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    #[serde(rename = "heartbeat")]
    Heartbeat,

    // Sandbox job streaming events (worker + Claude Code bridge)
    #[serde(rename = "job_message")]
    JobMessage {
        job_id: String,
        role: String,
        content: String,
    },
    #[serde(rename = "job_tool_use")]
    JobToolUse {
        job_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "job_tool_result")]
    JobToolResult {
        job_id: String,
        tool_name: String,
        output: String,
    },
    #[serde(rename = "job_status")]
    JobStatus { job_id: String, message: String },
    #[serde(rename = "job_result")]
    JobResult {
        job_id: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
}

// --- Memory ---

#[derive(Debug, Serialize)]
pub struct MemoryTreeResponse {
    pub entries: Vec<TreeEntry>,
}

#[derive(Debug, Serialize)]
pub struct TreeEntry {
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Serialize)]
pub struct MemoryListResponse {
    pub path: String,
    pub entries: Vec<ListEntry>,
}

#[derive(Debug, Serialize)]
pub struct ListEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MemoryReadResponse {
    pub path: String,
    pub content: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MemoryWriteRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct MemoryWriteResponse {
    pub path: String,
    pub status: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct MemorySearchRequest {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct MemorySearchResponse {
    pub results: Vec<SearchHit>,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub path: String,
    pub content: String,
    pub score: f64,
}

// --- Jobs ---

#[derive(Debug, Serialize)]
pub struct JobInfo {
    pub id: Uuid,
    pub title: String,
    pub state: String,
    pub user_id: String,
    pub created_at: String,
    pub started_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobListResponse {
    pub jobs: Vec<JobInfo>,
}

#[derive(Debug, Serialize)]
pub struct JobSummaryResponse {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub failed: usize,
    pub stuck: usize,
}

#[derive(Debug, Serialize)]
pub struct JobDetailResponse {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub state: String,
    pub user_id: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub elapsed_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browse_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_mode: Option<String>,
    pub transitions: Vec<TransitionInfo>,
}

// --- Project Files ---

#[derive(Debug, Serialize)]
pub struct ProjectFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Serialize)]
pub struct ProjectFilesResponse {
    pub entries: Vec<ProjectFileEntry>,
}

#[derive(Debug, Serialize)]
pub struct ProjectFileReadResponse {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct TransitionInfo {
    pub from: String,
    pub to: String,
    pub timestamp: String,
    pub reason: Option<String>,
}

// --- Extensions ---

#[derive(Debug, Serialize)]
pub struct ExtensionInfo {
    pub name: String,
    pub kind: String,
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub authenticated: bool,
    pub active: bool,
    pub tools: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ExtensionListResponse {
    pub extensions: Vec<ExtensionInfo>,
}

#[derive(Debug, Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct ToolListResponse {
    pub tools: Vec<ToolInfo>,
}

#[derive(Debug, Deserialize)]
pub struct InstallExtensionRequest {
    pub name: String,
    pub url: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ActionResponse {
    pub success: bool,
    pub message: String,
    /// Auth URL to open (when activation requires OAuth).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
    /// Whether the extension is waiting for a manual token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub awaiting_token: Option<bool>,
    /// Instructions for manual token entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

impl ActionResponse {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            auth_url: None,
            awaiting_token: None,
            instructions: None,
        }
    }

    pub fn fail(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            auth_url: None,
            awaiting_token: None,
            instructions: None,
        }
    }
}

// --- Skills ---

#[derive(Debug, Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub version: String,
    pub trust: String,
    pub source: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SkillListResponse {
    pub skills: Vec<SkillInfo>,
    pub count: usize,
}

#[derive(Debug, Deserialize)]
pub struct SkillSearchRequest {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct SkillSearchResponse {
    pub catalog: Vec<serde_json::Value>,
    pub installed: Vec<SkillInfo>,
    pub registry_url: String,
}

#[derive(Debug, Deserialize)]
pub struct SkillInstallRequest {
    pub name: String,
    pub url: Option<String>,
    pub content: Option<String>,
}

// --- Auth Token ---

/// Request to submit an auth token for an extension (dedicated endpoint).
#[derive(Debug, Deserialize)]
pub struct AuthTokenRequest {
    pub extension_name: String,
    pub token: String,
}

/// Request to cancel an in-progress auth flow.
#[derive(Debug, Deserialize)]
pub struct AuthCancelRequest {
    pub extension_name: String,
}

// --- WebSocket ---

/// Message sent by a WebSocket client to the server.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum WsClientMessage {
    /// Send a chat message to the agent.
    #[serde(rename = "message")]
    Message {
        content: String,
        thread_id: Option<String>,
    },
    /// Approve or deny a pending tool execution.
    #[serde(rename = "approval")]
    Approval {
        request_id: String,
        /// "approve", "always", or "deny"
        action: String,
        /// Thread that owns the pending approval.
        thread_id: Option<String>,
    },
    /// Submit an auth token for an extension (bypasses message pipeline).
    #[serde(rename = "auth_token")]
    AuthToken {
        extension_name: String,
        token: String,
    },
    /// Cancel an in-progress auth flow.
    #[serde(rename = "auth_cancel")]
    AuthCancel { extension_name: String },
    /// Client heartbeat ping.
    #[serde(rename = "ping")]
    Ping,
}

/// Message sent by the server to a WebSocket client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum WsServerMessage {
    /// An SSE-style event forwarded over WebSocket.
    #[serde(rename = "event")]
    Event {
        /// The event sub-type (response, thinking, tool_started, etc.)
        event_type: String,
        /// The event payload as a JSON value.
        data: serde_json::Value,
    },
    /// Server heartbeat pong.
    #[serde(rename = "pong")]
    Pong,
    /// Error message.
    #[serde(rename = "error")]
    Error { message: String },
}

impl WsServerMessage {
    /// Create a WsServerMessage from an SseEvent.
    pub fn from_sse_event(event: &SseEvent) -> Self {
        let event_type = match event {
            SseEvent::Response { .. } => "response",
            SseEvent::Thinking { .. } => "thinking",
            SseEvent::ToolStarted { .. } => "tool_started",
            SseEvent::ToolCompleted { .. } => "tool_completed",
            SseEvent::ToolResult { .. } => "tool_result",
            SseEvent::StreamChunk { .. } => "stream_chunk",
            SseEvent::Status { .. } => "status",
            SseEvent::JobStarted { .. } => "job_started",
            SseEvent::ApprovalNeeded { .. } => "approval_needed",
            SseEvent::AuthRequired { .. } => "auth_required",
            SseEvent::AuthCompleted { .. } => "auth_completed",
            SseEvent::Error { .. } => "error",
            SseEvent::Heartbeat => "heartbeat",
            SseEvent::JobMessage { .. } => "job_message",
            SseEvent::JobToolUse { .. } => "job_tool_use",
            SseEvent::JobToolResult { .. } => "job_tool_result",
            SseEvent::JobStatus { .. } => "job_status",
            SseEvent::JobResult { .. } => "job_result",
        };
        let data = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
        WsServerMessage::Event {
            event_type: event_type.to_string(),
            data,
        }
    }
}

// --- Routines ---

#[derive(Debug, Serialize)]
pub struct RoutineInfo {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub trigger_type: String,
    pub trigger_summary: String,
    pub action_type: String,
    pub last_run_at: Option<String>,
    pub next_fire_at: Option<String>,
    pub run_count: u64,
    pub consecutive_failures: u32,
    pub status: String,
    pub health: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_secret_configured: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct RoutineListResponse {
    pub routines: Vec<RoutineInfo>,
}

#[derive(Debug, Serialize)]
pub struct RoutineSummaryResponse {
    pub total: u64,
    pub enabled: u64,
    pub disabled: u64,
    pub failing: u64,
    pub runs_today: u64,
}

#[derive(Debug, Serialize)]
pub struct RoutineDetailResponse {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub trigger: serde_json::Value,
    pub action: serde_json::Value,
    pub guardrails: serde_json::Value,
    pub notify: serde_json::Value,
    pub last_run_at: Option<String>,
    pub next_fire_at: Option<String>,
    pub run_count: u64,
    pub consecutive_failures: u32,
    pub status: String,
    pub health: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_secret_configured: Option<bool>,
    pub recent_runs: Vec<RoutineRunInfo>,
}

#[derive(Debug, Serialize)]
pub struct RoutineRunInfo {
    pub id: Uuid,
    pub trigger_type: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub status: String,
    pub result_summary: Option<String>,
    pub tokens_used: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct RoutineTriggerVisibilityResponse {
    pub generated_at: String,
    pub channels: Vec<RoutineChannelHealth>,
    pub webhook: WebhookTriggerVisibility,
}

#[derive(Debug, Serialize)]
pub struct RoutineChannelHealth {
    pub channel: String,
    pub total_routines: u64,
    pub enabled_routines: u64,
    pub healthy_routines: u64,
    pub failing_routines: u64,
    pub status: String,
    pub last_run_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WebhookTriggerVisibility {
    pub total_routines: u64,
    pub enabled_routines: u64,
    pub with_secret: u64,
    pub status: String,
    pub routes: Vec<WebhookRouteInfo>,
}

#[derive(Debug, Serialize)]
pub struct WebhookRouteInfo {
    pub routine_id: Uuid,
    pub routine_name: String,
    pub path: String,
    pub enabled: bool,
    pub secret_configured: bool,
    pub status: String,
}

// --- Settings ---

#[derive(Debug, Serialize)]
pub struct SettingResponse {
    pub key: String,
    pub value: serde_json::Value,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct SettingsListResponse {
    pub settings: Vec<SettingResponse>,
}

#[derive(Debug, Deserialize)]
pub struct SettingWriteRequest {
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct SettingsImportRequest {
    pub settings: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SettingsExportResponse {
    pub settings: std::collections::HashMap<String, serde_json::Value>,
}

// --- Modules ---

#[derive(Debug, Serialize)]
pub struct ModuleCatalogResponse {
    pub modules: Vec<ModuleManifest>,
}

#[derive(Debug, Serialize)]
pub struct ModuleStateResponse {
    pub modules: Vec<ModuleState>,
}

#[derive(Debug, Serialize)]
pub struct ModuleUpdateResponse {
    pub module: ModuleState,
}

#[derive(Debug, Deserialize)]
pub struct ModuleConfigUpdateRequest {
    pub config: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ModuleHealthResponse {
    pub module_id: String,
    pub enabled: bool,
    pub status: String,
    pub checks: serde_json::Value,
}

// --- Org Workspace ---

#[derive(Debug, Serialize)]
pub struct OrgCurrentResponse {
    pub workspace: OrgWorkspace,
    pub membership: OrgMembership,
}

#[derive(Debug, Serialize)]
pub struct OrgMembersResponse {
    pub workspace: OrgWorkspace,
    pub members: Vec<OrgMembership>,
}

#[derive(Debug, Deserialize)]
pub struct OrgInviteRequest {
    pub member_id: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct OrgRoleUpdateRequest {
    pub role: String,
}

// --- Frontdoor Provisioning ---

#[derive(Debug, Serialize)]
pub struct FrontdoorBootstrapResponse {
    pub enabled: bool,
    pub require_privy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privy_app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privy_client_id: Option<String>,
    pub poll_interval_ms: u64,
    pub mandatory_steps: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct FrontdoorChallengeRequest {
    pub wallet_address: String,
    #[serde(default)]
    pub privy_user_id: Option<String>,
    #[serde(default)]
    pub chain_id: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct FrontdoorChallengeResponse {
    pub session_id: String,
    pub wallet_address: String,
    pub message: String,
    pub expires_at: String,
    pub version: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FrontdoorUserConfig {
    #[serde(default = "default_frontdoor_config_version")]
    pub config_version: u32,
    #[serde(default = "default_frontdoor_profile_domain")]
    pub profile_domain: String,
    #[serde(default)]
    pub domain_overrides: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub inference_summary: Option<String>,
    #[serde(default)]
    pub inference_confidence: Option<f64>,
    #[serde(default)]
    pub inference_warnings: Vec<String>,
    pub profile_name: String,
    pub hyperliquid_network: String,
    pub paper_live_policy: String,
    #[serde(default)]
    pub hyperliquid_api_base_url: Option<String>,
    #[serde(default)]
    pub hyperliquid_ws_url: Option<String>,
    pub request_timeout_ms: u64,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
    pub max_position_size_usd: u64,
    pub leverage_cap: u32,
    pub max_allocation_usd: u64,
    pub per_trade_notional_cap_usd: u64,
    pub max_leverage: u32,
    pub max_slippage_bps: u32,
    #[serde(default)]
    pub symbol_allowlist: Vec<String>,
    #[serde(default)]
    pub symbol_denylist: Vec<String>,
    pub custody_mode: String,
    #[serde(default)]
    pub operator_wallet_address: Option<String>,
    #[serde(default)]
    pub user_wallet_address: Option<String>,
    #[serde(default)]
    pub vault_address: Option<String>,
    pub information_sharing_scope: String,
    pub kill_switch_enabled: bool,
    pub kill_switch_behavior: String,
    #[serde(default)]
    pub enable_memory: bool,
    pub gateway_auth_key: String,
    #[serde(default)]
    pub eigencloud_auth_key: Option<String>,
    #[serde(default = "default_frontdoor_verification_backend")]
    pub verification_backend: String,
    #[serde(default)]
    pub verification_eigencloud_endpoint: Option<String>,
    #[serde(default = "default_frontdoor_eigencloud_auth_scheme")]
    pub verification_eigencloud_auth_scheme: String,
    #[serde(default = "default_frontdoor_eigencloud_timeout_ms")]
    pub verification_eigencloud_timeout_ms: u64,
    #[serde(default = "default_true")]
    pub verification_fallback_enabled: bool,
    #[serde(default)]
    pub verification_fallback_signing_key_id: Option<String>,
    #[serde(default)]
    pub verification_fallback_chain_path: Option<String>,
    #[serde(default = "default_true")]
    pub verification_fallback_require_signed_receipts: bool,
    pub accept_terms: bool,
}

fn default_true() -> bool {
    true
}

fn default_frontdoor_config_version() -> u32 {
    2
}

fn default_frontdoor_profile_domain() -> String {
    "general".to_string()
}

fn default_frontdoor_verification_backend() -> String {
    "eigencloud_primary".to_string()
}

fn default_frontdoor_eigencloud_auth_scheme() -> String {
    "bearer".to_string()
}

fn default_frontdoor_eigencloud_timeout_ms() -> u64 {
    5_000
}

#[derive(Debug, Deserialize)]
pub struct FrontdoorVerifyRequest {
    pub session_id: String,
    pub wallet_address: String,
    #[serde(default)]
    pub privy_user_id: Option<String>,
    #[serde(default)]
    pub privy_identity_token: Option<String>,
    #[serde(default)]
    pub privy_access_token: Option<String>,
    pub message: String,
    pub signature: String,
    pub config: FrontdoorUserConfig,
}

#[derive(Debug, Deserialize)]
pub struct FrontdoorSuggestConfigRequest {
    pub wallet_address: String,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub gateway_auth_key: Option<String>,
    #[serde(default)]
    pub base_config: Option<FrontdoorUserConfig>,
}

#[derive(Debug, Serialize)]
pub struct FrontdoorSuggestConfigResponse {
    pub config: FrontdoorUserConfig,
    pub assumptions: Vec<String>,
    pub warnings: Vec<String>,
    pub validated: bool,
}

#[derive(Debug, Serialize)]
pub struct FrontdoorConfigContractResponse {
    pub contract_id: String,
    pub current_config_version: u32,
    pub supported_config_versions: Vec<u32>,
    pub supported_domains: Vec<String>,
    pub domain_profiles: Vec<FrontdoorDomainProfile>,
    pub mandatory_steps: Vec<String>,
    pub enums: FrontdoorConfigEnums,
    pub defaults: FrontdoorConfigDefaults,
}

#[derive(Debug, Serialize)]
pub struct FrontdoorDomainProfile {
    pub domain: String,
    pub default_modules: Vec<String>,
    pub required_fields: Vec<String>,
    pub optional_fields: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct FrontdoorConfigEnums {
    pub hyperliquid_network: Vec<String>,
    pub paper_live_policy: Vec<String>,
    pub custody_mode: Vec<String>,
    pub information_sharing_scope: Vec<String>,
    pub kill_switch_behavior: Vec<String>,
    pub verification_backend: Vec<String>,
    pub verification_eigencloud_auth_scheme: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct FrontdoorConfigDefaults {
    pub profile_domain: String,
    pub hyperliquid_network: String,
    pub paper_live_policy: String,
    pub request_timeout_ms: u64,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
    pub max_position_size_usd: u64,
    pub leverage_cap: u32,
    pub max_allocation_usd: u64,
    pub per_trade_notional_cap_usd: u64,
    pub max_leverage: u32,
    pub max_slippage_bps: u32,
    pub symbol_allowlist: Vec<String>,
    pub symbol_denylist: Vec<String>,
    pub custody_mode: String,
    pub information_sharing_scope: String,
    pub kill_switch_enabled: bool,
    pub kill_switch_behavior: String,
    pub enable_memory: bool,
    pub verification_backend: String,
    pub verification_eigencloud_auth_scheme: String,
    pub verification_eigencloud_timeout_ms: u64,
    pub verification_fallback_enabled: bool,
    pub verification_fallback_require_signed_receipts: bool,
}

#[derive(Debug, Serialize)]
pub struct FrontdoorVerifyResponse {
    pub session_id: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct FrontdoorSessionResponse {
    pub session_id: String,
    pub wallet_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privy_user_id: Option<String>,
    pub version: u64,
    pub status: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eigen_app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub expires_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_name: Option<String>,
}

// --- Health ---

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub channel: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ChannelStatusResponse {
    pub generated_at: String,
    pub status: String,
    pub channels: Vec<ChannelStatusInfo>,
}

#[derive(Debug, Serialize)]
pub struct ChannelStatusInfo {
    pub name: String,
    pub configured: bool,
    pub enabled: bool,
    pub healthy: bool,
    pub status: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct VerificationStatusResponse {
    pub generated_at: String,
    pub status: String,
    pub backend: String,
    pub backend_status: String,
    pub eigencloud: VerificationPrimarySurface,
    pub fallback: VerificationFallbackSurface,
}

#[derive(Debug, Serialize)]
pub struct VerificationPrimarySurface {
    pub configured: bool,
    pub endpoint: Option<String>,
    pub auth_scheme: String,
    pub token_configured: bool,
    pub timeout_ms: u64,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct VerificationFallbackSurface {
    pub enabled: bool,
    pub require_signed_receipts: bool,
    pub signing_key_configured: bool,
    pub chain_path: Option<String>,
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- WsClientMessage deserialization tests ----

    #[test]
    fn test_ws_client_message_parse() {
        let json = r#"{"type":"message","content":"hello","thread_id":"t1"}"#;
        let msg: WsClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            WsClientMessage::Message { content, thread_id } => {
                assert_eq!(content, "hello");
                assert_eq!(thread_id.as_deref(), Some("t1"));
            }
            _ => panic!("Expected Message variant"),
        }
    }

    #[test]
    fn test_ws_client_message_no_thread() {
        let json = r#"{"type":"message","content":"hi"}"#;
        let msg: WsClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            WsClientMessage::Message { content, thread_id } => {
                assert_eq!(content, "hi");
                assert!(thread_id.is_none());
            }
            _ => panic!("Expected Message variant"),
        }
    }

    #[test]
    fn test_ws_client_approval_parse() {
        let json =
            r#"{"type":"approval","request_id":"abc-123","action":"approve","thread_id":"t1"}"#;
        let msg: WsClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            WsClientMessage::Approval {
                request_id,
                action,
                thread_id,
            } => {
                assert_eq!(request_id, "abc-123");
                assert_eq!(action, "approve");
                assert_eq!(thread_id.as_deref(), Some("t1"));
            }
            _ => panic!("Expected Approval variant"),
        }
    }

    #[test]
    fn test_ws_client_approval_parse_no_thread() {
        let json = r#"{"type":"approval","request_id":"abc-123","action":"deny"}"#;
        let msg: WsClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            WsClientMessage::Approval {
                request_id,
                action,
                thread_id,
            } => {
                assert_eq!(request_id, "abc-123");
                assert_eq!(action, "deny");
                assert!(thread_id.is_none());
            }
            _ => panic!("Expected Approval variant"),
        }
    }

    #[test]
    fn test_ws_client_ping_parse() {
        let json = r#"{"type":"ping"}"#;
        let msg: WsClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, WsClientMessage::Ping));
    }

    #[test]
    fn test_ws_client_unknown_type_fails() {
        let json = r#"{"type":"unknown"}"#;
        let result: Result<WsClientMessage, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // ---- WsServerMessage serialization tests ----

    #[test]
    fn test_ws_server_pong_serialize() {
        let msg = WsServerMessage::Pong;
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"pong"}"#);
    }

    #[test]
    fn test_ws_server_error_serialize() {
        let msg = WsServerMessage::Error {
            message: "bad request".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["message"], "bad request");
    }

    #[test]
    fn test_ws_server_from_sse_response() {
        let sse = SseEvent::Response {
            content: "hello".to_string(),
            thread_id: "t1".to_string(),
        };
        let ws = WsServerMessage::from_sse_event(&sse);
        match ws {
            WsServerMessage::Event { event_type, data } => {
                assert_eq!(event_type, "response");
                assert_eq!(data["content"], "hello");
                assert_eq!(data["thread_id"], "t1");
            }
            _ => panic!("Expected Event variant"),
        }
    }

    #[test]
    fn test_ws_server_from_sse_thinking() {
        let sse = SseEvent::Thinking {
            message: "reasoning...".to_string(),
            thread_id: None,
        };
        let ws = WsServerMessage::from_sse_event(&sse);
        match ws {
            WsServerMessage::Event { event_type, data } => {
                assert_eq!(event_type, "thinking");
                assert_eq!(data["message"], "reasoning...");
            }
            _ => panic!("Expected Event variant"),
        }
    }

    #[test]
    fn test_ws_server_from_sse_approval_needed() {
        let sse = SseEvent::ApprovalNeeded {
            request_id: "r1".to_string(),
            tool_name: "shell".to_string(),
            description: "Run ls".to_string(),
            parameters: "{}".to_string(),
        };
        let ws = WsServerMessage::from_sse_event(&sse);
        match ws {
            WsServerMessage::Event { event_type, data } => {
                assert_eq!(event_type, "approval_needed");
                assert_eq!(data["tool_name"], "shell");
            }
            _ => panic!("Expected Event variant"),
        }
    }

    #[test]
    fn test_ws_server_from_sse_heartbeat() {
        let sse = SseEvent::Heartbeat;
        let ws = WsServerMessage::from_sse_event(&sse);
        match ws {
            WsServerMessage::Event { event_type, .. } => {
                assert_eq!(event_type, "heartbeat");
            }
            _ => panic!("Expected Event variant"),
        }
    }

    // ---- Auth type tests ----

    #[test]
    fn test_ws_client_auth_token_parse() {
        let json = r#"{"type":"auth_token","extension_name":"notion","token":"sk-123"}"#;
        let msg: WsClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            WsClientMessage::AuthToken {
                extension_name,
                token,
            } => {
                assert_eq!(extension_name, "notion");
                assert_eq!(token, "sk-123");
            }
            _ => panic!("Expected AuthToken variant"),
        }
    }

    #[test]
    fn test_ws_client_auth_cancel_parse() {
        let json = r#"{"type":"auth_cancel","extension_name":"notion"}"#;
        let msg: WsClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            WsClientMessage::AuthCancel { extension_name } => {
                assert_eq!(extension_name, "notion");
            }
            _ => panic!("Expected AuthCancel variant"),
        }
    }

    #[test]
    fn test_sse_auth_required_serialize() {
        let event = SseEvent::AuthRequired {
            extension_name: "notion".to_string(),
            instructions: Some("Get your token from...".to_string()),
            auth_url: None,
            setup_url: Some("https://notion.so/integrations".to_string()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "auth_required");
        assert_eq!(parsed["extension_name"], "notion");
        assert_eq!(parsed["instructions"], "Get your token from...");
        assert!(parsed.get("auth_url").is_none());
        assert_eq!(parsed["setup_url"], "https://notion.so/integrations");
    }

    #[test]
    fn test_sse_auth_completed_serialize() {
        let event = SseEvent::AuthCompleted {
            extension_name: "notion".to_string(),
            success: true,
            message: "notion authenticated (3 tools loaded)".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "auth_completed");
        assert_eq!(parsed["extension_name"], "notion");
        assert_eq!(parsed["success"], true);
    }

    #[test]
    fn test_ws_server_from_sse_auth_required() {
        let sse = SseEvent::AuthRequired {
            extension_name: "openai".to_string(),
            instructions: Some("Enter API key".to_string()),
            auth_url: None,
            setup_url: None,
        };
        let ws = WsServerMessage::from_sse_event(&sse);
        match ws {
            WsServerMessage::Event { event_type, data } => {
                assert_eq!(event_type, "auth_required");
                assert_eq!(data["extension_name"], "openai");
            }
            _ => panic!("Expected Event variant"),
        }
    }

    #[test]
    fn test_ws_server_from_sse_auth_completed() {
        let sse = SseEvent::AuthCompleted {
            extension_name: "slack".to_string(),
            success: false,
            message: "Invalid token".to_string(),
        };
        let ws = WsServerMessage::from_sse_event(&sse);
        match ws {
            WsServerMessage::Event { event_type, data } => {
                assert_eq!(event_type, "auth_completed");
                assert_eq!(data["success"], false);
            }
            _ => panic!("Expected Event variant"),
        }
    }

    #[test]
    fn test_auth_token_request_deserialize() {
        let json = r#"{"extension_name":"telegram","token":"bot12345"}"#;
        let req: AuthTokenRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.extension_name, "telegram");
        assert_eq!(req.token, "bot12345");
    }

    #[test]
    fn test_auth_cancel_request_deserialize() {
        let json = r#"{"extension_name":"telegram"}"#;
        let req: AuthCancelRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.extension_name, "telegram");
    }
}
