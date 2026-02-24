//! Error types for Enclagent.

use std::time::Duration;

use serde::Serialize;
use uuid::Uuid;

/// Top-level error type for the agent.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Database error: {0}")]
    Database(#[from] DatabaseError),

    #[error("Channel error: {0}")]
    Channel(#[from] ChannelError),

    #[error("LLM error: {0}")]
    Llm(#[from] LlmError),

    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    #[error("Safety error: {0}")]
    Safety(#[from] SafetyError),

    #[error("Job error: {0}")]
    Job(#[from] JobError),

    #[error("Estimation error: {0}")]
    Estimation(#[from] EstimationError),

    #[error("Evaluation error: {0}")]
    Evaluation(#[from] EvaluationError),

    #[error("Repair error: {0}")]
    Repair(#[from] RepairError),

    #[error("Workspace error: {0}")]
    Workspace(#[from] WorkspaceError),

    #[error("Hook error: {0}")]
    Hook(#[from] crate::hooks::HookError),

    #[error("Orchestrator error: {0}")]
    Orchestrator(#[from] OrchestratorError),

    #[error("Worker error: {0}")]
    Worker(#[from] WorkerError),
}

/// Runtime pipeline stage for WS-2 status reporting.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStage {
    Intent,
    Execution,
    Verification,
}

/// Runtime stage state for WS-2 status reporting.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStageState {
    Started,
    Completed,
    Failed,
    Blocked,
    AwaitingInput,
}

/// Structured failure domains for REPL/web incident surface integration.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorDomain {
    Auth,
    Channel,
    Mcp,
    Verification,
    Unknown,
}

/// Structured runtime error payload for status events.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeErrorPayload {
    pub domain: RuntimeErrorDomain,
    pub code: &'static str,
    pub retryable: bool,
    pub message: String,
}

impl RuntimeErrorPayload {
    fn new(
        domain: RuntimeErrorDomain,
        code: &'static str,
        retryable: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            domain,
            code,
            retryable,
            message: message.into(),
        }
    }
}

/// Structured runtime status payload encoded into `StatusUpdate::Status`.
///
/// Channels currently transport free-form status strings. This envelope keeps
/// wire compatibility while giving REPL/web consumers a stable, parseable
/// payload shape for staged status and incident handling.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeStatusPayload {
    pub kind: &'static str,
    pub version: u8,
    pub stage: RuntimeStage,
    pub state: RuntimeStageState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RuntimeErrorPayload>,
}

impl RuntimeStatusPayload {
    const KIND: &'static str = "ws2_runtime_status";
    const VERSION: u8 = 1;

    pub fn new(stage: RuntimeStage, state: RuntimeStageState) -> Self {
        Self {
            kind: Self::KIND,
            version: Self::VERSION,
            stage,
            state,
            intent: None,
            detail: None,
            error: None,
        }
    }

    pub fn with_intent(mut self, intent: impl Into<String>) -> Self {
        self.intent = Some(intent.into());
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_error(mut self, error: RuntimeErrorPayload) -> Self {
        self.error = Some(error);
        self
    }

    /// Serialize for transport over existing string-only status channels.
    pub fn to_status_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            format!(
                "{{\"kind\":\"{}\",\"version\":{},\"stage\":\"{:?}\",\"state\":\"{:?}\"}}",
                Self::KIND,
                Self::VERSION,
                self.stage,
                self.state
            )
        })
    }
}

/// Configuration-related errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing required environment variable: {0}")]
    MissingEnvVar(String),

    #[error("Missing required configuration: {key}. {hint}")]
    MissingRequired { key: String, hint: String },

    #[error("Invalid configuration value for {key}: {message}")]
    InvalidValue { key: String, message: String },

    #[error("Failed to parse configuration: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Database-related errors.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Connection pool error: {0}")]
    Pool(String),

    #[error("Query failed: {0}")]
    Query(String),

    #[error("Entity not found: {entity} with id {id}")]
    NotFound { entity: String, id: String },

    #[error("Constraint violation: {0}")]
    Constraint(String),

    #[error("Migration failed: {0}")]
    Migration(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[cfg(feature = "postgres")]
    #[error("PostgreSQL error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    #[cfg(feature = "postgres")]
    #[error("Pool build error: {0}")]
    PoolBuild(#[from] deadpool_postgres::BuildError),

    #[cfg(feature = "postgres")]
    #[error("Pool runtime error: {0}")]
    PoolRuntime(#[from] deadpool_postgres::PoolError),

    #[cfg(feature = "libsql")]
    #[error("LibSQL error: {0}")]
    LibSql(#[from] libsql::Error),
}

/// Channel-related errors.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("Channel {name} failed to start: {reason}")]
    StartupFailed { name: String, reason: String },

    #[error("Channel {name} disconnected: {reason}")]
    Disconnected { name: String, reason: String },

    #[error("Failed to send response on channel {name}: {reason}")]
    SendFailed { name: String, reason: String },

    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    #[error("Authentication failed for channel {name}: {reason}")]
    AuthFailed { name: String, reason: String },

    #[error("Rate limited on channel {name}")]
    RateLimited { name: String },

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Channel health check failed: {name}")]
    HealthCheckFailed { name: String },
}

/// LLM provider errors.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("Provider {provider} request failed: {reason}")]
    RequestFailed { provider: String, reason: String },

    #[error("Provider {provider} rate limited, retry after {retry_after:?}")]
    RateLimited {
        provider: String,
        retry_after: Option<Duration>,
    },

    #[error("Invalid response from {provider}: {reason}")]
    InvalidResponse { provider: String, reason: String },

    #[error("Context length exceeded: {used} tokens used, {limit} allowed")]
    ContextLengthExceeded { used: usize, limit: usize },

    #[error("Model {model} not available on provider {provider}")]
    ModelNotAvailable { provider: String, model: String },

    #[error("Authentication failed for provider {provider}")]
    AuthFailed { provider: String },

    #[error("Session expired for provider {provider}")]
    SessionExpired { provider: String },

    #[error("Session renewal failed for provider {provider}: {reason}")]
    SessionRenewalFailed { provider: String, reason: String },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Tool execution errors.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool {name} not found")]
    NotFound { name: String },

    #[error("Tool {name} execution failed: {reason}")]
    ExecutionFailed { name: String, reason: String },

    #[error("Tool {name} timed out after {timeout:?}")]
    Timeout { name: String, timeout: Duration },

    #[error("Invalid parameters for tool {name}: {reason}")]
    InvalidParameters { name: String, reason: String },

    #[error("Tool {name} is disabled: {reason}")]
    Disabled { name: String, reason: String },

    #[error("Sandbox error for tool {name}: {reason}")]
    Sandbox { name: String, reason: String },

    #[error("Tool {name} requires authentication")]
    AuthRequired { name: String },

    #[error("Tool builder failed: {0}")]
    BuilderFailed(String),
}

/// Safety/sanitization errors.
#[derive(Debug, thiserror::Error)]
pub enum SafetyError {
    #[error("Potential prompt injection detected: {pattern}")]
    InjectionDetected { pattern: String },

    #[error("Output exceeded maximum length: {length} > {max}")]
    OutputTooLarge { length: usize, max: usize },

    #[error("Blocked content pattern detected: {pattern}")]
    BlockedContent { pattern: String },

    #[error("Validation failed: {reason}")]
    ValidationFailed { reason: String },

    #[error("Policy violation: {rule}")]
    PolicyViolation { rule: String },
}

/// Job-related errors.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("Job {id} not found")]
    NotFound { id: Uuid },

    #[error("Job {id} already in state {state}, cannot transition to {target}")]
    InvalidTransition {
        id: Uuid,
        state: String,
        target: String,
    },

    #[error("Job {id} failed: {reason}")]
    Failed { id: Uuid, reason: String },

    #[error("Job {id} stuck for {duration:?}")]
    Stuck { id: Uuid, duration: Duration },

    #[error("Maximum parallel jobs ({max}) exceeded")]
    MaxJobsExceeded { max: usize },

    #[error("Job {id} context error: {reason}")]
    ContextError { id: Uuid, reason: String },
}

/// Estimation errors.
#[derive(Debug, thiserror::Error)]
pub enum EstimationError {
    #[error("Insufficient data for estimation: need {needed} samples, have {have}")]
    InsufficientData { needed: usize, have: usize },

    #[error("Estimation calculation failed: {reason}")]
    CalculationFailed { reason: String },

    #[error("Invalid estimation parameters: {reason}")]
    InvalidParameters { reason: String },
}

/// Evaluation errors.
#[derive(Debug, thiserror::Error)]
pub enum EvaluationError {
    #[error("Evaluation failed for job {job_id}: {reason}")]
    Failed { job_id: Uuid, reason: String },

    #[error("Missing required evaluation data: {field}")]
    MissingData { field: String },

    #[error("Invalid evaluation criteria: {reason}")]
    InvalidCriteria { reason: String },
}

/// Self-repair errors.
#[derive(Debug, thiserror::Error)]
pub enum RepairError {
    #[error("Repair failed for {target_type} {target_id}: {reason}")]
    Failed {
        target_type: String,
        target_id: Uuid,
        reason: String,
    },

    #[error("Maximum repair attempts ({max}) exceeded for {target_type} {target_id}")]
    MaxAttemptsExceeded {
        target_type: String,
        target_id: Uuid,
        max: u32,
    },

    #[error("Cannot diagnose issue for {target_type} {target_id}: {reason}")]
    DiagnosisFailed {
        target_type: String,
        target_id: Uuid,
        reason: String,
    },
}

/// Workspace/memory errors.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("Document not found: {doc_type} for user {user_id}")]
    DocumentNotFound { doc_type: String, user_id: String },

    #[error("Search failed: {reason}")]
    SearchFailed { reason: String },

    #[error("Embedding generation failed: {reason}")]
    EmbeddingFailed { reason: String },

    #[error("Document chunking failed: {reason}")]
    ChunkingFailed { reason: String },

    #[error("Invalid document type: {doc_type}")]
    InvalidDocType { doc_type: String },

    #[error("Workspace not initialized for user {user_id}")]
    NotInitialized { user_id: String },

    #[error("Heartbeat error: {reason}")]
    HeartbeatError { reason: String },
}

/// Orchestrator errors (internal API, container management).
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("Container creation failed for job {job_id}: {reason}")]
    ContainerCreationFailed { job_id: Uuid, reason: String },

    #[error("Container not found for job {job_id}")]
    ContainerNotFound { job_id: Uuid },

    #[error("Container for job {job_id} is in unexpected state: {state}")]
    InvalidContainerState { job_id: Uuid, state: String },

    #[error("Internal API error: {reason}")]
    ApiError { reason: String },

    #[error("Docker error: {reason}")]
    Docker { reason: String },
}

/// Worker errors (container-side execution).
#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("Failed to connect to orchestrator at {url}: {reason}")]
    ConnectionFailed { url: String, reason: String },

    #[error("LLM proxy request failed: {reason}")]
    LlmProxyFailed { reason: String },

    #[error("Secret resolution failed for {secret_name}: {reason}")]
    SecretResolveFailed { secret_name: String, reason: String },

    #[error("Orchestrator returned error for job {job_id}: {reason}")]
    OrchestratorRejected { job_id: Uuid, reason: String },

    #[error("Worker execution failed: {reason}")]
    ExecutionFailed { reason: String },

    #[error("Missing worker token (ENCLAGENT_WORKER_TOKEN not set)")]
    MissingToken,
}

fn is_mcp_related(value: &str) -> bool {
    value.to_ascii_lowercase().contains("mcp")
}

impl LlmError {
    fn to_runtime_error_payload(&self) -> RuntimeErrorPayload {
        match self {
            Self::AuthFailed { provider } => RuntimeErrorPayload::new(
                RuntimeErrorDomain::Auth,
                "auth.provider_failed",
                false,
                format!("Authentication failed for provider {}", provider),
            ),
            Self::SessionExpired { provider } => RuntimeErrorPayload::new(
                RuntimeErrorDomain::Auth,
                "auth.session_expired",
                true,
                format!("Session expired for provider {}", provider),
            ),
            Self::SessionRenewalFailed { provider, reason } => {
                if is_mcp_related(provider) || is_mcp_related(reason) {
                    RuntimeErrorPayload::new(
                        RuntimeErrorDomain::Mcp,
                        "mcp.session_renewal_failed",
                        true,
                        self.to_string(),
                    )
                } else {
                    RuntimeErrorPayload::new(
                        RuntimeErrorDomain::Auth,
                        "auth.session_renewal_failed",
                        true,
                        self.to_string(),
                    )
                }
            }
            Self::RequestFailed { provider, reason } => {
                if is_mcp_related(provider) || is_mcp_related(reason) {
                    RuntimeErrorPayload::new(
                        RuntimeErrorDomain::Mcp,
                        "mcp.request_failed",
                        true,
                        self.to_string(),
                    )
                } else {
                    RuntimeErrorPayload::new(
                        RuntimeErrorDomain::Channel,
                        "channel.request_failed",
                        true,
                        self.to_string(),
                    )
                }
            }
            Self::RateLimited { provider, .. } => {
                if is_mcp_related(provider) {
                    RuntimeErrorPayload::new(
                        RuntimeErrorDomain::Mcp,
                        "mcp.rate_limited",
                        true,
                        self.to_string(),
                    )
                } else {
                    RuntimeErrorPayload::new(
                        RuntimeErrorDomain::Channel,
                        "channel.rate_limited",
                        true,
                        self.to_string(),
                    )
                }
            }
            Self::InvalidResponse { provider, reason } => {
                if is_mcp_related(provider) || is_mcp_related(reason) {
                    RuntimeErrorPayload::new(
                        RuntimeErrorDomain::Mcp,
                        "mcp.invalid_response",
                        false,
                        self.to_string(),
                    )
                } else {
                    RuntimeErrorPayload::new(
                        RuntimeErrorDomain::Verification,
                        "verification.invalid_response",
                        false,
                        self.to_string(),
                    )
                }
            }
            Self::ContextLengthExceeded { .. } => RuntimeErrorPayload::new(
                RuntimeErrorDomain::Verification,
                "verification.context_length_exceeded",
                false,
                self.to_string(),
            ),
            Self::ModelNotAvailable { .. } => RuntimeErrorPayload::new(
                RuntimeErrorDomain::Verification,
                "verification.model_not_available",
                false,
                self.to_string(),
            ),
            Self::Http(_) => RuntimeErrorPayload::new(
                RuntimeErrorDomain::Channel,
                "channel.http_error",
                true,
                self.to_string(),
            ),
            Self::Json(_) => RuntimeErrorPayload::new(
                RuntimeErrorDomain::Verification,
                "verification.json_error",
                false,
                self.to_string(),
            ),
            Self::Io(_) => RuntimeErrorPayload::new(
                RuntimeErrorDomain::Channel,
                "channel.io_error",
                true,
                self.to_string(),
            ),
        }
    }
}

impl Error {
    /// Map runtime errors into a structured surface for status/event consumers.
    pub fn to_runtime_error_payload(&self) -> RuntimeErrorPayload {
        match self {
            Self::Channel(err) => match err {
                ChannelError::AuthFailed { .. } => RuntimeErrorPayload::new(
                    RuntimeErrorDomain::Auth,
                    "auth.channel_failed",
                    false,
                    err.to_string(),
                ),
                ChannelError::StartupFailed { .. } => RuntimeErrorPayload::new(
                    RuntimeErrorDomain::Channel,
                    "channel.startup_failed",
                    false,
                    err.to_string(),
                ),
                ChannelError::Disconnected { .. } => RuntimeErrorPayload::new(
                    RuntimeErrorDomain::Channel,
                    "channel.disconnected",
                    true,
                    err.to_string(),
                ),
                ChannelError::SendFailed { .. } => RuntimeErrorPayload::new(
                    RuntimeErrorDomain::Channel,
                    "channel.send_failed",
                    true,
                    err.to_string(),
                ),
                ChannelError::InvalidMessage(_) => RuntimeErrorPayload::new(
                    RuntimeErrorDomain::Channel,
                    "channel.invalid_message",
                    false,
                    err.to_string(),
                ),
                ChannelError::RateLimited { .. } => RuntimeErrorPayload::new(
                    RuntimeErrorDomain::Channel,
                    "channel.rate_limited",
                    true,
                    err.to_string(),
                ),
                ChannelError::Http(_) => RuntimeErrorPayload::new(
                    RuntimeErrorDomain::Channel,
                    "channel.http_error",
                    true,
                    err.to_string(),
                ),
                ChannelError::HealthCheckFailed { .. } => RuntimeErrorPayload::new(
                    RuntimeErrorDomain::Channel,
                    "channel.health_check_failed",
                    true,
                    err.to_string(),
                ),
            },
            Self::Llm(err) => err.to_runtime_error_payload(),
            Self::Tool(err) => match err {
                ToolError::AuthRequired { name } => {
                    let code = if is_mcp_related(name) {
                        "auth.mcp_required"
                    } else {
                        "auth.tool_required"
                    };
                    RuntimeErrorPayload::new(RuntimeErrorDomain::Auth, code, false, err.to_string())
                }
                ToolError::NotFound { name } => {
                    if is_mcp_related(name) {
                        RuntimeErrorPayload::new(
                            RuntimeErrorDomain::Mcp,
                            "mcp.tool_not_found",
                            false,
                            err.to_string(),
                        )
                    } else {
                        RuntimeErrorPayload::new(
                            RuntimeErrorDomain::Unknown,
                            "runtime.tool_not_found",
                            false,
                            err.to_string(),
                        )
                    }
                }
                ToolError::ExecutionFailed { name, reason } => {
                    if is_mcp_related(name) || is_mcp_related(reason) {
                        RuntimeErrorPayload::new(
                            RuntimeErrorDomain::Mcp,
                            "mcp.execution_failed",
                            true,
                            err.to_string(),
                        )
                    } else {
                        RuntimeErrorPayload::new(
                            RuntimeErrorDomain::Unknown,
                            "runtime.tool_execution_failed",
                            true,
                            err.to_string(),
                        )
                    }
                }
                ToolError::InvalidParameters { name, reason } => {
                    if is_mcp_related(name) || is_mcp_related(reason) {
                        RuntimeErrorPayload::new(
                            RuntimeErrorDomain::Mcp,
                            "mcp.invalid_parameters",
                            false,
                            err.to_string(),
                        )
                    } else {
                        RuntimeErrorPayload::new(
                            RuntimeErrorDomain::Verification,
                            "verification.invalid_parameters",
                            false,
                            err.to_string(),
                        )
                    }
                }
                ToolError::Disabled { name, reason } => {
                    if is_mcp_related(name) || is_mcp_related(reason) {
                        RuntimeErrorPayload::new(
                            RuntimeErrorDomain::Mcp,
                            "mcp.disabled",
                            false,
                            err.to_string(),
                        )
                    } else {
                        RuntimeErrorPayload::new(
                            RuntimeErrorDomain::Unknown,
                            "runtime.tool_disabled",
                            false,
                            err.to_string(),
                        )
                    }
                }
                ToolError::Timeout { name, .. } => {
                    if is_mcp_related(name) {
                        RuntimeErrorPayload::new(
                            RuntimeErrorDomain::Mcp,
                            "mcp.timeout",
                            true,
                            err.to_string(),
                        )
                    } else {
                        RuntimeErrorPayload::new(
                            RuntimeErrorDomain::Unknown,
                            "runtime.tool_timeout",
                            true,
                            err.to_string(),
                        )
                    }
                }
                ToolError::Sandbox { .. } | ToolError::BuilderFailed(_) => {
                    RuntimeErrorPayload::new(
                        RuntimeErrorDomain::Unknown,
                        "runtime.tool_runtime_error",
                        false,
                        err.to_string(),
                    )
                }
            },
            Self::Safety(err) => RuntimeErrorPayload::new(
                RuntimeErrorDomain::Verification,
                "verification.safety_failed",
                false,
                err.to_string(),
            ),
            Self::Evaluation(err) => RuntimeErrorPayload::new(
                RuntimeErrorDomain::Verification,
                "verification.evaluation_failed",
                false,
                err.to_string(),
            ),
            _ => RuntimeErrorPayload::new(
                RuntimeErrorDomain::Unknown,
                "runtime.unclassified",
                false,
                self.to_string(),
            ),
        }
    }

    /// Build a failed runtime status payload from this error.
    pub fn to_runtime_status_payload(&self, stage: RuntimeStage) -> RuntimeStatusPayload {
        RuntimeStatusPayload::new(stage, RuntimeStageState::Failed)
            .with_detail(self.to_string())
            .with_error(self.to_runtime_error_payload())
    }
}

/// Result type alias for the agent.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_auth_errors_for_llm_sessions() {
        let err = Error::from(LlmError::SessionExpired {
            provider: "nearai".to_string(),
        });
        let payload = err.to_runtime_error_payload();

        assert_eq!(payload.domain, RuntimeErrorDomain::Auth);
        assert_eq!(payload.code, "auth.session_expired");
        assert!(payload.retryable);
    }

    #[test]
    fn maps_channel_errors() {
        let err = Error::from(ChannelError::Disconnected {
            name: "repl".to_string(),
            reason: "socket closed".to_string(),
        });
        let payload = err.to_runtime_error_payload();

        assert_eq!(payload.domain, RuntimeErrorDomain::Channel);
        assert_eq!(payload.code, "channel.disconnected");
        assert!(payload.retryable);
    }

    #[test]
    fn maps_mcp_tool_failures() {
        let err = Error::from(ToolError::ExecutionFailed {
            name: "mcp_notion_query".to_string(),
            reason: "upstream timeout".to_string(),
        });
        let payload = err.to_runtime_error_payload();

        assert_eq!(payload.domain, RuntimeErrorDomain::Mcp);
        assert_eq!(payload.code, "mcp.execution_failed");
    }

    #[test]
    fn maps_verification_failures() {
        let err = Error::from(SafetyError::ValidationFailed {
            reason: "position size exceeds policy".to_string(),
        });
        let payload = err.to_runtime_error_payload();

        assert_eq!(payload.domain, RuntimeErrorDomain::Verification);
        assert_eq!(payload.code, "verification.safety_failed");
        assert!(!payload.retryable);
    }

    #[test]
    fn serializes_runtime_status_payload() {
        let payload =
            RuntimeStatusPayload::new(RuntimeStage::Execution, RuntimeStageState::Started)
                .with_intent("trade.execute")
                .with_detail("dispatching tools");
        let encoded = payload.to_status_line();
        let value: serde_json::Value = serde_json::from_str(&encoded).expect("valid json");

        assert_eq!(value["kind"], "ws2_runtime_status");
        assert_eq!(value["stage"], "execution");
        assert_eq!(value["state"], "started");
        assert_eq!(value["intent"], "trade.execute");
    }
}
