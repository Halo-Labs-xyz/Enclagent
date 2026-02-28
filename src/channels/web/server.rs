//! Axum HTTP server for the web gateway.
//!
//! Handles all API routes: chat, memory, jobs, health, and static file serving.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, OriginalUri, Path, Query, State, WebSocketUpgrade},
    http::{HeaderValue, StatusCode, header},
    middleware,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::StreamExt;
use tower_http::cors::{AllowHeaders, CorsLayer};
use uuid::Uuid;

use crate::agent::SessionManager;
use crate::channels::IncomingMessage;
use crate::channels::web::auth::{AuthState, auth_middleware};
use crate::channels::web::log_layer::LogBroadcaster;
use crate::channels::web::sse::SseManager;
use crate::channels::web::types::*;
use crate::db::Database;
use crate::extensions::ExtensionManager;
use crate::orchestrator::job_manager::ContainerJobManager;
use crate::tools::ToolRegistry;
use crate::workspace::Workspace;

/// Shared prompt queue: maps job IDs to pending follow-up prompts for Claude Code bridges.
pub type PromptQueue = Arc<
    tokio::sync::Mutex<
        std::collections::HashMap<
            uuid::Uuid,
            std::collections::VecDeque<crate::orchestrator::api::PendingPrompt>,
        >,
    >,
>;

static WEB_STATIC_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/src/channels/web/static");
const WEB_STATIC_FS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/channels/web/static");

/// Simple sliding-window rate limiter.
///
/// Tracks the number of requests in the current window. Resets when the window expires.
/// Not per-IP (since this is a single-user gateway with auth), but prevents flooding.
pub struct RateLimiter {
    /// Requests remaining in the current window.
    remaining: AtomicU64,
    /// Epoch second when the current window started.
    window_start: AtomicU64,
    /// Maximum requests per window.
    max_requests: u64,
    /// Window duration in seconds.
    window_secs: u64,
}

impl RateLimiter {
    pub fn new(max_requests: u64, window_secs: u64) -> Self {
        Self {
            remaining: AtomicU64::new(max_requests),
            window_start: AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            ),
            max_requests,
            window_secs,
        }
    }

    /// Try to consume one request. Returns `true` if allowed, `false` if rate limited.
    pub fn check(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let window = self.window_start.load(Ordering::Relaxed);
        if now.saturating_sub(window) >= self.window_secs {
            // Window expired, reset
            self.window_start.store(now, Ordering::Relaxed);
            self.remaining
                .store(self.max_requests - 1, Ordering::Relaxed);
            return true;
        }

        // Try to decrement remaining
        loop {
            let current = self.remaining.load(Ordering::Relaxed);
            if current == 0 {
                return false;
            }
            if self
                .remaining
                .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }
}

/// Shared state for all gateway handlers.
pub struct GatewayState {
    /// Channel to send messages to the agent loop.
    pub msg_tx: tokio::sync::RwLock<Option<mpsc::Sender<IncomingMessage>>>,
    /// SSE broadcast manager.
    pub sse: SseManager,
    /// Workspace for memory API.
    pub workspace: Option<Arc<Workspace>>,
    /// Session manager for thread info.
    pub session_manager: Option<Arc<SessionManager>>,
    /// Log broadcaster for the logs SSE endpoint.
    pub log_broadcaster: Option<Arc<LogBroadcaster>>,
    /// Extension manager for extension management API.
    pub extension_manager: Option<Arc<ExtensionManager>>,
    /// Tool registry for listing registered tools.
    pub tool_registry: Option<Arc<ToolRegistry>>,
    /// Database store for sandbox job persistence.
    pub store: Option<Arc<dyn Database>>,
    /// Container job manager for sandbox operations.
    pub job_manager: Option<Arc<ContainerJobManager>>,
    /// Prompt queue for Claude Code follow-up prompts.
    pub prompt_queue: Option<PromptQueue>,
    /// User ID for this gateway.
    pub user_id: String,
    /// Shutdown signal sender.
    pub shutdown_tx: tokio::sync::RwLock<Option<oneshot::Sender<()>>>,
    /// WebSocket connection tracker.
    pub ws_tracker: Option<Arc<crate::channels::web::ws::WsConnectionTracker>>,
    /// LLM provider for OpenAI-compatible API proxy.
    pub llm_provider: Option<Arc<dyn crate::llm::LlmProvider>>,
    /// Skill registry for skill management API.
    pub skill_registry: Option<Arc<std::sync::RwLock<crate::skills::SkillRegistry>>>,
    /// Skill catalog for searching the ClawHub registry.
    pub skill_catalog: Option<Arc<crate::skills::catalog::SkillCatalog>>,
    /// Frontdoor wallet-auth/session provisioning flow (user-facing mode).
    pub frontdoor: Option<Arc<crate::channels::web::frontdoor::FrontdoorService>>,
    /// Rate limiter for chat endpoints (30 messages per 60 seconds).
    pub chat_rate_limiter: RateLimiter,
}

/// Start the gateway HTTP server.
///
/// Returns the actual bound `SocketAddr` (useful when binding to port 0).
pub async fn start_server(
    addr: SocketAddr,
    state: Arc<GatewayState>,
    auth_token: String,
) -> Result<SocketAddr, crate::error::ChannelError> {
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        crate::error::ChannelError::StartupFailed {
            name: "gateway".to_string(),
            reason: format!("Failed to bind to {}: {}", addr, e),
        }
    })?;
    let bound_addr =
        listener
            .local_addr()
            .map_err(|e| crate::error::ChannelError::StartupFailed {
                name: "gateway".to_string(),
                reason: format!("Failed to get local addr: {}", e),
            })?;

    let auth_state = AuthState { token: auth_token };

    // Public routes (no auth)
    let public = Router::new()
        .route("/api/health", get(health_handler))
        .route(
            "/api/frontdoor/config-contract",
            get(frontdoor_config_contract_handler),
        )
        .route(
            "/api/frontdoor/policy-templates",
            get(frontdoor_policy_template_library_handler),
        )
        .route(
            "/api/frontdoor/experience/manifest",
            get(frontdoor_experience_manifest_handler),
        )
        .route("/api/frontdoor/bootstrap", get(frontdoor_bootstrap_handler))
        .route(
            "/api/frontdoor/onboarding/state",
            get(frontdoor_onboarding_state_handler),
        )
        .route(
            "/api/frontdoor/onboarding/chat",
            post(frontdoor_onboarding_chat_handler),
        )
        .route(
            "/api/frontdoor/challenge",
            post(frontdoor_challenge_handler),
        )
        .route(
            "/api/frontdoor/suggest-config",
            post(frontdoor_suggest_config_handler),
        )
        .route("/api/frontdoor/verify", post(frontdoor_verify_handler))
        .route(
            "/api/frontdoor/session/{session_id}",
            get(frontdoor_session_handler),
        )
        .route(
            "/api/frontdoor/session/{session_id}/timeline",
            get(frontdoor_session_timeline_handler),
        )
        .route(
            "/api/frontdoor/session/{session_id}/verification-explanation",
            get(frontdoor_verification_explanation_handler),
        )
        .route(
            "/api/frontdoor/session/{session_id}/gateway-todos",
            get(frontdoor_gateway_todos_handler),
        )
        .route(
            "/api/frontdoor/session/{session_id}/funding-preflight",
            get(frontdoor_funding_preflight_handler),
        )
        .route("/api/frontdoor/sessions", get(frontdoor_sessions_handler));

    // Frontdoor operator and control routes are always auth-protected.
    let frontdoor_operator = Router::new()
        .route(
            "/api/frontdoor/session/{session_id}/runtime-control",
            post(frontdoor_runtime_control_handler),
        )
        .route(
            "/api/frontdoor/operator/sessions",
            get(frontdoor_operator_sessions_handler),
        )
        .route_layer(middleware::from_fn_with_state(
            auth_state.clone(),
            auth_middleware,
        ));

    // Protected routes (always require auth)
    let protected = Router::new()
        // Chat
        .route("/api/chat/send", post(chat_send_handler))
        .route("/api/chat/approval", post(chat_approval_handler))
        .route("/api/chat/auth-token", post(chat_auth_token_handler))
        .route("/api/chat/auth-cancel", post(chat_auth_cancel_handler))
        .route("/api/chat/events", get(chat_events_handler))
        .route("/api/chat/ws", get(chat_ws_handler))
        .route("/api/chat/history", get(chat_history_handler))
        .route("/api/chat/threads", get(chat_threads_handler))
        .route("/api/chat/thread/new", post(chat_new_thread_handler))
        // Memory
        .route("/api/memory/tree", get(memory_tree_handler))
        .route("/api/memory/list", get(memory_list_handler))
        .route("/api/memory/read", get(memory_read_handler))
        .route("/api/memory/write", post(memory_write_handler))
        .route("/api/memory/search", post(memory_search_handler))
        // Jobs
        .route("/api/jobs", get(jobs_list_handler))
        .route("/api/jobs/summary", get(jobs_summary_handler))
        .route("/api/jobs/{id}", get(jobs_detail_handler))
        .route("/api/jobs/{id}/cancel", post(jobs_cancel_handler))
        .route("/api/jobs/{id}/restart", post(jobs_restart_handler))
        .route("/api/jobs/{id}/prompt", post(jobs_prompt_handler))
        .route("/api/jobs/{id}/events", get(jobs_events_handler))
        .route("/api/jobs/{id}/files/list", get(job_files_list_handler))
        .route("/api/jobs/{id}/files/read", get(job_files_read_handler))
        // Logs
        .route("/api/logs/events", get(logs_events_handler))
        // Extensions
        .route("/api/extensions", get(extensions_list_handler))
        .route("/api/extensions/tools", get(extensions_tools_handler))
        .route("/api/extensions/install", post(extensions_install_handler))
        .route(
            "/api/extensions/{name}/activate",
            post(extensions_activate_handler),
        )
        .route(
            "/api/extensions/{name}/remove",
            post(extensions_remove_handler),
        )
        // Routines
        .route("/api/routines", get(routines_list_handler))
        .route("/api/routines/summary", get(routines_summary_handler))
        .route("/api/routines/visibility", get(routines_visibility_handler))
        .route("/api/routines/{id}", get(routines_detail_handler))
        .route("/api/routines/{id}/trigger", post(routines_trigger_handler))
        .route("/api/routines/{id}/toggle", post(routines_toggle_handler))
        .route(
            "/api/routines/{id}",
            axum::routing::delete(routines_delete_handler),
        )
        .route("/api/routines/{id}/runs", get(routines_runs_handler))
        .route("/api/gateway/todos", get(gateway_todos_handler))
        // Skills
        .route("/api/skills", get(skills_list_handler))
        .route("/api/skills/search", post(skills_search_handler))
        .route("/api/skills/install", post(skills_install_handler))
        .route(
            "/api/skills/{name}",
            axum::routing::delete(skills_remove_handler),
        )
        // Settings
        .route("/api/settings", get(settings_list_handler))
        .route("/api/settings/export", get(settings_export_handler))
        .route("/api/settings/import", post(settings_import_handler))
        .route("/api/settings/{key}", get(settings_get_handler))
        .route(
            "/api/settings/{key}",
            axum::routing::put(settings_set_handler),
        )
        .route(
            "/api/settings/{key}",
            axum::routing::delete(settings_delete_handler),
        )
        // Modules
        .route("/api/modules/catalog", get(modules_catalog_handler))
        .route("/api/modules/state", get(modules_state_handler))
        .route(
            "/api/modules/{module_id}/enable",
            post(modules_enable_handler),
        )
        .route(
            "/api/modules/{module_id}/disable",
            post(modules_disable_handler),
        )
        .route(
            "/api/modules/{module_id}/health",
            get(modules_health_handler),
        )
        .route(
            "/api/modules/{module_id}/config",
            axum::routing::put(modules_config_handler),
        )
        // Org
        .route("/api/org/current", get(org_current_handler))
        .route("/api/org/members", get(org_members_handler))
        .route("/api/org/members/invite", post(org_members_invite_handler))
        .route(
            "/api/org/members/{member_id}/role",
            axum::routing::put(org_members_role_handler),
        )
        .route(
            "/api/org/members/{member_id}",
            axum::routing::delete(org_members_delete_handler),
        )
        // Gateway control plane
        .route("/api/status/channels", get(status_channels_handler))
        .route("/api/status/verification", get(status_verification_handler))
        .route("/api/gateway/status", get(gateway_status_handler))
        // OpenAI-compatible API
        .route(
            "/v1/chat/completions",
            post(super::openai_compat::chat_completions_handler),
        )
        .route("/v1/models", get(super::openai_compat::models_handler))
        .route_layer(middleware::from_fn_with_state(
            auth_state.clone(),
            auth_middleware,
        ));

    // Static file routes (no auth, served from embedded strings)
    let statics = Router::new()
        .route("/", get(index_handler))
        .route("/gateway", get(legacy_gateway_redirect_handler))
        .route("/frontdoor", get(frontdoor_handler))
        .route("/launchpad", get(frontdoor_handler))
        .route("/favicon.ico", get(favicon_handler))
        .route("/launchpad.css", get(launchpad_css_handler))
        .route("/launchpad.js", get(launchpad_js_handler))
        .route("/launchpad-{asset}", get(launchpad_chunk_handler))
        .route("/style.css", get(css_handler))
        .route("/app.js", get(js_handler))
        .route("/frontdoor.css", get(frontdoor_css_handler))
        .route("/frontdoor.js", get(frontdoor_js_handler));

    // Project file serving (behind auth to prevent unauthorized file access).
    let projects = Router::new()
        .route("/projects/{project_id}", get(project_redirect_handler))
        .route("/projects/{project_id}/", get(project_index_handler))
        .route("/projects/{project_id}/{*path}", get(project_file_handler));

    let projects = projects.route_layer(middleware::from_fn_with_state(
        auth_state.clone(),
        auth_middleware,
    ));

    // CORS: restrict to same-origin by default. Only localhost/127.0.0.1
    // origins are allowed, since the gateway is a local-first service.
    let cors = CorsLayer::new()
        .allow_origin([
            format!("http://{}:{}", addr.ip(), addr.port())
                .parse()
                .expect("valid origin"),
            format!("http://localhost:{}", addr.port())
                .parse()
                .expect("valid origin"),
        ])
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
        ])
        .allow_headers(AllowHeaders::list([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
        ]))
        .allow_credentials(true);

    let app = Router::new()
        .merge(public)
        .merge(frontdoor_operator)
        .merge(statics)
        .merge(projects)
        .merge(protected)
        .layer(cors)
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1 MB max request body
        .with_state(state.clone());

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    *state.shutdown_tx.write().await = Some(shutdown_tx);

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
                tracing::info!("Web gateway shutting down");
            })
            .await
        {
            tracing::error!("Web gateway server error: {}", e);
        }
    });

    Ok(bound_addr)
}

// --- Static file handlers ---

async fn index_handler() -> impl IntoResponse {
    launchpad_static_asset_response("index.html")
}

async fn frontdoor_handler() -> impl IntoResponse {
    launchpad_static_asset_response("launchpad.html")
}

async fn launchpad_css_handler() -> impl IntoResponse {
    launchpad_static_asset_response("launchpad.css")
}

async fn launchpad_js_handler() -> impl IntoResponse {
    launchpad_static_asset_response("launchpad.js")
}

async fn launchpad_chunk_handler(Path(asset): Path<String>) -> impl IntoResponse {
    let filename = format!("launchpad-{asset}");
    if !filename.ends_with(".js") && !filename.ends_with(".css") && !filename.ends_with(".map") {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }
    launchpad_static_asset_response(&filename)
}

fn launchpad_static_asset_response(filename: &str) -> Response {
    if filename.contains("..") {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }
    if let Some(contents) = read_static_asset_from_fs(filename) {
        return static_asset_bytes_response(filename, contents);
    }
    if let Some(file) = WEB_STATIC_DIR.get_file(filename) {
        return static_asset_bytes_response(filename, file.contents().to_vec());
    }
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

fn read_static_asset_from_fs(filename: &str) -> Option<Vec<u8>> {
    let mut path = PathBuf::from(WEB_STATIC_FS_ROOT);
    path.push(filename);
    std::fs::read(path).ok()
}

fn static_asset_bytes_response(filename: &str, contents: Vec<u8>) -> Response {
    let content_type = mime_guess::from_path(filename)
        .first_or_octet_stream()
        .essence_str()
        .to_string();
    let mut response = Response::new(Body::from(contents));
    let content_type_header = HeaderValue::from_str(&content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type_header);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

async fn favicon_handler() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

async fn legacy_gateway_redirect_handler(original_uri: OriginalUri) -> impl IntoResponse {
    let redirect = match original_uri.0.query() {
        Some(query) if !query.is_empty() => format!("/?{query}"),
        _ => "/".to_string(),
    };
    (
        StatusCode::TEMPORARY_REDIRECT,
        [(header::LOCATION, redirect)],
    )
}

async fn css_handler() -> impl IntoResponse {
    launchpad_static_asset_response("style.css")
}

async fn js_handler() -> impl IntoResponse {
    launchpad_static_asset_response("app.js")
}

async fn frontdoor_css_handler() -> impl IntoResponse {
    launchpad_static_asset_response("frontdoor.css")
}

async fn frontdoor_js_handler() -> impl IntoResponse {
    launchpad_static_asset_response("frontdoor.js")
}

// --- Health ---

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy",
        channel: "gateway",
    })
}

// --- Frontdoor handlers ---

async fn frontdoor_bootstrap_handler(
    State(state): State<Arc<GatewayState>>,
) -> Json<FrontdoorBootstrapResponse> {
    if let Some(frontdoor) = &state.frontdoor {
        Json(frontdoor.bootstrap())
    } else {
        Json(FrontdoorBootstrapResponse {
            enabled: false,
            require_privy: false,
            privy_app_id: None,
            privy_client_id: None,
            provisioning_backend: "unconfigured".to_string(),
            dynamic_provisioning_enabled: false,
            default_instance_url_configured: false,
            default_instance_fallback_enabled: false,
            default_instance_looks_eigencloud: false,
            poll_interval_ms: 1500,
            mandatory_steps: Vec::new(),
        })
    }
}

async fn frontdoor_config_contract_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<FrontdoorConfigContractResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    Ok(Json(frontdoor.config_contract()))
}

async fn frontdoor_policy_template_library_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<FrontdoorPolicyTemplateLibraryResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    Ok(Json(frontdoor.policy_template_library()))
}

async fn frontdoor_experience_manifest_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<FrontdoorExperienceManifestResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    Ok(Json(frontdoor.experience_manifest()))
}

async fn frontdoor_onboarding_state_handler(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<FrontdoorOnboardingStateQuery>,
) -> Result<Json<FrontdoorOnboardingStateResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    let session_id = Uuid::parse_str(query.session_id.trim())
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid session id".to_string()))?;
    frontdoor
        .onboarding_state(session_id)
        .await
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "session not found".to_string()))
}

async fn frontdoor_onboarding_chat_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<FrontdoorOnboardingChatRequest>,
) -> Result<Json<FrontdoorOnboardingChatResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    frontdoor
        .onboarding_chat(req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn frontdoor_challenge_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<FrontdoorChallengeRequest>,
) -> Result<Json<FrontdoorChallengeResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    frontdoor
        .create_challenge(req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn frontdoor_suggest_config_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<FrontdoorSuggestConfigRequest>,
) -> Result<Json<FrontdoorSuggestConfigResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    frontdoor
        .suggest_config(req)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn frontdoor_verify_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<FrontdoorVerifyRequest>,
) -> Result<Json<FrontdoorVerifyResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    frontdoor
        .clone()
        .verify_and_start(req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn frontdoor_session_handler(
    State(state): State<Arc<GatewayState>>,
    Path(session_id): Path<String>,
) -> Result<Json<FrontdoorSessionResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    let session_id = Uuid::parse_str(session_id.trim())
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid session id".to_string()))?;
    frontdoor
        .get_session(session_id)
        .await
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "session not found".to_string()))
}

async fn frontdoor_session_timeline_handler(
    State(state): State<Arc<GatewayState>>,
    Path(session_id): Path<String>,
) -> Result<Json<FrontdoorSessionTimelineResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    let session_id = Uuid::parse_str(session_id.trim())
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid session id".to_string()))?;
    frontdoor
        .session_timeline(session_id)
        .await
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "session not found".to_string()))
}

async fn frontdoor_verification_explanation_handler(
    State(state): State<Arc<GatewayState>>,
    Path(session_id): Path<String>,
) -> Result<Json<FrontdoorVerificationExplanationResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    let session_id = Uuid::parse_str(session_id.trim())
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid session id".to_string()))?;
    frontdoor
        .verification_explanation(session_id)
        .await
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "session not found".to_string()))
}

async fn frontdoor_runtime_control_handler(
    State(state): State<Arc<GatewayState>>,
    Path(session_id): Path<String>,
    Json(req): Json<FrontdoorRuntimeControlRequest>,
) -> Result<Json<FrontdoorRuntimeControlResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    let session_id = Uuid::parse_str(session_id.trim())
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid session id".to_string()))?;
    frontdoor
        .runtime_control(session_id, req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn frontdoor_gateway_todos_handler(
    State(state): State<Arc<GatewayState>>,
    Path(session_id): Path<String>,
) -> Result<Json<FrontdoorGatewayTodosResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    let session_id = Uuid::parse_str(session_id.trim())
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid session id".to_string()))?;
    frontdoor
        .gateway_todos_for_session(session_id)
        .await
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "session not found".to_string()))
}

async fn frontdoor_funding_preflight_handler(
    State(state): State<Arc<GatewayState>>,
    Path(session_id): Path<String>,
) -> Result<Json<FrontdoorFundingPreflightResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    let session_id = Uuid::parse_str(session_id.trim())
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid session id".to_string()))?;
    frontdoor
        .funding_preflight(session_id)
        .await
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "session not found".to_string()))
}

async fn frontdoor_sessions_handler(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<FrontdoorSessionListQuery>,
) -> Result<Json<FrontdoorSessionMonitorResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    let wallet_address = query.wallet_address.ok_or((
        StatusCode::BAD_REQUEST,
        "wallet_address query parameter is required".to_string(),
    ))?;
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let (total, sessions) = frontdoor
        .list_sessions(Some(wallet_address.as_str()), limit)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(FrontdoorSessionMonitorResponse {
        generated_at: chrono::Utc::now().to_rfc3339(),
        wallet_address,
        limit,
        total,
        sessions,
    }))
}

async fn frontdoor_operator_sessions_handler(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<FrontdoorOperatorSessionListQuery>,
) -> Result<Json<FrontdoorOperatorSessionMonitorResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let (total, sessions) = frontdoor
        .list_sessions_full(query.wallet_address.as_deref(), limit)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(FrontdoorOperatorSessionMonitorResponse {
        generated_at: chrono::Utc::now().to_rfc3339(),
        wallet_address: query.wallet_address,
        limit,
        total,
        sessions,
    }))
}

async fn gateway_todos_handler(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<GatewayTodoListQuery>,
) -> Result<Json<GatewayTodoListResponse>, (StatusCode, String)> {
    let frontdoor = state.frontdoor.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Frontdoor provisioning is not enabled".to_string(),
    ))?;
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let session_id = match query.session_id.as_deref() {
        Some(raw) => Some(
            Uuid::parse_str(raw.trim())
                .map_err(|_| (StatusCode::BAD_REQUEST, "invalid session id".to_string()))?,
        ),
        None => None,
    };
    let (total, sessions) = frontdoor
        .gateway_todos(query.wallet_address.as_deref(), session_id, limit)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(GatewayTodoListResponse {
        generated_at: chrono::Utc::now().to_rfc3339(),
        total,
        sessions,
    }))
}

// --- Chat handlers ---

async fn chat_send_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<SendMessageResponse>), (StatusCode, String)> {
    if !state.chat_rate_limiter.check() {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded. Try again shortly.".to_string(),
        ));
    }

    let module_states = load_module_state(&state).await;
    let route_resolution = crate::platform::resolve_inference_route(&req.content, &module_states);
    if !route_resolution.allowed {
        return Err((
            StatusCode::FORBIDDEN,
            format!("Blocked by module policy: {}", route_resolution.reason),
        ));
    }

    let effective_module_id = route_resolution.decision.module_id.clone();
    let enabled_modules: Vec<String> = module_states
        .iter()
        .filter(|module| module.enabled)
        .map(|module| module.module_id.clone())
        .collect();

    let route_decision_json = serde_json::to_value(&route_resolution.decision).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize route decision: {}", e),
        )
    })?;
    let route_resolution_json = serde_json::to_value(&route_resolution).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize route resolution: {}", e),
        )
    })?;

    let mut metadata = serde_json::Map::new();
    if let Some(ref thread_id) = req.thread_id {
        metadata.insert(
            "thread_id".to_string(),
            serde_json::Value::String(thread_id.clone()),
        );
    }
    metadata.insert("inference_route".to_string(), route_decision_json);
    metadata.insert(
        "inference_route_resolution".to_string(),
        route_resolution_json,
    );
    metadata.insert(
        "module_capabilities".to_string(),
        serde_json::json!(crate::platform::module_capability_keys(
            &effective_module_id
        )),
    );
    metadata.insert(
        "enabled_modules".to_string(),
        serde_json::json!(enabled_modules),
    );
    metadata.insert(
        "effective_module".to_string(),
        serde_json::Value::String(effective_module_id),
    );

    let mut msg = IncomingMessage::new("gateway", &state.user_id, &req.content)
        .with_metadata(serde_json::Value::Object(metadata));
    if let Some(ref thread_id) = req.thread_id {
        msg = msg.with_thread(thread_id);
    }

    let msg_id = msg.id;

    let tx_guard = state.msg_tx.read().await;
    let tx = tx_guard.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Channel not started".to_string(),
    ))?;

    tx.send(msg).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Channel closed".to_string(),
        )
    })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(SendMessageResponse {
            message_id: msg_id,
            status: "accepted",
        }),
    ))
}

async fn chat_approval_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<ApprovalRequest>,
) -> Result<(StatusCode, Json<SendMessageResponse>), (StatusCode, String)> {
    let (approved, always) = match req.action.as_str() {
        "approve" => (true, false),
        "always" => (true, true),
        "deny" => (false, false),
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Unknown action: {}", other),
            ));
        }
    };

    let request_id = Uuid::parse_str(&req.request_id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid request_id (expected UUID)".to_string(),
        )
    })?;

    // Build a structured ExecApproval submission as JSON, sent through the
    // existing message pipeline so the agent loop picks it up.
    let approval = crate::agent::submission::Submission::ExecApproval {
        request_id,
        approved,
        always,
    };
    let content = serde_json::to_string(&approval).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize approval: {}", e),
        )
    })?;

    let mut msg = IncomingMessage::new("gateway", &state.user_id, content);

    if let Some(ref thread_id) = req.thread_id {
        msg = msg.with_thread(thread_id);
    }

    let msg_id = msg.id;

    let tx_guard = state.msg_tx.read().await;
    let tx = tx_guard.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Channel not started".to_string(),
    ))?;

    tx.send(msg).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Channel closed".to_string(),
        )
    })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(SendMessageResponse {
            message_id: msg_id,
            status: "accepted",
        }),
    ))
}

/// Submit an auth token directly to the extension manager, bypassing the message pipeline.
///
/// The token never touches the LLM, chat history, or SSE stream.
async fn chat_auth_token_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<AuthTokenRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Extension manager not available".to_string(),
    ))?;

    let result = ext_mgr
        .auth(&req.extension_name, Some(&req.token))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if result.status == "authenticated" {
        // Auto-activate so tools are available immediately
        let msg = match ext_mgr.activate(&req.extension_name).await {
            Ok(r) => format!(
                "{} authenticated ({} tools loaded)",
                req.extension_name,
                r.tools_loaded.len()
            ),
            Err(e) => format!(
                "{} authenticated but activation failed: {}",
                req.extension_name, e
            ),
        };

        // Clear auth mode on the active thread
        clear_auth_mode(&state).await;

        state.sse.broadcast(SseEvent::AuthCompleted {
            extension_name: req.extension_name,
            success: true,
            message: msg.clone(),
        });

        Ok(Json(ActionResponse::ok(msg)))
    } else {
        // Re-emit auth_required for retry
        state.sse.broadcast(SseEvent::AuthRequired {
            extension_name: req.extension_name.clone(),
            instructions: result.instructions.clone(),
            auth_url: result.auth_url.clone(),
            setup_url: result.setup_url.clone(),
        });
        Ok(Json(ActionResponse::fail(
            result
                .instructions
                .unwrap_or_else(|| "Invalid token".to_string()),
        )))
    }
}

/// Cancel an in-progress auth flow.
async fn chat_auth_cancel_handler(
    State(state): State<Arc<GatewayState>>,
    Json(_req): Json<AuthCancelRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    clear_auth_mode(&state).await;
    Ok(Json(ActionResponse::ok("Auth cancelled")))
}

/// Clear pending auth mode on the active thread.
pub async fn clear_auth_mode(state: &GatewayState) {
    if let Some(ref sm) = state.session_manager {
        let session = sm.get_or_create_session(&state.user_id).await;
        let mut sess = session.lock().await;
        if let Some(thread_id) = sess.active_thread
            && let Some(thread) = sess.threads.get_mut(&thread_id)
        {
            thread.pending_auth = None;
        }
    }
}

async fn chat_events_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    state.sse.subscribe().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Too many connections".to_string(),
    ))
}

async fn chat_ws_handler(
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<Arc<GatewayState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Validate Origin header to prevent cross-site WebSocket hijacking.
    // Require the header outright; browsers always send it for WS upgrades,
    // so a missing Origin means a non-browser client trying to bypass the check.
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                "WebSocket Origin header required".to_string(),
            )
        })?;

    // Extract the host from the origin and compare exactly, so that
    // crafted origins like "http://localhost.evil.com" are rejected.
    // Origin format is "scheme://host[:port]".
    let host = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .and_then(|rest| rest.split(':').next()?.split('/').next())
        .unwrap_or("");

    let is_local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    if !is_local {
        return Err((
            StatusCode::FORBIDDEN,
            "WebSocket origin not allowed".to_string(),
        ));
    }
    Ok(ws.on_upgrade(move |socket| crate::channels::web::ws::handle_ws_connection(socket, state)))
}

#[derive(Deserialize)]
struct HistoryQuery {
    thread_id: Option<String>,
    limit: Option<usize>,
    before: Option<String>,
}

async fn chat_history_handler(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, (StatusCode, String)> {
    let session_manager = state.session_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Session manager not available".to_string(),
    ))?;

    let session = session_manager.get_or_create_session(&state.user_id).await;
    let sess = session.lock().await;

    let limit = query.limit.unwrap_or(50);
    let before_cursor = query
        .before
        .as_deref()
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        "Invalid 'before' timestamp".to_string(),
                    )
                })
        })
        .transpose()?;

    // Find the thread
    let thread_id = if let Some(ref tid) = query.thread_id {
        Uuid::parse_str(tid)
            .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid thread_id".to_string()))?
    } else {
        sess.active_thread
            .ok_or((StatusCode::NOT_FOUND, "No active thread".to_string()))?
    };

    // Verify the thread belongs to the authenticated user before returning any data.
    // In-memory threads are already scoped by user via session_manager, but DB
    // lookups could expose another user's conversation if the UUID is guessed.
    if query.thread_id.is_some()
        && let Some(ref store) = state.store
    {
        let owned = store
            .conversation_belongs_to_user(thread_id, &state.user_id)
            .await
            .unwrap_or(false);
        if !owned && !sess.threads.contains_key(&thread_id) {
            return Err((StatusCode::NOT_FOUND, "Thread not found".to_string()));
        }
    }

    // For paginated requests (before cursor set), always go to DB
    if before_cursor.is_some()
        && let Some(ref store) = state.store
    {
        let (messages, has_more) = store
            .list_conversation_messages_paginated(thread_id, before_cursor, limit as i64)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let oldest_timestamp = messages.first().map(|m| m.created_at.to_rfc3339());
        let turns = build_turns_from_db_messages(&messages);
        return Ok(Json(HistoryResponse {
            thread_id,
            turns,
            has_more,
            oldest_timestamp,
        }));
    }

    // Try in-memory first (freshest data for active threads)
    if let Some(thread) = sess.threads.get(&thread_id)
        && !thread.turns.is_empty()
    {
        let turns: Vec<TurnInfo> = thread
            .turns
            .iter()
            .map(|t| TurnInfo {
                turn_number: t.turn_number,
                user_input: t.user_input.clone(),
                response: t.response.clone(),
                state: format!("{:?}", t.state),
                started_at: t.started_at.to_rfc3339(),
                completed_at: t.completed_at.map(|dt| dt.to_rfc3339()),
                tool_calls: t
                    .tool_calls
                    .iter()
                    .map(|tc| ToolCallInfo {
                        name: tc.name.clone(),
                        has_result: tc.result.is_some(),
                        has_error: tc.error.is_some(),
                    })
                    .collect(),
            })
            .collect();

        return Ok(Json(HistoryResponse {
            thread_id,
            turns,
            has_more: false,
            oldest_timestamp: None,
        }));
    }

    // Fall back to DB for historical threads not in memory (paginated)
    if let Some(ref store) = state.store {
        let (messages, has_more) = store
            .list_conversation_messages_paginated(thread_id, None, limit as i64)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if !messages.is_empty() {
            let oldest_timestamp = messages.first().map(|m| m.created_at.to_rfc3339());
            let turns = build_turns_from_db_messages(&messages);
            return Ok(Json(HistoryResponse {
                thread_id,
                turns,
                has_more,
                oldest_timestamp,
            }));
        }
    }

    // Empty thread (just created, no messages yet)
    Ok(Json(HistoryResponse {
        thread_id,
        turns: Vec::new(),
        has_more: false,
        oldest_timestamp: None,
    }))
}

/// Build TurnInfo pairs from flat DB messages (alternating user/assistant).
fn build_turns_from_db_messages(messages: &[crate::history::ConversationMessage]) -> Vec<TurnInfo> {
    let mut turns = Vec::new();
    let mut turn_number = 0;
    let mut iter = messages.iter().peekable();

    while let Some(msg) = iter.next() {
        if msg.role == "user" {
            let mut turn = TurnInfo {
                turn_number,
                user_input: msg.content.clone(),
                response: None,
                state: "Completed".to_string(),
                started_at: msg.created_at.to_rfc3339(),
                completed_at: None,
                tool_calls: Vec::new(),
            };

            // Check if next message is an assistant response
            if let Some(next) = iter.peek()
                && next.role == "assistant"
            {
                let assistant_msg = iter.next().expect("peeked");
                turn.response = Some(assistant_msg.content.clone());
                turn.completed_at = Some(assistant_msg.created_at.to_rfc3339());
            }

            // Incomplete turn (user message without response)
            if turn.response.is_none() {
                turn.state = "Failed".to_string();
            }

            turns.push(turn);
            turn_number += 1;
        }
    }

    turns
}

async fn chat_threads_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<ThreadListResponse>, (StatusCode, String)> {
    let session_manager = state.session_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Session manager not available".to_string(),
    ))?;

    let session = session_manager.get_or_create_session(&state.user_id).await;
    let sess = session.lock().await;

    // Try DB first for persistent thread list
    if let Some(ref store) = state.store {
        // Auto-create assistant thread if it doesn't exist
        let assistant_id = store
            .get_or_create_assistant_conversation(&state.user_id, "gateway")
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if let Ok(summaries) = store
            .list_conversations_with_preview(&state.user_id, "gateway", 50)
            .await
        {
            let mut assistant_thread = None;
            let mut threads = Vec::new();

            for s in &summaries {
                let info = ThreadInfo {
                    id: s.id,
                    state: "Idle".to_string(),
                    turn_count: (s.message_count / 2).max(0) as usize,
                    created_at: s.started_at.to_rfc3339(),
                    updated_at: s.last_activity.to_rfc3339(),
                    title: s.title.clone(),
                    thread_type: s.thread_type.clone(),
                };

                if s.id == assistant_id {
                    assistant_thread = Some(info);
                } else {
                    threads.push(info);
                }
            }

            // If assistant wasn't in the list (0 messages), synthesize it
            if assistant_thread.is_none() {
                assistant_thread = Some(ThreadInfo {
                    id: assistant_id,
                    state: "Idle".to_string(),
                    turn_count: 0,
                    created_at: chrono::Utc::now().to_rfc3339(),
                    updated_at: chrono::Utc::now().to_rfc3339(),
                    title: None,
                    thread_type: Some("assistant".to_string()),
                });
            }

            return Ok(Json(ThreadListResponse {
                assistant_thread,
                threads,
                active_thread: sess.active_thread,
            }));
        }
    }

    // Fallback: in-memory only (no assistant thread without DB)
    let threads: Vec<ThreadInfo> = sess
        .threads
        .values()
        .map(|t| ThreadInfo {
            id: t.id,
            state: format!("{:?}", t.state),
            turn_count: t.turns.len(),
            created_at: t.created_at.to_rfc3339(),
            updated_at: t.updated_at.to_rfc3339(),
            title: None,
            thread_type: None,
        })
        .collect();

    Ok(Json(ThreadListResponse {
        assistant_thread: None,
        threads,
        active_thread: sess.active_thread,
    }))
}

async fn chat_new_thread_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<ThreadInfo>, (StatusCode, String)> {
    let session_manager = state.session_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Session manager not available".to_string(),
    ))?;

    let session = session_manager.get_or_create_session(&state.user_id).await;
    let mut sess = session.lock().await;
    let thread = sess.create_thread();
    let thread_id = thread.id;
    let info = ThreadInfo {
        id: thread.id,
        state: format!("{:?}", thread.state),
        turn_count: thread.turns.len(),
        created_at: thread.created_at.to_rfc3339(),
        updated_at: thread.updated_at.to_rfc3339(),
        title: None,
        thread_type: Some("thread".to_string()),
    };

    // Persist the empty conversation row with thread_type metadata
    if let Some(ref store) = state.store {
        let store = Arc::clone(store);
        let user_id = state.user_id.clone();
        tokio::spawn(async move {
            if let Err(e) = store
                .ensure_conversation(thread_id, "gateway", &user_id, None)
                .await
            {
                tracing::warn!("Failed to persist new thread: {}", e);
            }
            let metadata_val = serde_json::json!("thread");
            if let Err(e) = store
                .update_conversation_metadata_field(thread_id, "thread_type", &metadata_val)
                .await
            {
                tracing::warn!("Failed to set thread_type metadata: {}", e);
            }
        });
    }

    Ok(Json(info))
}

// --- Memory handlers ---

#[derive(Deserialize)]
struct TreeQuery {
    #[allow(dead_code)]
    depth: Option<usize>,
}

async fn memory_tree_handler(
    State(state): State<Arc<GatewayState>>,
    Query(_query): Query<TreeQuery>,
) -> Result<Json<MemoryTreeResponse>, (StatusCode, String)> {
    let workspace = state.workspace.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Workspace not available".to_string(),
    ))?;

    // Build tree from list_all (flat list of all paths)
    let all_paths = workspace
        .list_all()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Collect unique directories and files
    let mut entries: Vec<TreeEntry> = Vec::new();
    let mut seen_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for path in &all_paths {
        // Add parent directories
        let parts: Vec<&str> = path.split('/').collect();
        for i in 0..parts.len().saturating_sub(1) {
            let dir_path = parts[..=i].join("/");
            if seen_dirs.insert(dir_path.clone()) {
                entries.push(TreeEntry {
                    path: dir_path,
                    is_dir: true,
                });
            }
        }
        // Add the file itself
        entries.push(TreeEntry {
            path: path.clone(),
            is_dir: false,
        });
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(Json(MemoryTreeResponse { entries }))
}

#[derive(Deserialize)]
struct ListQuery {
    path: Option<String>,
}

async fn memory_list_handler(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<MemoryListResponse>, (StatusCode, String)> {
    let workspace = state.workspace.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Workspace not available".to_string(),
    ))?;

    let path = query.path.as_deref().unwrap_or("");
    let entries = workspace
        .list(path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let list_entries: Vec<ListEntry> = entries
        .iter()
        .map(|e| ListEntry {
            name: e.path.rsplit('/').next().unwrap_or(&e.path).to_string(),
            path: e.path.clone(),
            is_dir: e.is_directory,
            updated_at: e.updated_at.map(|dt| dt.to_rfc3339()),
        })
        .collect();

    Ok(Json(MemoryListResponse {
        path: path.to_string(),
        entries: list_entries,
    }))
}

#[derive(Deserialize)]
struct ReadQuery {
    path: String,
}

async fn memory_read_handler(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<ReadQuery>,
) -> Result<Json<MemoryReadResponse>, (StatusCode, String)> {
    let workspace = state.workspace.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Workspace not available".to_string(),
    ))?;

    let doc = workspace
        .read(&query.path)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

    Ok(Json(MemoryReadResponse {
        path: query.path,
        content: doc.content,
        updated_at: Some(doc.updated_at.to_rfc3339()),
    }))
}

async fn memory_write_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<MemoryWriteRequest>,
) -> Result<Json<MemoryWriteResponse>, (StatusCode, String)> {
    let workspace = state.workspace.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Workspace not available".to_string(),
    ))?;

    workspace
        .write(&req.path, &req.content)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(MemoryWriteResponse {
        path: req.path,
        status: "written",
    }))
}

async fn memory_search_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<MemorySearchRequest>,
) -> Result<Json<MemorySearchResponse>, (StatusCode, String)> {
    let workspace = state.workspace.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Workspace not available".to_string(),
    ))?;

    let limit = req.limit.unwrap_or(10);
    let results = workspace
        .search(&req.query, limit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let hits: Vec<SearchHit> = results
        .iter()
        .map(|r| SearchHit {
            path: r.document_id.to_string(),
            content: r.content.clone(),
            score: r.score as f64,
        })
        .collect();

    Ok(Json(MemorySearchResponse { results: hits }))
}

// --- Jobs handlers ---

async fn jobs_list_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<JobListResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    // Fetch sandbox jobs scoped to the authenticated user.
    let sandbox_jobs = store
        .list_sandbox_jobs_for_user(&state.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Scope jobs to the authenticated user.
    let mut jobs: Vec<JobInfo> = sandbox_jobs
        .iter()
        .filter(|j| j.user_id == state.user_id)
        .map(|j| {
            let ui_state = match j.status.as_str() {
                "creating" => "pending",
                "running" => "in_progress",
                s => s,
            };
            JobInfo {
                id: j.id,
                title: j.task.clone(),
                state: ui_state.to_string(),
                user_id: j.user_id.clone(),
                created_at: j.created_at.to_rfc3339(),
                started_at: j.started_at.map(|dt| dt.to_rfc3339()),
            }
        })
        .collect();

    // Most recent first.
    jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(Json(JobListResponse { jobs }))
}

async fn jobs_summary_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<JobSummaryResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let s = store
        .sandbox_job_summary_for_user(&state.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(JobSummaryResponse {
        total: s.total,
        pending: s.creating,
        in_progress: s.running,
        completed: s.completed,
        failed: s.failed + s.interrupted,
        stuck: 0,
    }))
}

async fn jobs_detail_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<JobDetailResponse>, (StatusCode, String)> {
    let job_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    // Try sandbox job from DB first, scoped to the authenticated user.
    if let Some(ref store) = state.store
        && let Ok(Some(job)) = store.get_sandbox_job(job_id).await
    {
        if job.user_id != state.user_id {
            return Err((StatusCode::NOT_FOUND, "Job not found".to_string()));
        }
        let browse_id = std::path::Path::new(&job.project_dir)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| job.id.to_string());

        let ui_state = match job.status.as_str() {
            "creating" => "pending",
            "running" => "in_progress",
            s => s,
        };

        let elapsed_secs = job.started_at.map(|start| {
            let end = job.completed_at.unwrap_or_else(chrono::Utc::now);
            (end - start).num_seconds().max(0) as u64
        });

        // Synthesize transitions from timestamps.
        let mut transitions = Vec::new();
        if let Some(started) = job.started_at {
            transitions.push(TransitionInfo {
                from: "creating".to_string(),
                to: "running".to_string(),
                timestamp: started.to_rfc3339(),
                reason: None,
            });
        }
        if let Some(completed) = job.completed_at {
            transitions.push(TransitionInfo {
                from: "running".to_string(),
                to: job.status.clone(),
                timestamp: completed.to_rfc3339(),
                reason: job.failure_reason.clone(),
            });
        }

        return Ok(Json(JobDetailResponse {
            id: job.id,
            title: job.task.clone(),
            description: String::new(),
            state: ui_state.to_string(),
            user_id: job.user_id.clone(),
            created_at: job.created_at.to_rfc3339(),
            started_at: job.started_at.map(|dt| dt.to_rfc3339()),
            completed_at: job.completed_at.map(|dt| dt.to_rfc3339()),
            elapsed_secs,
            project_dir: Some(job.project_dir.clone()),
            browse_url: Some(format!("/projects/{}/", browse_id)),
            job_mode: {
                let mode = store.get_sandbox_job_mode(job.id).await.ok().flatten();
                mode.filter(|m| m != "worker")
            },
            transitions,
        }));
    }

    Err((StatusCode::NOT_FOUND, "Job not found".to_string()))
}

async fn jobs_cancel_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    // Try sandbox job cancellation, scoped to the authenticated user.
    if let Some(ref store) = state.store
        && let Ok(Some(job)) = store.get_sandbox_job(job_id).await
    {
        if job.user_id != state.user_id {
            return Err((StatusCode::NOT_FOUND, "Job not found".to_string()));
        }
        if job.status == "running" || job.status == "creating" {
            // Stop the container if we have a job manager.
            if let Some(ref jm) = state.job_manager
                && let Err(e) = jm.stop_job(job_id).await
            {
                tracing::warn!(job_id = %job_id, error = %e, "Failed to stop container during cancellation");
            }
            store
                .update_sandbox_job_status(
                    job_id,
                    "failed",
                    Some(false),
                    Some("Cancelled by user"),
                    None,
                    Some(chrono::Utc::now()),
                )
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
        return Ok(Json(serde_json::json!({
            "status": "cancelled",
            "job_id": job_id,
        })));
    }

    Err((StatusCode::NOT_FOUND, "Job not found".to_string()))
}

async fn jobs_restart_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;
    let jm = state.job_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Sandbox not enabled".to_string(),
    ))?;

    let old_job_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    let old_job = store
        .get_sandbox_job(old_job_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Job not found".to_string()))?;

    // Scope to the authenticated user.
    if old_job.user_id != state.user_id {
        return Err((StatusCode::NOT_FOUND, "Job not found".to_string()));
    }

    if old_job.status != "interrupted" && old_job.status != "failed" {
        return Err((
            StatusCode::CONFLICT,
            format!("Cannot restart job in state '{}'", old_job.status),
        ));
    }

    // Create a new job with the same task and project_dir.
    let new_job_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let record = crate::history::SandboxJobRecord {
        id: new_job_id,
        task: old_job.task.clone(),
        status: "creating".to_string(),
        user_id: old_job.user_id.clone(),
        project_dir: old_job.project_dir.clone(),
        success: None,
        failure_reason: None,
        created_at: now,
        started_at: None,
        completed_at: None,
        credential_grants_json: old_job.credential_grants_json.clone(),
    };
    store
        .save_sandbox_job(&record)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Look up the original job's mode so the restart uses the same mode.
    let mode = match store.get_sandbox_job_mode(old_job_id).await {
        Ok(Some(m)) if m == "claude_code" => crate::orchestrator::job_manager::JobMode::ClaudeCode,
        _ => crate::orchestrator::job_manager::JobMode::Worker,
    };

    // Restore credential grants from the original job so the restarted container
    // has access to the same secrets.
    let credential_grants: Vec<crate::orchestrator::auth::CredentialGrant> =
        serde_json::from_str(&old_job.credential_grants_json).unwrap_or_else(|e| {
            tracing::warn!(
                job_id = %old_job.id,
                "Failed to deserialize credential grants from stored job: {}. \
                 Restarted job will have no credentials.",
                e
            );
            vec![]
        });

    let project_dir = std::path::PathBuf::from(&old_job.project_dir);
    let _token = jm
        .create_job(
            new_job_id,
            &old_job.task,
            Some(project_dir),
            mode,
            credential_grants,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create container: {}", e),
            )
        })?;

    store
        .update_sandbox_job_status(new_job_id, "running", None, None, Some(now), None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "restarted",
        "old_job_id": old_job_id,
        "new_job_id": new_job_id,
    })))
}

// --- Claude Code prompt and events handlers ---

/// Submit a follow-up prompt to a running Claude Code sandbox job.
async fn jobs_prompt_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let prompt_queue = state.prompt_queue.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Claude Code not configured".to_string(),
    ))?;

    let job_id: uuid::Uuid = id
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    // Verify user owns this job.
    if let Some(ref store) = state.store
        && !store
            .sandbox_job_belongs_to_user(job_id, &state.user_id)
            .await
            .unwrap_or(false)
    {
        return Err((StatusCode::NOT_FOUND, "Job not found".to_string()));
    }

    let content = body
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Missing 'content' field".to_string(),
        ))?
        .to_string();

    let done = body.get("done").and_then(|v| v.as_bool()).unwrap_or(false);

    let prompt = crate::orchestrator::api::PendingPrompt { content, done };

    {
        let mut queue = prompt_queue.lock().await;
        queue.entry(job_id).or_default().push_back(prompt);
    }

    Ok(Json(serde_json::json!({
        "status": "queued",
        "job_id": job_id.to_string(),
    })))
}

/// Load persisted job events for a job (for history replay on page open).
async fn jobs_events_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Database not available".to_string(),
    ))?;

    let job_id: uuid::Uuid = id
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    // Verify user owns this job.
    if !store
        .sandbox_job_belongs_to_user(job_id, &state.user_id)
        .await
        .unwrap_or(false)
    {
        return Err((StatusCode::NOT_FOUND, "Job not found".to_string()));
    }

    let events = store
        .list_job_events(job_id, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let events_json: Vec<serde_json::Value> = events
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "event_type": e.event_type,
                "data": e.data,
                "created_at": e.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "job_id": job_id.to_string(),
        "events": events_json,
    })))
}

// --- Project file handlers for sandbox jobs ---

#[derive(Deserialize)]
struct FilePathQuery {
    path: Option<String>,
}

async fn job_files_list_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    Query(query): Query<FilePathQuery>,
) -> Result<Json<ProjectFilesResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let job_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    let job = store
        .get_sandbox_job(job_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Job not found".to_string()))?;

    // Verify user owns this job.
    if job.user_id != state.user_id {
        return Err((StatusCode::NOT_FOUND, "Job not found".to_string()));
    }

    let base = std::path::PathBuf::from(&job.project_dir);
    let rel_path = query.path.as_deref().unwrap_or("");
    let target = base.join(rel_path);

    // Path traversal guard.
    let canonical = target
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, "Path not found".to_string()))?;
    let base_canonical = base
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, "Project dir not found".to_string()))?;
    if !canonical.starts_with(&base_canonical) {
        return Err((StatusCode::FORBIDDEN, "Forbidden".to_string()));
    }

    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&canonical)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "Cannot read directory".to_string()))?;

    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry
            .file_type()
            .await
            .map(|ft| ft.is_dir())
            .unwrap_or(false);
        let rel = if rel_path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", rel_path, name)
        };
        entries.push(ProjectFileEntry {
            name,
            path: rel,
            is_dir,
        });
    }

    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));

    Ok(Json(ProjectFilesResponse { entries }))
}

async fn job_files_read_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    Query(query): Query<FilePathQuery>,
) -> Result<Json<ProjectFileReadResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let job_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    let job = store
        .get_sandbox_job(job_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Job not found".to_string()))?;

    // Verify user owns this job.
    if job.user_id != state.user_id {
        return Err((StatusCode::NOT_FOUND, "Job not found".to_string()));
    }

    let path = query.path.as_deref().ok_or((
        StatusCode::BAD_REQUEST,
        "path parameter required".to_string(),
    ))?;

    let base = std::path::PathBuf::from(&job.project_dir);
    let file_path = base.join(path);

    let canonical = file_path
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, "File not found".to_string()))?;
    let base_canonical = base
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, "Project dir not found".to_string()))?;
    if !canonical.starts_with(&base_canonical) {
        return Err((StatusCode::FORBIDDEN, "Forbidden".to_string()));
    }

    let content = tokio::fs::read_to_string(&canonical)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "Cannot read file".to_string()))?;

    Ok(Json(ProjectFileReadResponse {
        path: path.to_string(),
        content,
    }))
}

// --- Logs handlers ---

async fn logs_events_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<
    Sse<impl futures::Stream<Item = Result<Event, Infallible>> + Send + 'static>,
    (StatusCode, String),
> {
    let broadcaster = state.log_broadcaster.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Log broadcaster not available".to_string(),
    ))?;

    // Replay recent history so late-joining browsers see startup logs.
    // Subscribe BEFORE snapshotting to avoid a gap between history and live.
    let rx = broadcaster.subscribe();
    let history = broadcaster.recent_entries();

    let history_stream = futures::stream::iter(history).map(|entry| {
        let data = serde_json::to_string(&entry).unwrap_or_default();
        Ok(Event::default().event("log").data(data))
    });

    let live_stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|result| result.ok())
        .map(|entry| {
            let data = serde_json::to_string(&entry).unwrap_or_default();
            Ok(Event::default().event("log").data(data))
        });

    let stream = history_stream.chain(live_stream);

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(30))
            .text(""),
    ))
}

// --- Extension handlers ---

async fn extensions_list_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<ExtensionListResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    let installed = ext_mgr
        .list(None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let extensions = installed
        .into_iter()
        .map(|ext| ExtensionInfo {
            name: ext.name,
            kind: ext.kind.to_string(),
            description: ext.description,
            url: ext.url,
            authenticated: ext.authenticated,
            active: ext.active,
            tools: ext.tools,
        })
        .collect();

    Ok(Json(ExtensionListResponse { extensions }))
}

async fn extensions_tools_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<ToolListResponse>, (StatusCode, String)> {
    let registry = state.tool_registry.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Tool registry not available".to_string(),
    ))?;

    let definitions = registry.tool_definitions().await;
    let tools = definitions
        .into_iter()
        .map(|td| ToolInfo {
            name: td.name,
            description: td.description,
        })
        .collect();

    Ok(Json(ToolListResponse { tools }))
}

async fn extensions_install_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<InstallExtensionRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    let kind_hint = req.kind.as_deref().and_then(|k| match k {
        "mcp_server" => Some(crate::extensions::ExtensionKind::McpServer),
        "wasm_tool" => Some(crate::extensions::ExtensionKind::WasmTool),
        "wasm_channel" => Some(crate::extensions::ExtensionKind::WasmChannel),
        _ => None,
    });

    match ext_mgr
        .install(&req.name, req.url.as_deref(), kind_hint)
        .await
    {
        Ok(result) => Ok(Json(ActionResponse::ok(result.message))),
        Err(e) => Ok(Json(ActionResponse::fail(e.to_string()))),
    }
}

async fn extensions_activate_handler(
    State(state): State<Arc<GatewayState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    match ext_mgr.activate(&name).await {
        Ok(result) => Ok(Json(ActionResponse::ok(result.message))),
        Err(activate_err) => {
            let err_str = activate_err.to_string();
            let needs_auth = err_str.contains("authentication")
                || err_str.contains("401")
                || err_str.contains("Unauthorized");

            if !needs_auth {
                return Ok(Json(ActionResponse::fail(err_str)));
            }

            // Activation failed due to auth; try authenticating first.
            match ext_mgr.auth(&name, None).await {
                Ok(auth_result) if auth_result.status == "authenticated" => {
                    // Auth succeeded, retry activation.
                    match ext_mgr.activate(&name).await {
                        Ok(result) => Ok(Json(ActionResponse::ok(result.message))),
                        Err(e) => Ok(Json(ActionResponse::fail(e.to_string()))),
                    }
                }
                Ok(auth_result) => {
                    // Auth in progress (OAuth URL or awaiting manual token).
                    let mut resp = ActionResponse::fail(
                        auth_result
                            .instructions
                            .clone()
                            .unwrap_or_else(|| format!("'{}' requires authentication.", name)),
                    );
                    resp.auth_url = auth_result.auth_url;
                    resp.awaiting_token = Some(auth_result.awaiting_token);
                    resp.instructions = auth_result.instructions;
                    Ok(Json(resp))
                }
                Err(auth_err) => Ok(Json(ActionResponse::fail(format!(
                    "Authentication failed: {}",
                    auth_err
                )))),
            }
        }
    }
}

// --- Project file serving handlers ---

/// Redirect `/projects/{id}` to `/projects/{id}/` so relative paths in
/// the served HTML resolve within the project namespace.
async fn project_redirect_handler(Path(project_id): Path<String>) -> impl IntoResponse {
    axum::response::Redirect::permanent(&format!("/projects/{project_id}/"))
}

/// Serve `index.html` when hitting `/projects/{project_id}/`.
async fn project_index_handler(Path(project_id): Path<String>) -> impl IntoResponse {
    serve_project_file(&project_id, "index.html").await
}

/// Serve any file under `/projects/{project_id}/{path}`.
async fn project_file_handler(
    Path((project_id, path)): Path<(String, String)>,
) -> impl IntoResponse {
    serve_project_file(&project_id, &path).await
}

/// Shared logic: resolve the file inside `~/.enclagent/projects/{project_id}/`,
/// guard against path traversal, and stream the content with the right MIME type.
async fn serve_project_file(project_id: &str, path: &str) -> axum::response::Response {
    // Reject project_id values that could escape the projects directory.
    if project_id.contains('/')
        || project_id.contains('\\')
        || project_id.contains("..")
        || project_id.is_empty()
    {
        return (StatusCode::BAD_REQUEST, "Invalid project ID").into_response();
    }

    let base = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".enclagent")
        .join("projects")
        .join(project_id);

    let file_path = base.join(path);

    // Path traversal guard
    let canonical = match file_path.canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };
    if !canonical.starts_with(&base_canonical) {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    match tokio::fs::read(&canonical).await {
        Ok(contents) => {
            let mime = mime_guess::from_path(&canonical)
                .first_or_octet_stream()
                .to_string();
            ([(header::CONTENT_TYPE, mime)], contents).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

async fn extensions_remove_handler(
    State(state): State<Arc<GatewayState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    match ext_mgr.remove(&name).await {
        Ok(message) => Ok(Json(ActionResponse::ok(message))),
        Err(e) => Ok(Json(ActionResponse::fail(e.to_string()))),
    }
}

// --- Skills handlers ---

async fn skills_list_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<super::types::SkillListResponse>, (StatusCode, String)> {
    let registry = state.skill_registry.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skills system not enabled".to_string(),
    ))?;

    let guard = registry.read().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Skill registry lock poisoned: {}", e),
        )
    })?;

    let skills: Vec<super::types::SkillInfo> = guard
        .skills()
        .iter()
        .map(|s| super::types::SkillInfo {
            name: s.manifest.name.clone(),
            description: s.manifest.description.clone(),
            version: s.manifest.version.clone(),
            trust: s.trust.to_string(),
            source: format!("{:?}", s.source),
            keywords: s.manifest.activation.keywords.clone(),
        })
        .collect();

    let count = skills.len();
    Ok(Json(super::types::SkillListResponse { skills, count }))
}

async fn skills_search_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<super::types::SkillSearchRequest>,
) -> Result<Json<super::types::SkillSearchResponse>, (StatusCode, String)> {
    let registry = state.skill_registry.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skills system not enabled".to_string(),
    ))?;

    let catalog = state.skill_catalog.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skill catalog not available".to_string(),
    ))?;

    // Search ClawHub catalog
    let catalog_results = catalog.search(&req.query).await;
    let catalog_json: Vec<serde_json::Value> = catalog_results
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "slug": e.slug,
                "name": e.name,
                "description": e.description,
                "version": e.version,
                "score": e.score,
            })
        })
        .collect();

    // Search local skills
    let query_lower = req.query.to_lowercase();
    let installed: Vec<super::types::SkillInfo> = {
        let guard = registry.read().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Skill registry lock poisoned: {}", e),
            )
        })?;
        guard
            .skills()
            .iter()
            .filter(|s| {
                s.manifest.name.to_lowercase().contains(&query_lower)
                    || s.manifest.description.to_lowercase().contains(&query_lower)
            })
            .map(|s| super::types::SkillInfo {
                name: s.manifest.name.clone(),
                description: s.manifest.description.clone(),
                version: s.manifest.version.clone(),
                trust: s.trust.to_string(),
                source: format!("{:?}", s.source),
                keywords: s.manifest.activation.keywords.clone(),
            })
            .collect()
    };

    Ok(Json(super::types::SkillSearchResponse {
        catalog: catalog_json,
        installed,
        registry_url: catalog.registry_url().to_string(),
    }))
}

async fn skills_install_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<super::types::SkillInstallRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    // Require explicit confirmation header to prevent accidental installs.
    // Chat tools have requires_approval(); this is the equivalent for the web API.
    if headers
        .get("x-confirm-action")
        .and_then(|v| v.to_str().ok())
        != Some("true")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Skill install requires X-Confirm-Action: true header".to_string(),
        ));
    }

    let registry = state.skill_registry.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skills system not enabled".to_string(),
    ))?;

    let content = if let Some(ref raw) = req.content {
        raw.clone()
    } else if let Some(ref url) = req.url {
        // Fetch from explicit URL (with SSRF protection)
        crate::tools::builtin::skill_tools::fetch_skill_content(url)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    } else if let Some(ref catalog) = state.skill_catalog {
        let url = crate::skills::catalog::skill_download_url(catalog.registry_url(), &req.name);
        crate::tools::builtin::skill_tools::fetch_skill_content(&url)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
    } else {
        return Ok(Json(ActionResponse::fail(
            "Provide 'content' or 'url' to install a skill".to_string(),
        )));
    };

    // Parse, check duplicates, and get user_dir under a brief read lock.
    let (user_dir, skill_name_from_parse) = {
        let guard = registry.read().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Skill registry lock poisoned: {}", e),
            )
        })?;

        let normalized = crate::skills::normalize_line_endings(&content);
        let parsed = crate::skills::parser::parse_skill_md(&normalized)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let skill_name = parsed.manifest.name.clone();

        if guard.has(&skill_name) {
            return Ok(Json(ActionResponse::fail(format!(
                "Skill '{}' already exists",
                skill_name
            ))));
        }

        (guard.user_dir().to_path_buf(), skill_name)
    };

    // Perform async I/O (write to disk, load) with no lock held.
    let normalized = crate::skills::normalize_line_endings(&content);
    let (skill_name, loaded_skill) =
        crate::skills::registry::SkillRegistry::prepare_install_to_disk(
            &user_dir,
            &skill_name_from_parse,
            &normalized,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Commit: brief write lock for in-memory addition
    let mut guard = registry.write().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Skill registry lock poisoned: {}", e),
        )
    })?;

    match guard.commit_install(&skill_name, loaded_skill) {
        Ok(()) => Ok(Json(ActionResponse::ok(format!(
            "Skill '{}' installed",
            skill_name
        )))),
        Err(e) => Ok(Json(ActionResponse::fail(e.to_string()))),
    }
}

async fn skills_remove_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    // Require explicit confirmation header to prevent accidental removals.
    if headers
        .get("x-confirm-action")
        .and_then(|v| v.to_str().ok())
        != Some("true")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Skill removal requires X-Confirm-Action: true header".to_string(),
        ));
    }

    let registry = state.skill_registry.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skills system not enabled".to_string(),
    ))?;

    // Validate removal under a brief read lock
    let skill_path = {
        let guard = registry.read().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Skill registry lock poisoned: {}", e),
            )
        })?;
        guard
            .validate_remove(&name)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    };

    // Delete files from disk (async I/O, no lock held)
    crate::skills::registry::SkillRegistry::delete_skill_files(&skill_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Remove from in-memory registry under a brief write lock
    let mut guard = registry.write().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Skill registry lock poisoned: {}", e),
        )
    })?;

    match guard.commit_remove(&name) {
        Ok(()) => Ok(Json(ActionResponse::ok(format!(
            "Skill '{}' removed",
            name
        )))),
        Err(e) => Ok(Json(ActionResponse::fail(e.to_string()))),
    }
}

// --- Routines handlers ---

async fn routines_list_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<RoutineListResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routines = store
        .list_routines(&state.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<RoutineInfo> = routines.iter().map(routine_to_info).collect();

    Ok(Json(RoutineListResponse { routines: items }))
}

async fn routines_summary_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<RoutineSummaryResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routines = store
        .list_routines(&state.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let total = routines.len() as u64;
    let enabled = routines.iter().filter(|r| r.enabled).count() as u64;
    let disabled = total - enabled;
    let failing = routines
        .iter()
        .filter(|r| r.consecutive_failures > 0)
        .count() as u64;

    let today_start = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc());
    let runs_today = if let Some(start) = today_start {
        routines
            .iter()
            .filter(|r| r.last_run_at.is_some_and(|ts| ts >= start))
            .count() as u64
    } else {
        0
    };

    Ok(Json(RoutineSummaryResponse {
        total,
        enabled,
        disabled,
        failing,
        runs_today,
    }))
}

async fn routines_detail_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<RoutineDetailResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routine_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid routine ID".to_string()))?;

    let routine = store
        .get_routine(routine_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Routine not found".to_string()))?;

    let runs = store
        .list_routine_runs(routine_id, 20)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let recent_runs: Vec<RoutineRunInfo> = runs
        .iter()
        .map(|run| RoutineRunInfo {
            id: run.id,
            trigger_type: run.trigger_type.clone(),
            started_at: run.started_at.to_rfc3339(),
            completed_at: run.completed_at.map(|dt| dt.to_rfc3339()),
            status: format!("{:?}", run.status),
            result_summary: run.result_summary.clone(),
            tokens_used: run.tokens_used,
        })
        .collect();

    let webhook_path =
        crate::agent::routine_engine::routine_webhook_path(routine.id, &routine.trigger);
    let webhook_secret_configured = match &routine.trigger {
        crate::agent::routine::Trigger::Webhook { secret, .. } => Some(secret.is_some()),
        _ => None,
    };

    Ok(Json(RoutineDetailResponse {
        id: routine.id,
        name: routine.name.clone(),
        description: routine.description.clone(),
        enabled: routine.enabled,
        trigger: serde_json::to_value(&routine.trigger).unwrap_or_default(),
        action: serde_json::to_value(&routine.action).unwrap_or_default(),
        guardrails: serde_json::to_value(&routine.guardrails).unwrap_or_default(),
        notify: serde_json::to_value(&routine.notify).unwrap_or_default(),
        last_run_at: routine.last_run_at.map(|dt| dt.to_rfc3339()),
        next_fire_at: routine.next_fire_at.map(|dt| dt.to_rfc3339()),
        run_count: routine.run_count,
        consecutive_failures: routine.consecutive_failures,
        status: crate::agent::routine_engine::routine_status_label(&routine).to_string(),
        health: crate::agent::routine_engine::routine_health_label(&routine).to_string(),
        created_at: routine.created_at.to_rfc3339(),
        trigger_channel: crate::agent::routine_engine::routine_trigger_channel(&routine.trigger),
        webhook_path,
        webhook_secret_configured,
        recent_runs,
    }))
}

async fn routines_visibility_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<RoutineTriggerVisibilityResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routines = store
        .list_routines(&state.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(build_routine_visibility_response(&routines)))
}

async fn routines_trigger_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routine_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid routine ID".to_string()))?;

    let routine = store
        .get_routine(routine_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Routine not found".to_string()))?;

    // Send the routine prompt through the message pipeline as a manual trigger.
    let prompt = match &routine.action {
        crate::agent::routine::RoutineAction::Lightweight { prompt, .. } => prompt.clone(),
        crate::agent::routine::RoutineAction::FullJob {
            title, description, ..
        } => format!("{}: {}", title, description),
    };

    let content = format!("[routine:{}] {}", routine.name, prompt);
    let msg = IncomingMessage::new("gateway", &state.user_id, content);

    let tx_guard = state.msg_tx.read().await;
    let tx = tx_guard.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Channel not started".to_string(),
    ))?;

    tx.send(msg).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Channel closed".to_string(),
        )
    })?;

    Ok(Json(serde_json::json!({
        "status": "triggered",
        "routine_id": routine_id,
    })))
}

#[derive(Deserialize)]
struct ToggleRequest {
    enabled: Option<bool>,
}

async fn routines_toggle_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    body: Option<Json<ToggleRequest>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routine_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid routine ID".to_string()))?;

    let mut routine = store
        .get_routine(routine_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Routine not found".to_string()))?;

    // If a specific value was provided, use it; otherwise toggle.
    routine.enabled = match body {
        Some(Json(req)) => req.enabled.unwrap_or(!routine.enabled),
        None => !routine.enabled,
    };

    store
        .update_routine(&routine)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": if routine.enabled { "enabled" } else { "disabled" },
        "routine_id": routine_id,
    })))
}

async fn routines_delete_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routine_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid routine ID".to_string()))?;

    let deleted = store
        .delete_routine(routine_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if deleted {
        Ok(Json(serde_json::json!({
            "status": "deleted",
            "routine_id": routine_id,
        })))
    } else {
        Err((StatusCode::NOT_FOUND, "Routine not found".to_string()))
    }
}

async fn routines_runs_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routine_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid routine ID".to_string()))?;

    let runs = store
        .list_routine_runs(routine_id, 50)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let run_infos: Vec<RoutineRunInfo> = runs
        .iter()
        .map(|run| RoutineRunInfo {
            id: run.id,
            trigger_type: run.trigger_type.clone(),
            started_at: run.started_at.to_rfc3339(),
            completed_at: run.completed_at.map(|dt| dt.to_rfc3339()),
            status: format!("{:?}", run.status),
            result_summary: run.result_summary.clone(),
            tokens_used: run.tokens_used,
        })
        .collect();

    Ok(Json(serde_json::json!({
        "routine_id": routine_id,
        "runs": run_infos,
    })))
}

/// Convert a Routine to the trimmed RoutineInfo for list display.
fn routine_to_info(r: &crate::agent::routine::Routine) -> RoutineInfo {
    let (trigger_type, trigger_summary) = match &r.trigger {
        crate::agent::routine::Trigger::Cron { schedule } => {
            ("cron".to_string(), format!("cron: {}", schedule))
        }
        crate::agent::routine::Trigger::Event {
            pattern, channel, ..
        } => {
            let ch = channel.as_deref().unwrap_or("any");
            ("event".to_string(), format!("on {} /{}/", ch, pattern))
        }
        crate::agent::routine::Trigger::Webhook { path, .. } => {
            let p = path.as_deref().unwrap_or("/");
            ("webhook".to_string(), format!("webhook: {}", p))
        }
        crate::agent::routine::Trigger::Manual => ("manual".to_string(), "manual only".to_string()),
    };

    let action_type = match &r.action {
        crate::agent::routine::RoutineAction::Lightweight { .. } => "lightweight",
        crate::agent::routine::RoutineAction::FullJob { .. } => "full_job",
    };
    let webhook_path = crate::agent::routine_engine::routine_webhook_path(r.id, &r.trigger);
    let webhook_secret_configured = match &r.trigger {
        crate::agent::routine::Trigger::Webhook { secret, .. } => Some(secret.is_some()),
        _ => None,
    };

    RoutineInfo {
        id: r.id,
        name: r.name.clone(),
        description: r.description.clone(),
        enabled: r.enabled,
        trigger_type,
        trigger_summary,
        action_type: action_type.to_string(),
        last_run_at: r.last_run_at.map(|dt| dt.to_rfc3339()),
        next_fire_at: r.next_fire_at.map(|dt| dt.to_rfc3339()),
        run_count: r.run_count,
        consecutive_failures: r.consecutive_failures,
        status: crate::agent::routine_engine::routine_status_label(r).to_string(),
        health: crate::agent::routine_engine::routine_health_label(r).to_string(),
        trigger_channel: crate::agent::routine_engine::routine_trigger_channel(&r.trigger),
        webhook_path,
        webhook_secret_configured,
    }
}

#[derive(Default)]
struct ChannelRoutineAccumulator {
    total_routines: u64,
    enabled_routines: u64,
    failing_routines: u64,
    last_run_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn status_from_failures(enabled: u64, failing: u64) -> &'static str {
    if enabled == 0 {
        "idle"
    } else if failing == 0 {
        "healthy"
    } else if failing < enabled {
        "degraded"
    } else {
        "failing"
    }
}

fn build_routine_visibility_response(
    routines: &[crate::agent::routine::Routine],
) -> RoutineTriggerVisibilityResponse {
    let mut channels: BTreeMap<String, ChannelRoutineAccumulator> = BTreeMap::new();
    let mut webhook_routes = Vec::new();
    let mut webhook_total = 0u64;
    let mut webhook_enabled = 0u64;
    let mut webhook_with_secret = 0u64;
    let mut webhook_failing = 0u64;

    for routine in routines {
        if let Some(channel_name) =
            crate::agent::routine_engine::routine_trigger_channel(&routine.trigger)
        {
            let entry = channels.entry(channel_name).or_default();
            entry.total_routines += 1;
            if routine.enabled {
                entry.enabled_routines += 1;
            }
            if routine.enabled && routine.consecutive_failures > 0 {
                entry.failing_routines += 1;
            }
            if let Some(last_run_at) = routine.last_run_at
                && match entry.last_run_at {
                    Some(current) => last_run_at > current,
                    None => true,
                }
            {
                entry.last_run_at = Some(last_run_at);
            }
        }

        if let crate::agent::routine::Trigger::Webhook { secret, .. } = &routine.trigger {
            webhook_total += 1;
            if routine.enabled {
                webhook_enabled += 1;
            }
            if secret.is_some() {
                webhook_with_secret += 1;
            }
            if routine.enabled && routine.consecutive_failures > 0 {
                webhook_failing += 1;
            }

            webhook_routes.push(WebhookRouteInfo {
                routine_id: routine.id,
                routine_name: routine.name.clone(),
                path: crate::agent::routine_engine::routine_webhook_path(
                    routine.id,
                    &routine.trigger,
                )
                .unwrap_or_else(|| format!("/hooks/routine/{}", routine.id)),
                enabled: routine.enabled,
                secret_configured: secret.is_some(),
                status: crate::agent::routine_engine::routine_health_label(routine).to_string(),
            });
        }
    }

    let channel_rows = channels
        .into_iter()
        .map(|(channel, row)| {
            let healthy_routines = row.enabled_routines.saturating_sub(row.failing_routines);
            RoutineChannelHealth {
                channel,
                total_routines: row.total_routines,
                enabled_routines: row.enabled_routines,
                healthy_routines,
                failing_routines: row.failing_routines,
                status: status_from_failures(row.enabled_routines, row.failing_routines)
                    .to_string(),
                last_run_at: row.last_run_at.map(|dt| dt.to_rfc3339()),
            }
        })
        .collect();

    webhook_routes.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.routine_name.cmp(&b.routine_name))
    });

    RoutineTriggerVisibilityResponse {
        generated_at: chrono::Utc::now().to_rfc3339(),
        channels: channel_rows,
        webhook: WebhookTriggerVisibility {
            total_routines: webhook_total,
            enabled_routines: webhook_enabled,
            with_secret: webhook_with_secret,
            status: if webhook_total == 0 {
                "none".to_string()
            } else {
                status_from_failures(webhook_enabled, webhook_failing).to_string()
            },
            routes: webhook_routes,
        },
    }
}

// --- Settings handlers ---

async fn settings_list_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<SettingsListResponse>, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let rows = store.list_settings(&state.user_id).await.map_err(|e| {
        tracing::error!("Failed to list settings: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let settings = rows
        .into_iter()
        .map(|r| SettingResponse {
            key: r.key,
            value: r.value,
            updated_at: r.updated_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(SettingsListResponse { settings }))
}

async fn settings_get_handler(
    State(state): State<Arc<GatewayState>>,
    Path(key): Path<String>,
) -> Result<Json<SettingResponse>, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let row = store
        .get_setting_full(&state.user_id, &key)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get setting '{}': {}", key, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(SettingResponse {
        key: row.key,
        value: row.value,
        updated_at: row.updated_at.to_rfc3339(),
    }))
}

async fn settings_set_handler(
    State(state): State<Arc<GatewayState>>,
    Path(key): Path<String>,
    Json(body): Json<SettingWriteRequest>,
) -> Result<StatusCode, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store
        .set_setting(&state.user_id, &key, &body.value)
        .await
        .map_err(|e| {
            tracing::error!("Failed to set setting '{}': {}", key, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT)
}

async fn settings_delete_handler(
    State(state): State<Arc<GatewayState>>,
    Path(key): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store
        .delete_setting(&state.user_id, &key)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete setting '{}': {}", key, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT)
}

async fn settings_export_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<SettingsExportResponse>, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let settings = store.get_all_settings(&state.user_id).await.map_err(|e| {
        tracing::error!("Failed to export settings: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(SettingsExportResponse { settings }))
}

async fn settings_import_handler(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<SettingsImportRequest>,
) -> Result<StatusCode, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store
        .set_all_settings(&state.user_id, &body.settings)
        .await
        .map_err(|e| {
            tracing::error!("Failed to import settings: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT)
}

const PLATFORM_ORG_WORKSPACE_KEY: &str = "platform.org.workspace";
const PLATFORM_ORG_MEMBERS_KEY: &str = "platform.org.members";

// --- Modules handlers ---

async fn modules_catalog_handler() -> Json<ModuleCatalogResponse> {
    Json(ModuleCatalogResponse {
        modules: crate::platform::curated_module_catalog(),
    })
}

async fn modules_state_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<ModuleStateResponse>, (StatusCode, String)> {
    let modules = load_module_state(&state).await;
    Ok(Json(ModuleStateResponse { modules }))
}

async fn modules_enable_handler(
    State(state): State<Arc<GatewayState>>,
    Path(module_id): Path<String>,
) -> Result<Json<ModuleUpdateResponse>, (StatusCode, String)> {
    update_module_enabled(&state, &module_id, true).await
}

async fn modules_disable_handler(
    State(state): State<Arc<GatewayState>>,
    Path(module_id): Path<String>,
) -> Result<Json<ModuleUpdateResponse>, (StatusCode, String)> {
    update_module_enabled(&state, &module_id, false).await
}

async fn modules_health_handler(
    State(state): State<Arc<GatewayState>>,
    Path(module_id): Path<String>,
) -> Result<Json<ModuleHealthResponse>, (StatusCode, String)> {
    let modules = load_module_state(&state).await;
    let module = modules
        .into_iter()
        .find(|m| m.module_id == module_id)
        .ok_or((StatusCode::NOT_FOUND, "Unknown module".to_string()))?;

    let settings = load_effective_settings(&state).await;
    let verification = build_verification_status_response(&settings);
    let checks = serde_json::json!({
        "module_id": module.module_id,
        "enabled": module.enabled,
        "catalog_entry_exists": crate::platform::module_exists(&module_id),
        "verification_status": verification.status,
        "verification_backend": verification.backend,
    });

    Ok(Json(ModuleHealthResponse {
        module_id,
        enabled: module.enabled,
        status: if module.enabled {
            "healthy".to_string()
        } else {
            "disabled".to_string()
        },
        checks,
    }))
}

async fn modules_config_handler(
    State(state): State<Arc<GatewayState>>,
    Path(module_id): Path<String>,
    Json(body): Json<ModuleConfigUpdateRequest>,
) -> Result<Json<ModuleUpdateResponse>, (StatusCode, String)> {
    if !crate::platform::module_exists(&module_id) {
        return Err((StatusCode::NOT_FOUND, "Unknown module".to_string()));
    }

    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;
    require_module_admin(&state).await?;

    let mut modules = load_module_state(&state).await;
    let now = chrono::Utc::now().to_rfc3339();
    let module_idx = modules
        .iter()
        .position(|m| m.module_id == module_id)
        .ok_or((StatusCode::NOT_FOUND, "Unknown module".to_string()))?;

    modules[module_idx].config = body.config;
    modules[module_idx].status = if modules[module_idx].enabled {
        "enabled".to_string()
    } else {
        "disabled".to_string()
    };
    modules[module_idx].updated_at = now;
    let updated_module = modules[module_idx].clone();
    let serialized = serde_json::to_value(&modules)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    store
        .set_setting(
            &state.user_id,
            crate::platform::PLATFORM_MODULE_STATE_KEY,
            &serialized,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ModuleUpdateResponse {
        module: updated_module,
    }))
}

async fn update_module_enabled(
    state: &GatewayState,
    module_id: &str,
    enabled: bool,
) -> Result<Json<ModuleUpdateResponse>, (StatusCode, String)> {
    if !crate::platform::module_exists(module_id) {
        return Err((StatusCode::NOT_FOUND, "Unknown module".to_string()));
    }

    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;
    require_module_admin(state).await?;

    let mut modules = load_module_state(state).await;
    let now = chrono::Utc::now().to_rfc3339();
    let module_idx = modules
        .iter()
        .position(|m| m.module_id == module_id)
        .ok_or((StatusCode::NOT_FOUND, "Unknown module".to_string()))?;

    modules[module_idx].enabled = enabled;
    modules[module_idx].status = if enabled {
        "enabled".to_string()
    } else {
        "disabled".to_string()
    };
    modules[module_idx].updated_at = now;
    let updated_module = modules[module_idx].clone();
    let serialized = serde_json::to_value(&modules)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    store
        .set_setting(
            &state.user_id,
            crate::platform::PLATFORM_MODULE_STATE_KEY,
            &serialized,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ModuleUpdateResponse {
        module: updated_module,
    }))
}

async fn load_module_state(state: &GatewayState) -> Vec<ModuleState> {
    let defaults = crate::platform::default_module_states();
    let Some(store) = state.store.as_ref() else {
        return defaults;
    };

    let stored = match store
        .get_setting(&state.user_id, crate::platform::PLATFORM_MODULE_STATE_KEY)
        .await
    {
        Ok(Some(value)) => match serde_json::from_value::<Vec<ModuleState>>(value) {
            Ok(parsed) => parsed,
            Err(error) => {
                tracing::warn!("Invalid stored module state; using defaults: {}", error);
                Vec::new()
            }
        },
        Ok(None) => Vec::new(),
        Err(error) => {
            tracing::warn!("Failed to load module state; using defaults: {}", error);
            Vec::new()
        }
    };

    if stored.is_empty() {
        defaults
    } else {
        crate::platform::merge_module_states(stored)
    }
}

async fn require_module_admin(state: &GatewayState) -> Result<(), (StatusCode, String)> {
    let role = actor_role_for_state(state).await;
    if crate::platform::can_manage_modules(&role) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Module management requires owner/admin role".to_string(),
        ))
    }
}

// --- Org handlers ---

async fn org_current_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<OrgCurrentResponse>, (StatusCode, String)> {
    let workspace = load_org_workspace(&state).await;
    let members = load_org_members(&state).await;
    let membership = ensure_actor_membership(&state, members).await?;

    Ok(Json(OrgCurrentResponse {
        workspace,
        membership,
    }))
}

async fn org_members_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<OrgMembersResponse>, (StatusCode, String)> {
    let workspace = load_org_workspace(&state).await;
    let members = load_org_members(&state).await;

    Ok(Json(OrgMembersResponse { workspace, members }))
}

async fn org_members_invite_handler(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<OrgInviteRequest>,
) -> Result<Json<OrgMembersResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;
    let actor_role = actor_role_for_state(&state).await;
    if !crate::platform::can_manage_org(&actor_role) {
        return Err((
            StatusCode::FORBIDDEN,
            "Org invite requires owner/admin role".to_string(),
        ));
    }

    let role = crate::platform::normalize_org_role(&body.role)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid role".to_string()))?;
    let member_id = body.member_id.trim().to_string();
    if member_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "member_id is required".to_string()));
    }

    let mut members = load_org_members(&state).await;
    let now = chrono::Utc::now().to_rfc3339();

    if let Some(existing) = members.iter_mut().find(|m| m.member_id == member_id) {
        existing.role = role;
        existing.status = "invited".to_string();
        existing.updated_at = now.clone();
    } else {
        members.push(OrgMembership {
            member_id,
            role,
            status: "invited".to_string(),
            invited_at: now.clone(),
            updated_at: now,
        });
    }

    store
        .set_setting(
            &state.user_id,
            PLATFORM_ORG_MEMBERS_KEY,
            &serde_json::to_value(&members)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(OrgMembersResponse {
        workspace: load_org_workspace(&state).await,
        members,
    }))
}

async fn org_members_role_handler(
    State(state): State<Arc<GatewayState>>,
    Path(member_id): Path<String>,
    Json(body): Json<OrgRoleUpdateRequest>,
) -> Result<Json<OrgMembersResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;
    let actor_role = actor_role_for_state(&state).await;
    if actor_role != "owner" {
        return Err((
            StatusCode::FORBIDDEN,
            "Only owner can update member roles".to_string(),
        ));
    }

    let normalized_role = crate::platform::normalize_org_role(&body.role)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid role".to_string()))?;

    let mut members = load_org_members(&state).await;
    let now = chrono::Utc::now().to_rfc3339();
    let mut updated = false;

    for member in &mut members {
        if member.member_id == member_id {
            member.role = normalized_role.clone();
            member.status = "active".to_string();
            member.updated_at = now.clone();
            updated = true;
            break;
        }
    }
    if !updated {
        return Err((StatusCode::NOT_FOUND, "Member not found".to_string()));
    }

    store
        .set_setting(
            &state.user_id,
            PLATFORM_ORG_MEMBERS_KEY,
            &serde_json::to_value(&members)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(OrgMembersResponse {
        workspace: load_org_workspace(&state).await,
        members,
    }))
}

async fn org_members_delete_handler(
    State(state): State<Arc<GatewayState>>,
    Path(member_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;
    let actor_role = actor_role_for_state(&state).await;
    if actor_role != "owner" {
        return Err((
            StatusCode::FORBIDDEN,
            "Only owner can remove members".to_string(),
        ));
    }
    if member_id == state.user_id {
        return Err((
            StatusCode::BAD_REQUEST,
            "Owner cannot remove self".to_string(),
        ));
    }

    let mut members = load_org_members(&state).await;
    let before = members.len();
    members.retain(|member| member.member_id != member_id);
    if members.len() == before {
        return Err((StatusCode::NOT_FOUND, "Member not found".to_string()));
    }

    store
        .set_setting(
            &state.user_id,
            PLATFORM_ORG_MEMBERS_KEY,
            &serde_json::to_value(&members)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn actor_role_for_state(state: &GatewayState) -> String {
    let members = load_org_members(state).await;
    members
        .iter()
        .find(|member| member.member_id == state.user_id)
        .map(|member| member.role.clone())
        .unwrap_or_else(|| "member".to_string())
}

async fn ensure_actor_membership(
    state: &GatewayState,
    mut members: Vec<OrgMembership>,
) -> Result<OrgMembership, (StatusCode, String)> {
    if let Some(found) = members
        .iter()
        .find(|member| member.member_id == state.user_id)
        .cloned()
    {
        return Ok(found);
    }

    let now = chrono::Utc::now().to_rfc3339();
    let fallback = OrgMembership {
        member_id: state.user_id.clone(),
        role: "owner".to_string(),
        status: "active".to_string(),
        invited_at: now.clone(),
        updated_at: now,
    };
    members.push(fallback.clone());

    if let Some(store) = state.store.as_ref() {
        let value = serde_json::to_value(&members)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        store
            .set_setting(&state.user_id, PLATFORM_ORG_MEMBERS_KEY, &value)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok(fallback)
}

async fn load_org_workspace(state: &GatewayState) -> OrgWorkspace {
    let default = crate::platform::default_org_workspace(&state.user_id);
    let Some(store) = state.store.as_ref() else {
        return default;
    };
    match store
        .get_setting(&state.user_id, PLATFORM_ORG_WORKSPACE_KEY)
        .await
    {
        Ok(Some(value)) => serde_json::from_value::<OrgWorkspace>(value).unwrap_or(default),
        _ => default,
    }
}

async fn load_org_members(state: &GatewayState) -> Vec<OrgMembership> {
    let default = crate::platform::default_org_memberships(&state.user_id);
    let Some(store) = state.store.as_ref() else {
        return default;
    };

    match store
        .get_setting(&state.user_id, PLATFORM_ORG_MEMBERS_KEY)
        .await
    {
        Ok(Some(value)) => serde_json::from_value::<Vec<OrgMembership>>(value).unwrap_or(default),
        _ => default,
    }
}

// --- Gateway control plane handlers ---

async fn load_effective_settings(state: &GatewayState) -> crate::settings::Settings {
    let Some(store) = state.store.as_ref() else {
        return crate::settings::Settings::default();
    };

    match store.get_all_settings(&state.user_id).await {
        Ok(map) => crate::settings::Settings::from_db_map(&map),
        Err(e) => {
            tracing::warn!("Failed to load settings for status surfaces: {}", e);
            crate::settings::Settings::default()
        }
    }
}

async fn build_channel_status_response(
    state: &GatewayState,
    settings: &crate::settings::Settings,
) -> ChannelStatusResponse {
    let message_pipeline_ready = state.msg_tx.read().await.is_some();
    let sse_connections = state.sse.connection_count();
    let ws_connections = state
        .ws_tracker
        .as_ref()
        .map(|t| t.connection_count())
        .unwrap_or(0);

    let mut channels = Vec::new();
    channels.push(ChannelStatusInfo {
        name: "gateway".to_string(),
        configured: true,
        enabled: true,
        healthy: message_pipeline_ready,
        status: if message_pipeline_ready {
            "healthy".to_string()
        } else {
            "degraded".to_string()
        },
        detail: serde_json::json!({
            "message_pipeline_ready": message_pipeline_ready,
            "sse_connections": sse_connections,
            "ws_connections": ws_connections,
            "max_connections": state.sse.max_connections(),
            "remaining_capacity": state.sse.remaining_capacity(),
        }),
    });

    let http_enabled = settings.channels.http_enabled;
    let http_configured = settings.channels.http_port.is_some();
    let http_healthy = !http_enabled || http_configured;
    channels.push(ChannelStatusInfo {
        name: "http_webhook".to_string(),
        configured: http_configured,
        enabled: http_enabled,
        healthy: http_healthy,
        status: if !http_enabled {
            "disabled".to_string()
        } else if http_healthy {
            "healthy".to_string()
        } else {
            "degraded".to_string()
        },
        detail: serde_json::json!({
            "host": settings.channels.http_host.clone(),
            "port": settings.channels.http_port,
        }),
    });

    let wasm_enabled = settings.channels.wasm_channels_enabled;
    let wasm_configured = !settings.channels.wasm_channels.is_empty();
    let wasm_healthy = !wasm_enabled || wasm_configured;
    channels.push(ChannelStatusInfo {
        name: "wasm_channels".to_string(),
        configured: wasm_configured,
        enabled: wasm_enabled,
        healthy: wasm_healthy,
        status: if !wasm_enabled {
            "disabled".to_string()
        } else if wasm_healthy {
            "healthy".to_string()
        } else {
            "idle".to_string()
        },
        detail: serde_json::json!({
            "configured_channels": settings.channels.wasm_channels.clone(),
            "channels_dir": settings
                .channels
                .wasm_channels_dir
                .as_ref()
            .map(|p| p.display().to_string()),
        }),
    });

    let mcp_servers = match crate::tools::mcp::config::load_mcp_servers().await {
        Ok(servers) => servers,
        Err(error) => {
            channels.push(ChannelStatusInfo {
                name: "mcp_servers".to_string(),
                configured: false,
                enabled: false,
                healthy: false,
                status: "degraded".to_string(),
                detail: serde_json::json!({
                    "error": error.to_string(),
                }),
            });
            crate::tools::mcp::config::McpServersFile::default()
        }
    };
    let mcp_enabled = mcp_servers.enabled_servers().count();
    if mcp_enabled > 0 {
        let preflight = crate::tools::mcp::health::probe_enabled_servers(
            mcp_servers.enabled_servers().cloned(),
            std::time::Duration::from_secs(2),
        )
        .await;
        let healthy = preflight.iter().all(|entry| entry.is_healthy());
        channels.push(ChannelStatusInfo {
            name: "mcp_servers".to_string(),
            configured: !mcp_servers.servers.is_empty(),
            enabled: true,
            healthy,
            status: if healthy {
                "healthy".to_string()
            } else {
                "degraded".to_string()
            },
            detail: serde_json::json!({
                "enabled_count": mcp_enabled,
                "configured_count": mcp_servers.servers.len(),
                "servers": preflight,
            }),
        });
    } else {
        channels.push(ChannelStatusInfo {
            name: "mcp_servers".to_string(),
            configured: !mcp_servers.servers.is_empty(),
            enabled: false,
            healthy: true,
            status: "disabled".to_string(),
            detail: serde_json::json!({
                "enabled_count": 0,
                "configured_count": mcp_servers.servers.len(),
            }),
        });
    }

    let overall = if channels.iter().any(|c| c.enabled && !c.healthy) {
        "degraded"
    } else {
        "healthy"
    };

    ChannelStatusResponse {
        generated_at: chrono::Utc::now().to_rfc3339(),
        status: overall.to_string(),
        channels,
    }
}

fn build_verification_status_response(
    settings: &crate::settings::Settings,
) -> VerificationStatusResponse {
    let v = &settings.verification_backend;
    let backend = v.backend.clone();

    let token_required = !matches!(v.eigencloud_auth_scheme.as_str(), "none");
    let primary_configured =
        v.eigencloud_endpoint.is_some() && (!token_required || v.eigencloud_auth_token.is_some());
    let backend_status = if backend == "fallback_only" {
        "fallback_only".to_string()
    } else if primary_configured {
        "configured".to_string()
    } else {
        "missing_config".to_string()
    };

    let fallback_signing_key_configured = v.fallback_signing_key_id.is_some();
    let fallback_chain_path = v
        .fallback_chain_path
        .as_ref()
        .map(|p| p.display().to_string());
    let fallback_status = if !v.fallback_enabled {
        "disabled".to_string()
    } else if v.fallback_require_signed_receipts && !fallback_signing_key_configured {
        "missing_signing_key".to_string()
    } else if fallback_chain_path.is_none() {
        "missing_chain_path".to_string()
    } else {
        "configured".to_string()
    };

    let overall = if backend == "fallback_only" {
        if v.fallback_enabled && fallback_status == "configured" {
            "healthy"
        } else {
            "degraded"
        }
    } else if backend_status == "configured" {
        "healthy"
    } else if v.fallback_enabled && fallback_status == "configured" {
        "degraded"
    } else {
        "failing"
    };

    VerificationStatusResponse {
        generated_at: chrono::Utc::now().to_rfc3339(),
        status: overall.to_string(),
        backend,
        backend_status,
        eigencloud: VerificationPrimarySurface {
            configured: primary_configured,
            endpoint: v.eigencloud_endpoint.clone(),
            auth_scheme: v.eigencloud_auth_scheme.clone(),
            token_configured: v.eigencloud_auth_token.is_some(),
            timeout_ms: v.eigencloud_timeout_ms,
            status: if primary_configured {
                "configured".to_string()
            } else {
                "missing_config".to_string()
            },
        },
        fallback: VerificationFallbackSurface {
            enabled: v.fallback_enabled,
            require_signed_receipts: v.fallback_require_signed_receipts,
            signing_key_configured: fallback_signing_key_configured,
            chain_path: fallback_chain_path,
            status: fallback_status,
        },
    }
}

async fn status_channels_handler(
    State(state): State<Arc<GatewayState>>,
) -> Json<ChannelStatusResponse> {
    let settings = load_effective_settings(&state).await;
    Json(build_channel_status_response(&state, &settings).await)
}

async fn status_verification_handler(
    State(state): State<Arc<GatewayState>>,
) -> Json<VerificationStatusResponse> {
    let settings = load_effective_settings(&state).await;
    Json(build_verification_status_response(&settings))
}

async fn gateway_status_handler(
    State(state): State<Arc<GatewayState>>,
) -> Json<GatewayStatusResponse> {
    let settings = load_effective_settings(&state).await;
    let channel_surface = build_channel_status_response(&state, &settings).await;
    let verification_surface = build_verification_status_response(&settings);
    let routine_webhook_status = if let Some(store) = state.store.as_ref() {
        match store.list_routines(&state.user_id).await {
            Ok(routines) => Some(build_routine_visibility_response(&routines).webhook.status),
            Err(e) => {
                tracing::warn!(
                    "Failed to build routine visibility for gateway status: {}",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    let sse_connections = state.sse.connection_count();
    let ws_connections = state
        .ws_tracker
        .as_ref()
        .map(|t| t.connection_count())
        .unwrap_or(0);

    Json(GatewayStatusResponse {
        sse_connections,
        ws_connections,
        total_connections: sse_connections + ws_connections,
        channel_status: channel_surface.status,
        verification_status: verification_surface.status,
        routine_webhook_status,
    })
}

#[derive(serde::Serialize)]
struct GatewayStatusResponse {
    sse_connections: u64,
    ws_connections: u64,
    total_connections: u64,
    channel_status: String,
    verification_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    routine_webhook_status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Uri;

    #[test]
    fn test_build_turns_from_db_messages_complete() {
        let now = chrono::Utc::now();
        let messages = vec![
            crate::history::ConversationMessage {
                id: Uuid::new_v4(),
                role: "user".to_string(),
                content: "Hello".to_string(),
                created_at: now,
            },
            crate::history::ConversationMessage {
                id: Uuid::new_v4(),
                role: "assistant".to_string(),
                content: "Hi there!".to_string(),
                created_at: now + chrono::TimeDelta::seconds(1),
            },
            crate::history::ConversationMessage {
                id: Uuid::new_v4(),
                role: "user".to_string(),
                content: "How are you?".to_string(),
                created_at: now + chrono::TimeDelta::seconds(2),
            },
            crate::history::ConversationMessage {
                id: Uuid::new_v4(),
                role: "assistant".to_string(),
                content: "Doing well!".to_string(),
                created_at: now + chrono::TimeDelta::seconds(3),
            },
        ];

        let turns = build_turns_from_db_messages(&messages);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].user_input, "Hello");
        assert_eq!(turns[0].response.as_deref(), Some("Hi there!"));
        assert_eq!(turns[0].state, "Completed");
        assert_eq!(turns[1].user_input, "How are you?");
        assert_eq!(turns[1].response.as_deref(), Some("Doing well!"));
    }

    #[test]
    fn test_build_turns_from_db_messages_incomplete_last() {
        let now = chrono::Utc::now();
        let messages = vec![
            crate::history::ConversationMessage {
                id: Uuid::new_v4(),
                role: "user".to_string(),
                content: "Hello".to_string(),
                created_at: now,
            },
            crate::history::ConversationMessage {
                id: Uuid::new_v4(),
                role: "assistant".to_string(),
                content: "Hi!".to_string(),
                created_at: now + chrono::TimeDelta::seconds(1),
            },
            crate::history::ConversationMessage {
                id: Uuid::new_v4(),
                role: "user".to_string(),
                content: "Lost message".to_string(),
                created_at: now + chrono::TimeDelta::seconds(2),
            },
        ];

        let turns = build_turns_from_db_messages(&messages);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].user_input, "Lost message");
        assert!(turns[1].response.is_none());
        assert_eq!(turns[1].state, "Failed");
    }

    #[test]
    fn test_build_turns_from_db_messages_empty() {
        let turns = build_turns_from_db_messages(&[]);
        assert!(turns.is_empty());
    }

    #[tokio::test]
    async fn test_legacy_gateway_redirect_preserves_query() {
        let uri: Uri = "/gateway?token=demo123".parse().expect("uri");
        let response = legacy_gateway_redirect_handler(OriginalUri(uri))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location value");
        assert_eq!(location, "/?token=demo123");
    }
}
