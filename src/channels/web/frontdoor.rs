//! Frontdoor orchestration for wallet-authenticated enclave provisioning.
//!
//! This service powers the user-facing "frontdoor" flow:
//! 1) create signing challenge
//! 2) verify signed intent payload
//! 3) provision per-user enclave through a configurable command
//! 4) return redirect URL for the dedicated instance

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;

use base64::Engine;
use chrono::{DateTime, Utc};
use k256::ecdsa::{RecoveryId, Signature as EcdsaSignature, VerifyingKey};
use rand::{Rng, distributions::Alphanumeric};
use sha3::{Digest, Keccak256};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::RwLock;
use url::Url;
use uuid::Uuid;

use crate::channels::web::types::{
    FrontdoorBootstrapResponse, FrontdoorChallengeRequest, FrontdoorChallengeResponse,
    FrontdoorConfigContractResponse, FrontdoorConfigDefaults, FrontdoorConfigEnums,
    FrontdoorDomainProfile, FrontdoorEvidenceLabel, FrontdoorExperienceManifestResponse,
    FrontdoorExperienceStep, FrontdoorFundingPreflightCheck, FrontdoorFundingPreflightResponse,
    FrontdoorGatewayTodoItem, FrontdoorGatewayTodosResponse, FrontdoorOnboardingChatRequest,
    FrontdoorOnboardingChatResponse, FrontdoorOnboardingRequiredVariable,
    FrontdoorOnboardingStateResponse, FrontdoorOnboardingStep2Payload,
    FrontdoorOnboardingStep3Payload, FrontdoorOnboardingStep4Payload,
    FrontdoorOnboardingTranscriptArtifactResponse, FrontdoorOnboardingTurn,
    FrontdoorPolicyTemplate, FrontdoorPolicyTemplateConfig, FrontdoorPolicyTemplateLibraryResponse,
    FrontdoorPolicyTemplateRiskProfile, FrontdoorRuntimeControlRequest,
    FrontdoorRuntimeControlResponse, FrontdoorSessionResponse, FrontdoorSessionSummaryResponse,
    FrontdoorSessionTimelineEvent, FrontdoorSessionTimelineResponse, FrontdoorSuggestConfigRequest,
    FrontdoorSuggestConfigResponse, FrontdoorTodoEvidenceRefs, FrontdoorUserConfig,
    FrontdoorVerificationExplanationResponse, FrontdoorVerifyRequest, FrontdoorVerifyResponse,
};

#[derive(Debug, Clone)]
pub struct FrontdoorConfig {
    pub require_privy: bool,
    pub privy_app_id: Option<String>,
    pub privy_client_id: Option<String>,
    pub provision_command: Option<String>,
    pub default_instance_url: Option<String>,
    pub allow_default_instance_fallback: bool,
    pub verify_app_base_url: Option<String>,
    pub session_ttl_secs: u64,
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct WalletSessionStore {
    wallets: HashMap<String, WalletSessionRecord>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WalletSessionRecord {
    version: u64,
    last_instance_url: String,
    last_profile_name: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
enum SessionStatus {
    AwaitingSignature,
    Provisioning,
    Ready,
    Failed,
    Expired,
}

impl SessionStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::AwaitingSignature => "awaiting_signature",
            Self::Provisioning => "provisioning",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ProvisioningSource {
    Unknown,
    Command,
    DefaultInstanceUrl,
    Unconfigured,
}

#[derive(Debug, Clone)]
struct ProvisioningDecision {
    prefer_shared_runtime: bool,
    reason: String,
}

#[derive(Debug, Clone, Copy)]
enum RuntimeState {
    Running,
    Paused,
    Terminated,
}

impl RuntimeState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Terminated => "terminated",
        }
    }
}

#[derive(Debug, Clone)]
struct OnboardingTurnState {
    role: String,
    message: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct OnboardingState {
    current_step: String,
    completed: bool,
    objective: Option<String>,
    missing_fields: Vec<String>,
    step2_payload: Option<FrontdoorOnboardingStep2Payload>,
    step3_payload: Option<FrontdoorOnboardingStep3Payload>,
    step4_payload: Option<FrontdoorOnboardingStep4Payload>,
    transcript_artifact_id: Option<String>,
    captured_variables: HashMap<String, String>,
    transcript: Vec<OnboardingTurnState>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct TimelineEvent {
    seq_id: u64,
    event_type: String,
    status: String,
    detail: String,
    actor: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct FundingPreflightCheckState {
    check_id: String,
    status: String,
    detail: String,
}

#[derive(Debug, Clone)]
struct FundingPreflightState {
    status: String,
    failure_category: Option<String>,
    checks: Vec<FundingPreflightCheckState>,
    updated_at: DateTime<Utc>,
}

impl ProvisioningSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Command => "command",
            Self::DefaultInstanceUrl => "default_instance_url",
            Self::Unconfigured => "unconfigured",
        }
    }

    fn dedicated_instance(self) -> bool {
        matches!(self, Self::Command)
    }
}

#[derive(Debug, Clone)]
struct ProvisioningSession {
    id: Uuid,
    wallet_address: String,
    privy_user_id: Option<String>,
    privy_identity_token: Option<String>,
    privy_access_token: Option<String>,
    chain_id: u64,
    message: String,
    version: u64,
    config: Option<FrontdoorUserConfig>,
    status: SessionStatus,
    detail: String,
    provisioning_source: ProvisioningSource,
    runtime_state: RuntimeState,
    instance_url: Option<String>,
    app_url: Option<String>,
    verify_url: Option<String>,
    eigen_app_id: Option<String>,
    signature_verification_latency_ms: Option<u64>,
    provisioning_started_at: Option<DateTime<Utc>>,
    onboarding: OnboardingState,
    timeline: Vec<TimelineEvent>,
    next_timeline_seq_id: u64,
    funding_preflight: FundingPreflightState,
    error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
struct FrontdoorState {
    sessions: HashMap<Uuid, ProvisioningSession>,
    wallets: HashMap<String, WalletSessionRecord>,
}

pub struct FrontdoorService {
    config: FrontdoorConfig,
    state: RwLock<FrontdoorState>,
    store_path: PathBuf,
}

const FRONTDOOR_CURRENT_CONFIG_VERSION: u32 = 2;
const FRONTDOOR_SUPPORTED_CONFIG_VERSIONS: [u32; 2] = [1, 2];
const ONBOARDING_GATEWAY_AUTH_FROM_CONFIG_MARKER: &str = "__from_config__";
const FRONTDOOR_TIMELINE_EVENT_CAP: usize = 1200;
const FRONTDOOR_SUPPORTED_DOMAINS: [&str; 8] = [
    "general",
    "developer",
    "creative",
    "research",
    "business_ops",
    "communications",
    "hyperliquid",
    "eigenda",
];

impl FrontdoorService {
    pub fn new(config: FrontdoorConfig) -> Arc<Self> {
        Self::new_with_store_path(config, default_wallet_store_path())
    }

    fn new_with_store_path(config: FrontdoorConfig, store_path: PathBuf) -> Arc<Self> {
        let wallets = load_wallet_store(&store_path).unwrap_or_default().wallets;
        Arc::new(Self {
            config,
            state: RwLock::new(FrontdoorState {
                sessions: HashMap::new(),
                wallets,
            }),
            store_path,
        })
    }

    #[cfg(test)]
    fn new_for_tests(config: FrontdoorConfig, store_path: PathBuf) -> Arc<Self> {
        Self::new_with_store_path(config, store_path)
    }

    pub fn bootstrap(&self) -> FrontdoorBootstrapResponse {
        let command_configured = parse_provision_command_template(
            self.config.provision_command.as_deref().unwrap_or_default(),
        )
        .is_ok();
        let default_url_configured =
            is_non_empty_config_value(self.config.default_instance_url.as_deref());
        let provisioning_backend = if command_configured {
            ProvisioningSource::Command.as_str().to_string()
        } else if self.config.allow_default_instance_fallback && default_url_configured {
            ProvisioningSource::DefaultInstanceUrl.as_str().to_string()
        } else {
            ProvisioningSource::Unconfigured.as_str().to_string()
        };

        FrontdoorBootstrapResponse {
            enabled: true,
            require_privy: self.config.require_privy,
            privy_app_id: self.config.privy_app_id.clone(),
            privy_client_id: self.config.privy_client_id.clone(),
            provisioning_backend,
            dynamic_provisioning_enabled: command_configured,
            default_instance_url_configured: default_url_configured,
            default_instance_fallback_enabled: self.config.allow_default_instance_fallback
                && default_url_configured,
            default_instance_looks_eigencloud: self
                .config
                .default_instance_url
                .as_deref()
                .is_some_and(looks_like_eigencloud_url),
            poll_interval_ms: self.config.poll_interval_ms,
            mandatory_steps: mandatory_frontdoor_steps(),
        }
    }

    pub fn config_contract(&self) -> FrontdoorConfigContractResponse {
        FrontdoorConfigContractResponse {
            contract_id: "enclagent.frontdoor.launchpad".to_string(),
            current_config_version: FRONTDOOR_CURRENT_CONFIG_VERSION,
            supported_config_versions: FRONTDOOR_SUPPORTED_CONFIG_VERSIONS.to_vec(),
            supported_domains: FRONTDOOR_SUPPORTED_DOMAINS
                .iter()
                .map(|domain| (*domain).to_string())
                .collect(),
            domain_profiles: frontdoor_domain_profiles(),
            mandatory_steps: mandatory_frontdoor_steps(),
            enums: FrontdoorConfigEnums {
                hyperliquid_network: vec!["testnet".to_string(), "mainnet".to_string()],
                paper_live_policy: vec![
                    "paper_only".to_string(),
                    "paper_first".to_string(),
                    "live_allowed".to_string(),
                ],
                custody_mode: vec![
                    "operator_wallet".to_string(),
                    "user_wallet".to_string(),
                    "dual_mode".to_string(),
                ],
                information_sharing_scope: vec![
                    "none".to_string(),
                    "signals_only".to_string(),
                    "signals_and_execution".to_string(),
                    "full_audit".to_string(),
                ],
                kill_switch_behavior: vec![
                    "pause_agent".to_string(),
                    "cancel_open_orders".to_string(),
                    "cancel_and_flatten".to_string(),
                ],
                verification_backend: vec![
                    "eigencloud_primary".to_string(),
                    "fallback_only".to_string(),
                ],
                verification_eigencloud_auth_scheme: vec![
                    "bearer".to_string(),
                    "api_key".to_string(),
                ],
            },
            defaults: FrontdoorConfigDefaults {
                profile_domain: "general".to_string(),
                hyperliquid_network: "testnet".to_string(),
                paper_live_policy: "paper_only".to_string(),
                request_timeout_ms: 10_000,
                max_retries: 3,
                retry_backoff_ms: 500,
                max_position_size_usd: 1_000,
                leverage_cap: 2,
                max_allocation_usd: 1_000,
                per_trade_notional_cap_usd: 100,
                max_leverage: 2,
                max_slippage_bps: 50,
                symbol_allowlist: vec!["BTC".to_string(), "ETH".to_string()],
                symbol_denylist: Vec::new(),
                custody_mode: "user_wallet".to_string(),
                information_sharing_scope: "signals_only".to_string(),
                kill_switch_enabled: true,
                kill_switch_behavior: "pause_agent".to_string(),
                enable_memory: true,
                verification_backend: "eigencloud_primary".to_string(),
                verification_eigencloud_auth_scheme: "bearer".to_string(),
                verification_eigencloud_timeout_ms: 5_000,
                verification_fallback_enabled: true,
                verification_fallback_require_signed_receipts: true,
            },
        }
    }

    pub fn policy_template_library(&self) -> FrontdoorPolicyTemplateLibraryResponse {
        FrontdoorPolicyTemplateLibraryResponse {
            generated_at: Utc::now().to_rfc3339(),
            templates: frontdoor_policy_templates(),
        }
    }

    pub fn suggest_config(
        &self,
        req: FrontdoorSuggestConfigRequest,
    ) -> Result<FrontdoorSuggestConfigResponse, String> {
        let connected_wallet = normalize_wallet_address(&req.wallet_address)
            .ok_or_else(|| "wallet_address must be a 0x-prefixed 40-hex address".to_string())?;
        let domain = normalize_domain_name(
            req.domain
                .as_deref()
                .unwrap_or("general")
                .trim()
                .to_ascii_lowercase()
                .as_str(),
        )?;

        let mut assumptions = Vec::new();
        let mut warnings = Vec::new();

        let mut config = req.base_config.unwrap_or_else(|| {
            default_frontdoor_user_config(
                &connected_wallet,
                req.gateway_auth_key.as_deref(),
                domain.as_str(),
            )
        });

        if config.gateway_auth_key.trim().is_empty() {
            config.gateway_auth_key = req
                .gateway_auth_key
                .as_deref()
                .filter(|v| is_valid_gateway_auth_key(v))
                .map(str::to_string)
                .unwrap_or_else(generate_gateway_auth_key);
        }

        config.profile_domain = domain.clone();
        config.accept_terms = true;

        apply_intent_overrides(
            &mut config,
            &req.intent,
            &connected_wallet,
            &mut assumptions,
            &mut warnings,
        );
        normalize_suggested_config(&mut config, &connected_wallet, &mut assumptions);
        config.inference_warnings = warnings.clone();
        validate_user_config(&config)?;
        validate_wallet_association(&config, &connected_wallet)?;

        Ok(FrontdoorSuggestConfigResponse {
            config,
            assumptions,
            warnings,
            validated: true,
        })
    }

    pub async fn create_challenge(
        &self,
        req: FrontdoorChallengeRequest,
    ) -> Result<FrontdoorChallengeResponse, String> {
        let wallet = normalize_wallet_address(&req.wallet_address)
            .ok_or_else(|| "wallet_address must be a 0x-prefixed 40-hex address".to_string())?;

        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);

        let version = state.wallets.get(&wallet).map_or(1, |r| r.version + 1);
        let session_id = Uuid::new_v4();
        let nonce = random_nonce(24);
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(self.config.session_ttl_secs as i64);
        let chain_id = req.chain_id.unwrap_or(1);
        let privy = req.privy_user_id.as_deref().unwrap_or("wallet_only");
        let message = format!(
            "Enclagent Gasless Authorization Transaction\nWallet: {wallet}\nPrivy Link: {privy}\nChain ID: {chain_id}\nSession ID: {session_id}\nVersion: v{version}\nNonce: {nonce}\nIssued At: {}\n\nSign this gasless authorization transaction to verify wallet control and start provisioning your dedicated Enclagent enclave.",
            now.to_rfc3339()
        );

        let mut session = ProvisioningSession {
            id: session_id,
            wallet_address: wallet.clone(),
            privy_user_id: req.privy_user_id,
            privy_identity_token: None,
            privy_access_token: None,
            chain_id,
            message: message.clone(),
            version,
            config: None,
            status: SessionStatus::AwaitingSignature,
            detail: "Waiting for gasless authorization signature.".to_string(),
            provisioning_source: ProvisioningSource::Unknown,
            runtime_state: RuntimeState::Running,
            instance_url: None,
            app_url: None,
            verify_url: None,
            eigen_app_id: None,
            signature_verification_latency_ms: None,
            provisioning_started_at: None,
            onboarding: default_onboarding_state(session_id, now),
            timeline: Vec::new(),
            next_timeline_seq_id: 1,
            funding_preflight: pending_funding_preflight(now),
            error: None,
            created_at: now,
            updated_at: now,
            expires_at,
        };
        push_timeline_event(
            &mut session,
            "challenge_created",
            "awaiting_signature",
            "Wallet challenge issued",
            "system",
        );
        let todo_snapshot = todo_status_summary(&build_gateway_todos(&session));
        push_timeline_event(
            &mut session,
            "todo_snapshot",
            "awaiting_signature",
            &todo_snapshot,
            "system",
        );
        self.persist_onboarding_transcript(&session)?;
        state.sessions.insert(session_id, session);

        Ok(FrontdoorChallengeResponse {
            session_id: session_id.to_string(),
            wallet_address: wallet,
            message,
            expires_at: expires_at.to_rfc3339(),
            version,
        })
    }

    pub async fn verify_and_start(
        self: Arc<Self>,
        req: FrontdoorVerifyRequest,
    ) -> Result<FrontdoorVerifyResponse, String> {
        let wallet = normalize_wallet_address(&req.wallet_address)
            .ok_or_else(|| "wallet_address must be a 0x-prefixed 40-hex address".to_string())?;
        if !is_signature_like(&req.signature) {
            return Err("signature must be a 65-byte hex string (0x-prefixed)".to_string());
        }
        validate_user_config(&req.config)?;
        validate_wallet_association(&req.config, &wallet)?;

        let session_id = Uuid::parse_str(req.session_id.trim())
            .map_err(|_| "session_id must be a valid UUID".to_string())?;

        {
            let mut state = self.state.write().await;
            purge_expired_sessions(&mut state);
            let session = state
                .sessions
                .get_mut(&session_id)
                .ok_or_else(|| "session not found".to_string())?;

            if session.wallet_address != wallet {
                return Err("wallet_address does not match challenge session".to_string());
            }
            if self.config.require_privy {
                let expected = session.privy_user_id.as_deref().unwrap_or("").trim();
                let provided = req.privy_user_id.as_deref().unwrap_or("").trim();
                if !expected.is_empty() && !provided.is_empty() && expected != provided {
                    return Err("privy_user_id mismatch".to_string());
                }
            }

            // Idempotency guard: once a session leaves awaiting_signature, do not
            // re-run signature verification/provisioning on repeated verify calls.
            if !matches!(session.status, SessionStatus::AwaitingSignature) {
                return Ok(FrontdoorVerifyResponse {
                    session_id: session_id.to_string(),
                    status: session.status.as_str().to_string(),
                    detail: session.detail.clone(),
                });
            }

            if session.expires_at < Utc::now() {
                session.status = SessionStatus::Expired;
                session.updated_at = Utc::now();
                session.detail =
                    "Challenge expired. Request a new signature challenge.".to_string();
                return Err("challenge expired".to_string());
            }

            if !message_matches(&req.message, &session.message) {
                return Err("signed message does not match challenge".to_string());
            }
            let signature_started = Instant::now();
            verify_wallet_signature(&req.message, &req.signature, &wallet)?;
            let signature_latency_ms = (signature_started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX))) as u64;

            session.config = Some(req.config.clone());
            session.privy_identity_token = req.privy_identity_token.clone();
            session.privy_access_token = req.privy_access_token.clone();
            session.signature_verification_latency_ms = Some(signature_latency_ms);
            session.onboarding.current_step = "ready_to_sign".to_string();
            session.onboarding.completed = true;
            if session.onboarding.objective.is_none() {
                session.onboarding.objective = req.config.inference_summary.clone();
            }
            session.onboarding.missing_fields.clear();
            session.onboarding.step4_payload = Some(FrontdoorOnboardingStep4Payload {
                ready_to_sign: true,
                confirmation_required: false,
                unresolved_required_fields: Vec::new(),
                signature_action: "Signature verification complete. Provisioning in progress."
                    .to_string(),
            });
            session.onboarding.updated_at = Utc::now();
            self.persist_onboarding_transcript(session)?;
            push_timeline_event(
                session,
                "signature_verified",
                "awaiting_signature",
                "Wallet signature verified",
                "system",
            );

            let preflight = evaluate_funding_preflight(session, &req.config);
            session.funding_preflight = preflight.clone();
            if preflight.status != "passed" {
                session.status = SessionStatus::Failed;
                session.updated_at = Utc::now();
                session.detail = format!(
                    "Funding preflight failed ({})",
                    preflight.failure_category.as_deref().unwrap_or("policy")
                );
                session.error = Some(format!(
                    "funding preflight failed: {}",
                    preflight.failure_category.as_deref().unwrap_or("policy")
                ));
                let detail = session.detail.clone();
                push_timeline_event(
                    session,
                    "funding_preflight_failed",
                    "failed",
                    &detail,
                    "system",
                );
                let summary = todo_status_summary(&build_gateway_todos(session));
                push_timeline_event(session, "todo_snapshot", "failed", &summary, "system");
                return Err(session
                    .error
                    .clone()
                    .unwrap_or_else(|| "funding preflight failed".to_string()));
            }
            push_timeline_event(
                session,
                "funding_preflight_passed",
                "awaiting_signature",
                "Funding preflight checks passed",
                "system",
            );

            let command_configured = self
                .config
                .provision_command
                .as_deref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            let default_fallback_ready = self.config.allow_default_instance_fallback
                && self
                    .config
                    .default_instance_url
                    .as_deref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false);
            let provisional_source = if command_configured {
                ProvisioningSource::Command
            } else if default_fallback_ready {
                ProvisioningSource::DefaultInstanceUrl
            } else {
                ProvisioningSource::Unconfigured
            };
            session.provisioning_source = provisional_source;

            session.status = SessionStatus::Provisioning;
            session.updated_at = Utc::now();
            session.error = None;
            session.detail = "Provisioning dedicated enclave...".to_string();
            session.provisioning_started_at = Some(Utc::now());
            push_timeline_event(
                session,
                "provisioning_decision",
                "provisioning",
                &format!(
                    "Provisioning backend selected: {} (dedicated={})",
                    provisional_source.as_str(),
                    provisional_source.dedicated_instance()
                ),
                "system",
            );
            push_timeline_event(
                session,
                "provisioning_started",
                "provisioning",
                "Provision command queued",
                "system",
            );
            let summary = todo_status_summary(&build_gateway_todos(session));
            push_timeline_event(session, "todo_snapshot", "provisioning", &summary, "system");
        }

        let svc = Arc::clone(&self);
        tokio::spawn(async move {
            svc.run_provision(session_id).await;
        });

        Ok(FrontdoorVerifyResponse {
            session_id: session_id.to_string(),
            status: "provisioning".to_string(),
            detail: "Provisioning started".to_string(),
        })
    }

    pub async fn get_session(&self, session_id: Uuid) -> Option<FrontdoorSessionResponse> {
        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);
        let session = state.sessions.get(&session_id)?;
        Some(render_session_response(session))
    }

    pub async fn list_sessions(
        &self,
        wallet_filter: Option<&str>,
        limit: usize,
    ) -> Result<(usize, Vec<FrontdoorSessionSummaryResponse>), String> {
        let normalized_wallet = match wallet_filter {
            Some(raw) => Some(normalize_wallet_address(raw).ok_or_else(|| {
                "wallet_address must be a 0x-prefixed 40-hex address".to_string()
            })?),
            None => None,
        };

        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);

        let mut filtered: Vec<FrontdoorSessionSummaryResponse> = state
            .sessions
            .values()
            .filter(|session| match normalized_wallet.as_ref() {
                Some(wallet) => &session.wallet_address == wallet,
                None => true,
            })
            .map(render_session_summary)
            .collect();
        filtered.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        let total = filtered.len();
        let capped_limit = limit.min(100);
        filtered.truncate(capped_limit);

        Ok((total, filtered))
    }

    pub async fn list_sessions_full(
        &self,
        wallet_filter: Option<&str>,
        limit: usize,
    ) -> Result<(usize, Vec<FrontdoorSessionResponse>), String> {
        let normalized_wallet = match wallet_filter {
            Some(raw) => Some(normalize_wallet_address(raw).ok_or_else(|| {
                "wallet_address must be a 0x-prefixed 40-hex address".to_string()
            })?),
            None => None,
        };

        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);

        let mut filtered: Vec<FrontdoorSessionResponse> = state
            .sessions
            .values()
            .filter(|session| match normalized_wallet.as_ref() {
                Some(wallet) => &session.wallet_address == wallet,
                None => true,
            })
            .map(render_session_response)
            .collect();
        filtered.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        let total = filtered.len();
        let capped_limit = limit.min(100);
        filtered.truncate(capped_limit);
        Ok((total, filtered))
    }

    pub fn experience_manifest(&self) -> FrontdoorExperienceManifestResponse {
        FrontdoorExperienceManifestResponse {
            manifest_version: 1,
            steps: vec![
                FrontdoorExperienceStep {
                    step_id: "identity".to_string(),
                    title: "Identity".to_string(),
                    purpose_id: "frontdoor.identity".to_string(),
                    user_value: "Bind wallet ownership before any privileged action.".to_string(),
                    backend_contract: "POST /api/frontdoor/challenge".to_string(),
                    artifact_binding: "challenge_message".to_string(),
                    state_inputs: vec!["wallet_address".to_string(), "chain_id".to_string()],
                    success_state: "awaiting_signature".to_string(),
                    failure_state: "challenge_rejected".to_string(),
                },
                FrontdoorExperienceStep {
                    step_id: "policy".to_string(),
                    title: "Policy".to_string(),
                    purpose_id: "frontdoor.policy".to_string(),
                    user_value: "Generate and validate launch policy from user intent.".to_string(),
                    backend_contract: "POST /api/frontdoor/suggest-config".to_string(),
                    artifact_binding: "frontdoor_user_config".to_string(),
                    state_inputs: vec![
                        "intent".to_string(),
                        "profile_domain".to_string(),
                        "gateway_auth_key".to_string(),
                    ],
                    success_state: "config_validated".to_string(),
                    failure_state: "config_invalid".to_string(),
                },
                FrontdoorExperienceStep {
                    step_id: "verification".to_string(),
                    title: "Verification".to_string(),
                    purpose_id: "frontdoor.verification".to_string(),
                    user_value: "Verify wallet signature and preflight readiness.".to_string(),
                    backend_contract: "POST /api/frontdoor/verify".to_string(),
                    artifact_binding: "signature_receipt".to_string(),
                    state_inputs: vec![
                        "session_id".to_string(),
                        "wallet_address".to_string(),
                        "signature".to_string(),
                        "config".to_string(),
                    ],
                    success_state: "provisioning".to_string(),
                    failure_state: "verification_failed".to_string(),
                },
                FrontdoorExperienceStep {
                    step_id: "provisioning".to_string(),
                    title: "Provisioning".to_string(),
                    purpose_id: "frontdoor.provisioning".to_string(),
                    user_value: "Launch and hand off to a dedicated runtime endpoint.".to_string(),
                    backend_contract: "GET /api/frontdoor/session/{session_id}".to_string(),
                    artifact_binding: "provisioning_receipt".to_string(),
                    state_inputs: vec!["session_id".to_string()],
                    success_state: "ready".to_string(),
                    failure_state: "failed".to_string(),
                },
            ],
            capabilities: vec![
                "wallet_signature_proof".to_string(),
                "typed_policy_config".to_string(),
                "session_timeline".to_string(),
                "gateway_todo_feed".to_string(),
                "runtime_controls".to_string(),
            ],
            constraints: vec![
                "wallet_binding_required".to_string(),
                "preflight_required_before_provisioning".to_string(),
                "fallback_receipt_policy_enforced".to_string(),
                "terminal_success_is_server_driven".to_string(),
            ],
            evidence_labels: vec![
                FrontdoorEvidenceLabel {
                    key: "intent".to_string(),
                    description: "User-provided objective and config intent.".to_string(),
                },
                FrontdoorEvidenceLabel {
                    key: "receipt".to_string(),
                    description: "Provisioning/session outcomes and status transitions."
                        .to_string(),
                },
                FrontdoorEvidenceLabel {
                    key: "verification".to_string(),
                    description: "Verification backend, level, fallback posture, and latency."
                        .to_string(),
                },
            ],
        }
    }

    pub async fn onboarding_state(
        &self,
        session_id: Uuid,
    ) -> Option<FrontdoorOnboardingStateResponse> {
        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);
        let session = state.sessions.get(&session_id)?;
        Some(render_onboarding_state(session))
    }

    pub async fn onboarding_transcript(
        &self,
        session_id: Uuid,
    ) -> Option<FrontdoorOnboardingTranscriptArtifactResponse> {
        {
            let mut state = self.state.write().await;
            purge_expired_sessions(&mut state);
            if let Some(session) = state.sessions.get(&session_id) {
                return Some(render_onboarding_transcript_artifact(session));
            }
        }

        let path = onboarding_transcript_path(&self.store_path, session_id);
        let data = std::fs::read(path).ok()?;
        serde_json::from_slice::<FrontdoorOnboardingTranscriptArtifactResponse>(&data).ok()
    }

    pub async fn onboarding_chat(
        &self,
        req: FrontdoorOnboardingChatRequest,
    ) -> Result<FrontdoorOnboardingChatResponse, String> {
        let session_id = Uuid::parse_str(req.session_id.trim())
            .map_err(|_| "session_id must be a valid UUID".to_string())?;
        let message = req.message.trim();
        if message.is_empty() {
            return Err("message must be non-empty".to_string());
        }

        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);
        let session = state
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| "session not found".to_string())?;

        let now = Utc::now();
        session.onboarding.transcript.push(OnboardingTurnState {
            role: "user".to_string(),
            message: message.to_string(),
            created_at: now,
        });

        let assistant_message = match session.onboarding.current_step.as_str() {
            "capture_objective" => {
                session.onboarding.objective = Some(message.to_string());
                session.onboarding.step2_payload = Some(build_onboarding_step2_payload(
                    &session.wallet_address,
                    message,
                ));
                session.onboarding.step3_payload = Some(build_onboarding_step3_payload(
                    &session.onboarding.captured_variables,
                ));
                session.onboarding.step4_payload = None;
                session.onboarding.current_step = "propose_plan".to_string();
                session.onboarding.completed = false;
                session.onboarding.missing_fields =
                    unresolved_required_fields(&session.onboarding.step3_payload);
                "Step 2 complete. Proposed config, risk posture, and module plan are generated. Reply 'confirm plan' to continue or provide a revised objective.".to_string()
            }
            "propose_plan" => {
                let lower = message.to_ascii_lowercase();
                if onboarding_plan_confirmed(&lower) {
                    session.onboarding.step3_payload = Some(build_onboarding_step3_payload(
                        &session.onboarding.captured_variables,
                    ));
                    session.onboarding.missing_fields =
                        unresolved_required_fields(&session.onboarding.step3_payload);
                    session.onboarding.current_step = "confirm_and_sign".to_string();
                    session.onboarding.step4_payload = Some(build_onboarding_step4_payload(
                        &session.onboarding.missing_fields,
                    ));
                    if !session.onboarding.missing_fields.is_empty() {
                        session.onboarding.current_step = "collect_required_variables".to_string();
                        format!(
                            "Step 3 required variables are still unresolved: {}. Provide key=value assignments.",
                            session.onboarding.missing_fields.join(", ")
                        )
                    } else {
                        "Step 4 ready. Reply 'confirm sign' to complete onboarding and proceed to signature verification.".to_string()
                    }
                } else {
                    session.onboarding.objective = Some(message.to_string());
                    session.onboarding.step2_payload = Some(build_onboarding_step2_payload(
                        &session.wallet_address,
                        message,
                    ));
                    session.onboarding.step3_payload = Some(build_onboarding_step3_payload(
                        &session.onboarding.captured_variables,
                    ));
                    session.onboarding.step4_payload = None;
                    session.onboarding.missing_fields =
                        unresolved_required_fields(&session.onboarding.step3_payload);
                    "Step 2 updated. Reply 'confirm plan' when the proposed policy and modules are acceptable.".to_string()
                }
            }
            "collect_required_variables" => {
                let assignments = parse_onboarding_assignments(message);
                for (field, value) in assignments {
                    session.onboarding.captured_variables.insert(field, value);
                }
                session.onboarding.step3_payload = Some(build_onboarding_step3_payload(
                    &session.onboarding.captured_variables,
                ));
                session.onboarding.missing_fields =
                    unresolved_required_fields(&session.onboarding.step3_payload);
                if session.onboarding.missing_fields.is_empty() {
                    session.onboarding.current_step = "confirm_and_sign".to_string();
                    session.onboarding.step4_payload = Some(build_onboarding_step4_payload(
                        &session.onboarding.missing_fields,
                    ));
                    "Step 3 complete. Reply 'confirm sign' to finalize onboarding and continue to wallet signature verification.".to_string()
                } else {
                    format!(
                        "Step 3 updated. Remaining required variables: {}.",
                        session.onboarding.missing_fields.join(", ")
                    )
                }
            }
            "confirm_and_sign" => {
                let lower = message.to_ascii_lowercase();
                if onboarding_signature_confirmed(&lower)
                    && session.onboarding.missing_fields.is_empty()
                {
                    session.onboarding.current_step = "ready_to_sign".to_string();
                    session.onboarding.completed = true;
                    session.onboarding.step4_payload = Some(FrontdoorOnboardingStep4Payload {
                        ready_to_sign: true,
                        confirmation_required: false,
                        unresolved_required_fields: Vec::new(),
                        signature_action:
                            "Submit POST /api/frontdoor/verify with challenge signature."
                                .to_string(),
                    });
                    "Onboarding complete. Submit signature verification to trigger provisioning."
                        .to_string()
                } else if !session.onboarding.missing_fields.is_empty() {
                    session.onboarding.current_step = "collect_required_variables".to_string();
                    session.onboarding.step4_payload = Some(build_onboarding_step4_payload(
                        &session.onboarding.missing_fields,
                    ));
                    format!(
                        "Cannot finalize yet. Missing required variables: {}.",
                        session.onboarding.missing_fields.join(", ")
                    )
                } else {
                    "Step 4 pending confirmation. Reply 'confirm sign' to complete onboarding."
                        .to_string()
                }
            }
            "ready_to_sign" => {
                session.onboarding.completed = true;
                "Onboarding already complete. Continue with signature verification.".to_string()
            }
            _ => "Onboarding state advanced. Continue to signature and provisioning.".to_string(),
        };

        session.onboarding.updated_at = now;
        session.onboarding.transcript.push(OnboardingTurnState {
            role: "assistant".to_string(),
            message: assistant_message.clone(),
            created_at: Utc::now(),
        });
        self.persist_onboarding_transcript(session)?;
        push_timeline_event(
            session,
            "onboarding_chat",
            session.status.as_str(),
            "Onboarding transcript updated",
            "user",
        );

        Ok(FrontdoorOnboardingChatResponse {
            session_id: session.id.to_string(),
            assistant_message,
            state: render_onboarding_state(session),
        })
    }

    pub async fn session_timeline(
        &self,
        session_id: Uuid,
    ) -> Option<FrontdoorSessionTimelineResponse> {
        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);
        let session = state.sessions.get(&session_id)?;
        Some(FrontdoorSessionTimelineResponse {
            session_id: session.id.to_string(),
            events: session
                .timeline
                .iter()
                .map(|event| FrontdoorSessionTimelineEvent {
                    seq_id: event.seq_id,
                    event_type: event.event_type.clone(),
                    status: event.status.clone(),
                    detail: event.detail.clone(),
                    actor: event.actor.clone(),
                    created_at: event.created_at.to_rfc3339(),
                })
                .collect(),
        })
    }

    pub async fn verification_explanation(
        &self,
        session_id: Uuid,
    ) -> Option<FrontdoorVerificationExplanationResponse> {
        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);
        let session = state.sessions.get(&session_id)?;
        let config = session.config.as_ref();
        Some(FrontdoorVerificationExplanationResponse {
            session_id: session.id.to_string(),
            backend: config
                .map(|c| c.verification_backend.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            level: verification_assurance_level(config),
            fallback_used: config
                .map(|c| c.verification_fallback_enabled)
                .unwrap_or(false),
            latency_ms: session.signature_verification_latency_ms.unwrap_or(0),
            failure_reason: session.error.clone(),
        })
    }

    pub async fn runtime_control(
        &self,
        session_id: Uuid,
        req: FrontdoorRuntimeControlRequest,
    ) -> Result<FrontdoorRuntimeControlResponse, String> {
        let action = req.action.trim().to_ascii_lowercase();
        if action.is_empty() {
            return Err("action is required".to_string());
        }
        let actor = req
            .actor
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("frontdoor_operator");

        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);
        let session = state
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| "session not found".to_string())?;

        let (status, detail) = match action.as_str() {
            "pause" => {
                if matches!(session.runtime_state, RuntimeState::Terminated) {
                    ("blocked", "Runtime already terminated")
                } else if matches!(session.runtime_state, RuntimeState::Paused) {
                    ("noop", "Runtime already paused")
                } else {
                    session.runtime_state = RuntimeState::Paused;
                    ("applied", "Runtime paused")
                }
            }
            "resume" => {
                if matches!(session.runtime_state, RuntimeState::Terminated) {
                    ("blocked", "Runtime already terminated")
                } else if matches!(session.runtime_state, RuntimeState::Running) {
                    ("noop", "Runtime already running")
                } else {
                    session.runtime_state = RuntimeState::Running;
                    ("applied", "Runtime resumed")
                }
            }
            "terminate" => {
                if matches!(session.runtime_state, RuntimeState::Terminated) {
                    ("noop", "Runtime already terminated")
                } else {
                    session.runtime_state = RuntimeState::Terminated;
                    ("applied", "Runtime terminated")
                }
            }
            "rotate_auth_key" => {
                if let Some(cfg) = session.config.as_mut() {
                    cfg.gateway_auth_key = generate_gateway_auth_key();
                    ("applied", "Gateway auth key rotated")
                } else {
                    ("blocked", "Session config not available")
                }
            }
            _ => {
                return Err(
                    "action must be one of: pause, resume, terminate, rotate_auth_key".to_string(),
                );
            }
        };

        session.updated_at = Utc::now();
        push_timeline_event(
            session,
            "runtime_control",
            session.status.as_str(),
            &format!("action={action}; status={status}; detail={detail}"),
            actor,
        );
        let summary = todo_status_summary(&build_gateway_todos(session));
        push_timeline_event(
            session,
            "todo_snapshot",
            session.status.as_str(),
            &summary,
            actor,
        );

        Ok(FrontdoorRuntimeControlResponse {
            session_id: session.id.to_string(),
            action,
            status: status.to_string(),
            runtime_state: session.runtime_state.as_str().to_string(),
            detail: detail.to_string(),
            updated_at: session.updated_at.to_rfc3339(),
        })
    }

    pub async fn gateway_todos_for_session(
        &self,
        session_id: Uuid,
    ) -> Option<FrontdoorGatewayTodosResponse> {
        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);
        let session = state.sessions.get(&session_id)?;
        Some(build_gateway_todos(session))
    }

    pub async fn gateway_todos(
        &self,
        wallet_filter: Option<&str>,
        session_id: Option<Uuid>,
        limit: usize,
    ) -> Result<(usize, Vec<FrontdoorGatewayTodosResponse>), String> {
        let normalized_wallet = match wallet_filter {
            Some(raw) => Some(normalize_wallet_address(raw).ok_or_else(|| {
                "wallet_address must be a 0x-prefixed 40-hex address".to_string()
            })?),
            None => None,
        };
        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);

        let mut filtered: Vec<FrontdoorGatewayTodosResponse> = state
            .sessions
            .values()
            .filter(|session| {
                if let Some(expected) = session_id
                    && session.id != expected
                {
                    return false;
                }
                if let Some(wallet) = normalized_wallet.as_ref()
                    && &session.wallet_address != wallet
                {
                    return false;
                }
                true
            })
            .map(build_gateway_todos)
            .collect();
        filtered.sort_by(|a, b| b.session_id.cmp(&a.session_id));
        let total = filtered.len();
        filtered.truncate(limit.min(100));
        Ok((total, filtered))
    }

    pub async fn funding_preflight(
        &self,
        session_id: Uuid,
    ) -> Option<FrontdoorFundingPreflightResponse> {
        let mut state = self.state.write().await;
        purge_expired_sessions(&mut state);
        let session = state.sessions.get_mut(&session_id)?;
        if session.funding_preflight.status == "pending"
            && let Some(cfg) = session.config.clone()
        {
            session.funding_preflight = evaluate_funding_preflight(session, &cfg);
        }
        Some(render_funding_preflight_response(session))
    }

    async fn emit_provision_log(&self, session_id: Uuid, entry: &ProvisionCommandLog) {
        let line = entry.line.trim();
        if line.is_empty() {
            return;
        }
        let mut state = self.state.write().await;
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return;
        };
        let status = session.status.as_str().to_string();
        let detail = format!("[{}/{}] {}", entry.source, entry.stream, line);
        push_timeline_event(session, "provision_log", &status, &detail, "provisioner");
        session.updated_at = Utc::now();
    }

    async fn run_provision(self: Arc<Self>, session_id: Uuid) {
        let (
            wallet,
            privy_user,
            privy_identity_token,
            privy_access_token,
            chain_id,
            version,
            cfg,
            onboarding_objective,
            command,
            default_url,
            allow_default_fallback,
            verify_base_url,
        ) = {
            let state = self.state.read().await;
            let Some(session) = state.sessions.get(&session_id) else {
                return;
            };
            let Some(cfg) = session.config.clone() else {
                return;
            };
            (
                session.wallet_address.clone(),
                session.privy_user_id.clone(),
                session.privy_identity_token.clone(),
                session.privy_access_token.clone(),
                session.chain_id,
                session.version,
                cfg,
                session.onboarding.objective.clone(),
                self.config.provision_command.clone(),
                self.config.default_instance_url.clone(),
                self.config.allow_default_instance_fallback,
                self.config.verify_app_base_url.clone(),
            )
        };

        let normalized_default_url = normalize_default_instance_url(default_url.as_deref());
        let default_fallback_ready = matches!(&normalized_default_url, Ok(Some(_)));
        let provisioning_decision = decide_provisioning_decision(
            onboarding_objective
                .as_deref()
                .or(cfg.inference_summary.as_deref()),
        );
        let command_input = ProvisionCommandInput {
            session_id,
            wallet: &wallet,
            privy_user_id: privy_user.as_deref(),
            privy_identity_token: privy_identity_token.as_deref(),
            privy_access_token: privy_access_token.as_deref(),
            chain_id,
            version,
            config: &cfg,
            verify_base_url: verify_base_url.as_deref(),
        };
        let shared_fallback_allowed = allow_default_fallback && default_fallback_ready;
        let (result, provisioning_source, decision_detail) = if provisioning_decision
            .prefer_shared_runtime
            && shared_fallback_allowed
        {
            (
                provision_from_default_url(&normalized_default_url),
                ProvisioningSource::DefaultInstanceUrl,
                format!(
                    "{} Using shared fallback runtime instead of spawning a dedicated enclave.",
                    provisioning_decision.reason
                ),
            )
        } else {
            let decision_detail = if provisioning_decision.prefer_shared_runtime {
                format!(
                    "{} Shared fallback runtime is unavailable; continuing with configured provisioning backend.",
                    provisioning_decision.reason
                )
            } else {
                provisioning_decision.reason.clone()
            };
            let (result, source) = match command.as_deref().map(str::trim) {
                Some(raw_template) if !raw_template.is_empty() => {
                    match parse_provision_command_template(raw_template) {
                        Ok(parsed_template) => {
                            let service = Arc::clone(&self);
                            (
                                execute_provision_command_with_stream(
                                    parsed_template.as_str(),
                                    &command_input,
                                    move |entry| {
                                        let service = Arc::clone(&service);
                                        async move {
                                            service.emit_provision_log(session_id, &entry).await;
                                        }
                                    },
                                )
                                .await,
                                ProvisioningSource::Command,
                            )
                        }
                        Err(_template_err)
                            if allow_default_fallback && normalized_default_url.is_ok() =>
                        {
                            (
                                provision_from_default_url(&normalized_default_url),
                                ProvisioningSource::DefaultInstanceUrl,
                            )
                        }
                        Err(template_err) => (
                            Err(format!("provision_command is malformed: {template_err}")),
                            ProvisioningSource::Unconfigured,
                        ),
                    }
                }
                _ if allow_default_fallback && normalized_default_url.is_ok() => (
                    provision_from_default_url(&normalized_default_url),
                    ProvisioningSource::DefaultInstanceUrl,
                ),
                _ => (
                    Err("No valid provisioning command configured. Static fallback is disabled; set GATEWAY_FRONTDOOR_PROVISION_COMMAND or opt in to GATEWAY_FRONTDOOR_ALLOW_DEFAULT_INSTANCE_FALLBACK=1 with GATEWAY_FRONTDOOR_DEFAULT_INSTANCE_URL.".to_string()),
                    ProvisioningSource::Unconfigured,
                ),
            };
            (result, source, decision_detail)
        };

        let mut state = self.state.write().await;
        let mut wallet_record: Option<WalletSessionRecord> = None;
        {
            let Some(session) = state.sessions.get_mut(&session_id) else {
                return;
            };
            session.provisioning_source = provisioning_source;
            push_timeline_event(
                session,
                "provisioning_decision",
                session.status.as_str(),
                &decision_detail,
                "system",
            );

            match result {
                Ok(provisioned) => {
                    session.status = SessionStatus::Ready;
                    session.instance_url = Some(provisioned.instance_url.clone());
                    session.app_url = provisioned.app_url.clone();
                    session.verify_url = provisioned.verify_url.clone();
                    session.eigen_app_id = provisioned.eigen_app_id.clone();
                    session.error = None;
                    session.detail = "Enclave is live. Redirect ready.".to_string();
                    session.updated_at = Utc::now();
                    session.provisioning_started_at = None;

                    tracing::info!(
                        session_id = %session.id,
                        wallet = %session.wallet_address,
                        verification_backend = %session
                            .config
                            .as_ref()
                            .map(|cfg| cfg.verification_backend.as_str())
                            .unwrap_or("unknown"),
                        provisioning_source = provisioning_source.as_str(),
                        verification_level = %verification_assurance_level(session.config.as_ref()),
                        eigen_app_id = ?session.eigen_app_id,
                        verify_url = ?session.verify_url,
                        launched_on_eigencloud = ?session_launched_on_eigencloud(session),
                        "Frontdoor provisioning completed"
                    );
                    push_timeline_event(
                        session,
                        "provisioning_completed",
                        "ready",
                        "Provisioning completed and instance URL available",
                        "system",
                    );
                    let summary = todo_status_summary(&build_gateway_todos(session));
                    push_timeline_event(session, "todo_snapshot", "ready", &summary, "system");

                    wallet_record = Some(WalletSessionRecord {
                        version: session.version,
                        last_instance_url: provisioned.instance_url,
                        last_profile_name: cfg.profile_name.clone(),
                        updated_at: Utc::now().to_rfc3339(),
                    });
                }
                Err(err) => {
                    session.status = SessionStatus::Failed;
                    session.detail = "Provisioning failed".to_string();
                    session.error = Some(err);
                    session.updated_at = Utc::now();
                    session.provisioning_started_at = None;

                    tracing::warn!(
                        session_id = %session.id,
                        wallet = %session.wallet_address,
                        verification_backend = %session
                            .config
                            .as_ref()
                            .map(|cfg| cfg.verification_backend.as_str())
                            .unwrap_or("unknown"),
                        provisioning_source = provisioning_source.as_str(),
                        verification_level = %verification_assurance_level(session.config.as_ref()),
                        eigen_app_id = ?session.eigen_app_id,
                        error = ?session.error,
                        "Frontdoor provisioning failed"
                    );
                    let error_detail = session
                        .error
                        .clone()
                        .unwrap_or_else(|| "Provisioning command failed".to_string());
                    push_timeline_event(
                        session,
                        "provisioning_failed",
                        "failed",
                        &error_detail,
                        "system",
                    );
                    let summary = todo_status_summary(&build_gateway_todos(session));
                    push_timeline_event(session, "todo_snapshot", "failed", &summary, "system");
                }
            }
        }

        if let Some(record) = wallet_record {
            state.wallets.insert(wallet.clone(), record);
            let store = WalletSessionStore {
                wallets: state.wallets.clone(),
            };
            if let Err(err) = persist_wallet_store(&self.store_path, &store) {
                tracing::warn!(
                    session_id = %session_id,
                    wallet = %wallet,
                    verification_backend = %cfg.verification_backend,
                    verification_level = %verification_assurance_level(Some(&cfg)),
                    provisioning_source = "wallet_store",
                    eigen_app_id = ?None::<String>,
                    error = %err,
                    "Failed to persist frontdoor wallet session store"
                );
            }
        }
    }

    fn persist_onboarding_transcript(&self, session: &ProvisioningSession) -> Result<(), String> {
        let artifact = render_onboarding_transcript_artifact(session);
        let path = onboarding_transcript_path(&self.store_path, session.id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed creating onboarding artifact dir: {e}"))?;
        }
        let data = serde_json::to_vec_pretty(&artifact)
            .map_err(|e| format!("failed serializing onboarding transcript artifact: {e}"))?;
        std::fs::write(path, data)
            .map_err(|e| format!("failed writing onboarding transcript artifact: {e}"))?;
        Ok(())
    }
}

fn default_wallet_store_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".enclagent")
        .join("frontdoor")
        .join("wallet_sessions.json")
}

fn onboarding_artifact_id(session_id: Uuid) -> String {
    format!("frontdoor.onboarding_transcript.{session_id}")
}

fn onboarding_transcript_path(store_path: &std::path::Path, session_id: Uuid) -> PathBuf {
    store_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("onboarding")
        .join(format!("{session_id}.json"))
}

#[derive(Debug, Clone)]
struct ProvisioningResult {
    instance_url: String,
    app_url: Option<String>,
    verify_url: Option<String>,
    eigen_app_id: Option<String>,
}

fn normalize_default_instance_url(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(candidate) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    validate_optional_url(candidate, &["http", "https"], "default_instance_url")?;
    Ok(Some(candidate.to_string()))
}

fn provision_from_default_url(
    normalized_default_url: &Result<Option<String>, String>,
) -> Result<ProvisioningResult, String> {
    match normalized_default_url {
        Ok(Some(url)) => Ok(ProvisioningResult {
            instance_url: url.clone(),
            app_url: None,
            verify_url: if looks_like_verify_url(url) {
                Some(url.clone())
            } else {
                None
            },
            eigen_app_id: None,
        }),
        Ok(None) => Err("default_instance_url fallback requested but not configured".to_string()),
        Err(err) => Err(format!("default_instance_url is invalid: {err}")),
    }
}

fn parse_provision_command_template(template: &str) -> Result<String, String> {
    let trimmed = template.trim();
    if trimmed.is_empty() {
        return Err("provision command template is empty".to_string());
    }

    let allowed_placeholders: HashSet<&str> = HashSet::from([
        "session_id",
        "wallet_address",
        "privy_user_id",
        "privy_identity_token",
        "privy_access_token",
        "chain_id",
        "version",
        "config_version",
        "profile_domain",
        "domain_overrides_json",
        "profile_name",
        "custody_mode",
        "operator_wallet_address",
        "user_wallet_address",
        "vault_address",
        "gateway_auth_key",
        "eigencloud_auth_key",
        "verification_backend",
        "verification_eigencloud_endpoint",
        "verification_eigencloud_auth_scheme",
        "verification_eigencloud_timeout_ms",
        "verification_fallback_enabled",
        "verification_fallback_signing_key_id",
        "verification_fallback_chain_path",
        "verification_fallback_require_signed_receipts",
        "verify_app_base_url",
        "inference_summary",
        "inference_confidence",
        "config_json",
        "config_b64",
    ]);

    let bytes = trimmed.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] != b'{' || (idx > 0 && bytes[idx - 1] == b'$') {
            idx = idx.saturating_add(1);
            continue;
        }
        let mut end = idx.saturating_add(1);
        while end < bytes.len() && bytes[end] != b'}' {
            end = end.saturating_add(1);
        }
        if end >= bytes.len() {
            return Err("provision command template has unmatched '{'".to_string());
        }
        let token = &trimmed[idx + 1..end];
        if !token.is_empty()
            && token
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && !allowed_placeholders.contains(token)
        {
            return Err(format!("unsupported placeholder '{{{token}}}'"));
        }
        idx = end.saturating_add(1);
    }

    Ok(trimmed.to_string())
}

fn is_non_empty_config_value(value: Option<&str>) -> bool {
    value.map(|v| !v.trim().is_empty()).unwrap_or(false)
}

fn decide_provisioning_decision(objective_hint: Option<&str>) -> ProvisioningDecision {
    let Some(objective) = objective_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return ProvisioningDecision {
            prefer_shared_runtime: false,
            reason:
                "No explicit objective requested shared runtime; defaulting to dedicated provisioning."
                    .to_string(),
        };
    };

    let lower = objective.to_ascii_lowercase();
    let shared_runtime_hints = [
        "shared runtime",
        "shared gateway",
        "reuse existing runtime",
        "reuse existing instance",
        "no enclave",
        "without enclave",
        "skip enclave",
        "skip provisioning",
        "dont provision",
        "don't provision",
        "dont spin up",
        "don't spin up",
        "no spinup",
        "local only",
        "dry run",
        "test only",
    ];

    if shared_runtime_hints.iter().any(|hint| lower.contains(hint)) {
        return ProvisioningDecision {
            prefer_shared_runtime: true,
            reason: format!(
                "Objective requests shared runtime semantics ({objective}); dedicated spin-up is optional."
            ),
        };
    }

    ProvisioningDecision {
        prefer_shared_runtime: false,
        reason: format!(
            "Objective implies dedicated runtime isolation ({objective}); dedicated spin-up remains enabled."
        ),
    }
}

fn verification_assurance_level(config: Option<&FrontdoorUserConfig>) -> String {
    let Some(config) = config else {
        return "unknown".to_string();
    };

    match config.verification_backend.as_str() {
        "eigencloud_primary" => {
            if config.verification_fallback_enabled {
                if config.verification_fallback_require_signed_receipts {
                    "primary_plus_signed_fallback".to_string()
                } else {
                    "primary_plus_unsigned_fallback".to_string()
                }
            } else {
                "primary_only".to_string()
            }
        }
        "fallback_only" => {
            if !config.verification_fallback_enabled {
                "fallback_only_disabled".to_string()
            } else if config.verification_fallback_require_signed_receipts {
                "signed_fallback_only".to_string()
            } else {
                "unsigned_fallback_only".to_string()
            }
        }
        other => format!("custom:{other}"),
    }
}

fn default_onboarding_state(session_id: Uuid, now: DateTime<Utc>) -> OnboardingState {
    OnboardingState {
        current_step: "capture_objective".to_string(),
        completed: false,
        objective: None,
        missing_fields: vec![
            "profile_name".to_string(),
            "gateway_auth_key".to_string(),
            "accept_terms".to_string(),
        ],
        step2_payload: None,
        step3_payload: Some(build_onboarding_step3_payload(&HashMap::new())),
        step4_payload: Some(build_onboarding_step4_payload(&[
            "profile_name".to_string(),
            "gateway_auth_key".to_string(),
            "accept_terms".to_string(),
        ])),
        transcript_artifact_id: Some(onboarding_artifact_id(session_id)),
        captured_variables: HashMap::new(),
        transcript: vec![OnboardingTurnState {
            role: "assistant".to_string(),
            message:
                "Describe your objective. The frontdoor flow will produce a validated launch plan."
                    .to_string(),
            created_at: now,
        }],
        created_at: now,
        updated_at: now,
    }
}

fn build_onboarding_step2_payload(
    connected_wallet: &str,
    objective: &str,
) -> FrontdoorOnboardingStep2Payload {
    let mut assumptions = Vec::new();
    let mut warnings = Vec::new();
    let mut config = default_frontdoor_user_config(connected_wallet, None, "general");
    apply_intent_overrides(
        &mut config,
        objective,
        connected_wallet,
        &mut assumptions,
        &mut warnings,
    );
    normalize_suggested_config(&mut config, connected_wallet, &mut assumptions);
    let module_plan = frontdoor_domain_profiles()
        .into_iter()
        .find(|profile| profile.domain == config.profile_domain)
        .map(|profile| profile.default_modules)
        .unwrap_or_else(|| vec!["general".to_string()]);
    let warnings_suffix = if warnings.is_empty() {
        String::new()
    } else {
        format!("; warnings={}", warnings.join(" | "))
    };

    FrontdoorOnboardingStep2Payload {
        objective: objective.trim().to_string(),
        proposed_profile_domain: config.profile_domain.clone(),
        proposed_profile_name: config.profile_name.clone(),
        proposed_module_plan: module_plan,
        proposed_verification_backend: config.verification_backend.clone(),
        proposed_custody_mode: config.custody_mode.clone(),
        risk_summary: format!(
            "paper_live_policy={}; max_position_size_usd={}; max_leverage={}; fallback_signed_receipts={}{warnings_suffix}",
            config.paper_live_policy,
            config.max_position_size_usd,
            config.max_leverage,
            config.verification_fallback_require_signed_receipts
        ),
    }
}

fn build_onboarding_step3_payload(
    captured_variables: &HashMap<String, String>,
) -> FrontdoorOnboardingStep3Payload {
    let mut required_variables = Vec::new();
    for (field, rationale) in [
        (
            "profile_name",
            "Stable profile identifier used in policy and receipt records.",
        ),
        (
            "gateway_auth_key",
            "Gateway auth key secures runtime API access after launch.",
        ),
        (
            "accept_terms",
            "Policy gates require explicit terms acceptance before provisioning.",
        ),
    ] {
        let value = captured_variables
            .get(field)
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        let status = match field {
            "profile_name" => {
                if value.is_empty() {
                    "open"
                } else if value.len() <= 64 {
                    "resolved"
                } else {
                    "blocked"
                }
            }
            "gateway_auth_key" => {
                if value.is_empty() {
                    "open"
                } else if onboarding_gateway_auth_key_supplied(&value) {
                    "resolved"
                } else {
                    "blocked"
                }
            }
            "accept_terms" => {
                if value.is_empty() {
                    "open"
                } else if matches!(value.to_ascii_lowercase().as_str(), "true" | "1" | "yes") {
                    "resolved"
                } else {
                    "blocked"
                }
            }
            _ => "open",
        };
        required_variables.push(FrontdoorOnboardingRequiredVariable {
            field: field.to_string(),
            required: true,
            status: status.to_string(),
            rationale: rationale.to_string(),
        });
    }

    let unresolved_required_count = required_variables
        .iter()
        .filter(|variable| variable.required && variable.status != "resolved")
        .count();
    let validation_status = if unresolved_required_count == 0 {
        "ready_for_confirmation"
    } else {
        "missing_required_variables"
    };

    FrontdoorOnboardingStep3Payload {
        required_variables,
        unresolved_required_count,
        validation_status: validation_status.to_string(),
        rationale: "Populate all required variables to continue to signature confirmation."
            .to_string(),
    }
}

fn build_onboarding_step4_payload(
    unresolved_required_fields: &[String],
) -> FrontdoorOnboardingStep4Payload {
    FrontdoorOnboardingStep4Payload {
        ready_to_sign: unresolved_required_fields.is_empty(),
        confirmation_required: true,
        unresolved_required_fields: unresolved_required_fields.to_vec(),
        signature_action:
            "Reply 'confirm sign' to finalize onboarding, then call POST /api/frontdoor/verify."
                .to_string(),
    }
}

fn unresolved_required_fields(payload: &Option<FrontdoorOnboardingStep3Payload>) -> Vec<String> {
    payload
        .as_ref()
        .map(|step| {
            step.required_variables
                .iter()
                .filter(|variable| variable.required && variable.status != "resolved")
                .map(|variable| variable.field.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn parse_onboarding_assignments(message: &str) -> HashMap<String, String> {
    let mut assignments = HashMap::new();
    let normalized_message = message.trim();
    if normalized_message.is_empty() {
        return assignments;
    }

    let lower = normalized_message.to_ascii_lowercase();
    if lower.contains("accept terms") || lower.contains("i accept") {
        assignments.insert("accept_terms".to_string(), "true".to_string());
    }

    for segment in normalized_message.split(['\n', ',', ';']) {
        let item = segment.trim();
        if item.is_empty() {
            continue;
        }
        let pair = item
            .split_once('=')
            .or_else(|| item.split_once(':'))
            .map(|(key, value)| (normalize_onboarding_field(key), value.trim().to_string()));
        if let Some((field, value)) = pair
            && !field.is_empty()
            && !value.is_empty()
        {
            assignments.insert(field, value);
        }
    }

    assignments
}

fn normalize_onboarding_field(raw: &str) -> String {
    let field = raw
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
        .replace("__", "_");
    match field.as_str() {
        "profile" | "name" | "profile_name" => "profile_name".to_string(),
        "gateway_key" | "auth_key" | "gateway_auth" | "gateway_auth_key" => {
            "gateway_auth_key".to_string()
        }
        "terms" | "accept" | "accept_terms" => "accept_terms".to_string(),
        other => other.to_string(),
    }
}

fn onboarding_plan_confirmed(message_lower: &str) -> bool {
    message_lower.contains("confirm plan")
        || message_lower.contains("approve plan")
        || message_lower.contains("proceed plan")
        || message_lower.trim() == "confirm"
}

fn onboarding_signature_confirmed(message_lower: &str) -> bool {
    message_lower.contains("confirm sign")
        || message_lower.contains("confirm signature")
        || message_lower.contains("proceed to sign")
        || message_lower.trim() == "confirm"
}

fn pending_funding_preflight(now: DateTime<Utc>) -> FundingPreflightState {
    FundingPreflightState {
        status: "pending".to_string(),
        failure_category: None,
        checks: vec![
            FundingPreflightCheckState {
                check_id: "wallet_binding".to_string(),
                status: "pending".to_string(),
                detail: "Waiting for verified wallet association.".to_string(),
            },
            FundingPreflightCheckState {
                check_id: "auth_material".to_string(),
                status: "pending".to_string(),
                detail: "Waiting for verification backend auth validation.".to_string(),
            },
            FundingPreflightCheckState {
                check_id: "gas_budget".to_string(),
                status: "pending".to_string(),
                detail: "Waiting for deterministic gas readiness checks.".to_string(),
            },
            FundingPreflightCheckState {
                check_id: "platform_fee".to_string(),
                status: "pending".to_string(),
                detail: "Waiting for deterministic platform-fee checks.".to_string(),
            },
        ],
        updated_at: now,
    }
}

fn push_timeline_event(
    session: &mut ProvisioningSession,
    event_type: &str,
    status: &str,
    detail: &str,
    actor: &str,
) {
    let seq_id = session.next_timeline_seq_id;
    session.next_timeline_seq_id = session.next_timeline_seq_id.saturating_add(1);
    session.timeline.push(TimelineEvent {
        seq_id,
        event_type: event_type.to_string(),
        status: status.to_string(),
        detail: detail.to_string(),
        actor: actor.to_string(),
        created_at: Utc::now(),
    });
    if session.timeline.len() > FRONTDOOR_TIMELINE_EVENT_CAP {
        let overflow = session.timeline.len() - FRONTDOOR_TIMELINE_EVENT_CAP;
        session.timeline.drain(0..overflow);
    }
    tracing::info!(
        session_id = %session.id,
        wallet = %session.wallet_address,
        verification_backend = %session
            .config
            .as_ref()
            .map(|cfg| cfg.verification_backend.as_str())
            .unwrap_or("unknown"),
        verification_level = %verification_assurance_level(session.config.as_ref()),
        provisioning_source = %session.provisioning_source.as_str(),
        eigen_app_id = ?session.eigen_app_id,
        event_type = event_type,
        event_status = status,
        actor = actor,
        detail = detail,
        "Frontdoor timeline event"
    );
}

fn render_onboarding_state(session: &ProvisioningSession) -> FrontdoorOnboardingStateResponse {
    FrontdoorOnboardingStateResponse {
        session_id: session.id.to_string(),
        current_step: session.onboarding.current_step.clone(),
        completed: session.onboarding.completed,
        objective: session.onboarding.objective.clone(),
        missing_fields: session.onboarding.missing_fields.clone(),
        step2_payload: session.onboarding.step2_payload.clone(),
        step3_payload: session.onboarding.step3_payload.clone(),
        step4_payload: session.onboarding.step4_payload.clone(),
        transcript_artifact_id: session.onboarding.transcript_artifact_id.clone(),
        transcript: session
            .onboarding
            .transcript
            .iter()
            .map(|turn| FrontdoorOnboardingTurn {
                role: turn.role.clone(),
                message: turn.message.clone(),
                created_at: turn.created_at.to_rfc3339(),
            })
            .collect(),
        updated_at: session.onboarding.updated_at.to_rfc3339(),
    }
}

fn render_onboarding_transcript_artifact(
    session: &ProvisioningSession,
) -> FrontdoorOnboardingTranscriptArtifactResponse {
    FrontdoorOnboardingTranscriptArtifactResponse {
        artifact_id: session
            .onboarding
            .transcript_artifact_id
            .clone()
            .unwrap_or_else(|| onboarding_artifact_id(session.id)),
        session_id: session.id.to_string(),
        wallet_address: session.wallet_address.clone(),
        current_step: session.onboarding.current_step.clone(),
        completed: session.onboarding.completed,
        objective: session.onboarding.objective.clone(),
        step2_payload: session.onboarding.step2_payload.clone(),
        step3_payload: session.onboarding.step3_payload.clone(),
        step4_payload: session.onboarding.step4_payload.clone(),
        transcript: session
            .onboarding
            .transcript
            .iter()
            .map(|turn| FrontdoorOnboardingTurn {
                role: turn.role.clone(),
                message: turn.message.clone(),
                created_at: turn.created_at.to_rfc3339(),
            })
            .collect(),
        created_at: session.onboarding.created_at.to_rfc3339(),
        updated_at: session.onboarding.updated_at.to_rfc3339(),
    }
}

fn render_funding_preflight_response(
    session: &ProvisioningSession,
) -> FrontdoorFundingPreflightResponse {
    FrontdoorFundingPreflightResponse {
        session_id: session.id.to_string(),
        status: session.funding_preflight.status.clone(),
        failure_category: session.funding_preflight.failure_category.clone(),
        checks: session
            .funding_preflight
            .checks
            .iter()
            .map(|check| FrontdoorFundingPreflightCheck {
                check_id: check.check_id.clone(),
                status: check.status.clone(),
                detail: check.detail.clone(),
            })
            .collect(),
        updated_at: session.funding_preflight.updated_at.to_rfc3339(),
    }
}

fn build_gateway_todos(session: &ProvisioningSession) -> FrontdoorGatewayTodosResponse {
    let verification_level = verification_assurance_level(session.config.as_ref());
    let provisioning_source = session.provisioning_source.as_str().to_string();
    let module_state = session
        .config
        .as_ref()
        .map(|cfg| format!("profile_domain:{}", cfg.profile_domain))
        .unwrap_or_else(|| "profile_domain:unknown".to_string());
    let control_state = session.runtime_state.as_str().to_string();
    let session_id = session.id.to_string();

    let signature_status = match session.status {
        SessionStatus::AwaitingSignature => "open",
        SessionStatus::Provisioning | SessionStatus::Ready => "resolved",
        SessionStatus::Failed | SessionStatus::Expired => {
            if session.signature_verification_latency_ms.is_some() {
                "resolved"
            } else {
                "blocked"
            }
        }
    };
    let funding_status = match session.funding_preflight.status.as_str() {
        "passed" => "resolved",
        "failed" => "blocked",
        "pending" => {
            if matches!(session.status, SessionStatus::Provisioning) {
                "in_progress"
            } else {
                "open"
            }
        }
        _ => "open",
    };
    let provisioning_status = match session.status {
        SessionStatus::Ready
            if matches!(session.provisioning_source, ProvisioningSource::Command) =>
        {
            "resolved"
        }
        SessionStatus::Provisioning => "in_progress",
        SessionStatus::Failed | SessionStatus::Expired => "blocked",
        _ if matches!(
            session.provisioning_source,
            ProvisioningSource::DefaultInstanceUrl
        ) =>
        {
            "blocked"
        }
        _ => "open",
    };
    let runtime_status = match session.runtime_state {
        RuntimeState::Running => "open",
        RuntimeState::Paused | RuntimeState::Terminated => "resolved",
    };
    let fallback_receipt_status = session
        .config
        .as_ref()
        .map(|cfg| {
            if cfg.verification_fallback_enabled
                && cfg.verification_fallback_require_signed_receipts
            {
                "resolved"
            } else {
                "open"
            }
        })
        .unwrap_or("open");

    let mut todos = vec![
        FrontdoorGatewayTodoItem {
            todo_id: "sign_authorization_challenge".to_string(),
            severity: "required".to_string(),
            status: signature_status.to_string(),
            priority: 0,
            entry_blocking: false,
            owner: "user".to_string(),
            action: "Sign the wallet challenge and verify session ownership.".to_string(),
            evidence_refs: FrontdoorTodoEvidenceRefs {
                session_id: session_id.clone(),
                provisioning_source: provisioning_source.clone(),
                verification_level: verification_level.clone(),
                module_state: module_state.clone(),
                control_state: control_state.clone(),
            },
        },
        FrontdoorGatewayTodoItem {
            todo_id: "funding_preflight".to_string(),
            severity: "required".to_string(),
            status: funding_status.to_string(),
            priority: 0,
            entry_blocking: false,
            owner: "operator".to_string(),
            action: "Pass deterministic funding/auth preflight checks before provisioning."
                .to_string(),
            evidence_refs: FrontdoorTodoEvidenceRefs {
                session_id: session_id.clone(),
                provisioning_source: provisioning_source.clone(),
                verification_level: verification_level.clone(),
                module_state: module_state.clone(),
                control_state: control_state.clone(),
            },
        },
        FrontdoorGatewayTodoItem {
            todo_id: "dedicated_provisioning".to_string(),
            severity: "required".to_string(),
            status: provisioning_status.to_string(),
            priority: 0,
            entry_blocking: false,
            owner: "operator".to_string(),
            action: "Launch via command provisioning and persist EigenCloud evidence.".to_string(),
            evidence_refs: FrontdoorTodoEvidenceRefs {
                session_id: session_id.clone(),
                provisioning_source: provisioning_source.clone(),
                verification_level: verification_level.clone(),
                module_state: module_state.clone(),
                control_state: control_state.clone(),
            },
        },
        FrontdoorGatewayTodoItem {
            todo_id: "runtime_hardening".to_string(),
            severity: "recommended".to_string(),
            status: runtime_status.to_string(),
            priority: 0,
            entry_blocking: false,
            owner: "operator".to_string(),
            action: "Apply runtime control posture (pause/resume/terminate) intentionally."
                .to_string(),
            evidence_refs: FrontdoorTodoEvidenceRefs {
                session_id: session_id.clone(),
                provisioning_source: provisioning_source.clone(),
                verification_level: verification_level.clone(),
                module_state: module_state.clone(),
                control_state: control_state.clone(),
            },
        },
        FrontdoorGatewayTodoItem {
            todo_id: "signed_fallback_receipts".to_string(),
            severity: "recommended".to_string(),
            status: fallback_receipt_status.to_string(),
            priority: 0,
            entry_blocking: false,
            owner: "operator".to_string(),
            action: "Keep signed fallback receipts enabled for degraded verification paths."
                .to_string(),
            evidence_refs: FrontdoorTodoEvidenceRefs {
                session_id,
                provisioning_source,
                verification_level,
                module_state,
                control_state,
            },
        },
    ];

    for todo in &mut todos {
        todo.entry_blocking = todo.severity == "required" && todo.status != "resolved";
        todo.priority = gateway_todo_priority(todo.severity.as_str(), todo.status.as_str());
    }
    todos.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.todo_id.cmp(&b.todo_id))
    });

    let todo_open_required_count = todos
        .iter()
        .filter(|todo| todo.severity == "required" && todo.status != "resolved")
        .count();
    let todo_open_recommended_count = todos
        .iter()
        .filter(|todo| todo.severity == "recommended" && todo.status != "resolved")
        .count();
    let required_todo_ids_in_priority_order = todos
        .iter()
        .filter(|todo| todo.severity == "required" && todo.status != "resolved")
        .map(|todo| todo.todo_id.clone())
        .collect::<Vec<_>>();

    FrontdoorGatewayTodosResponse {
        session_id: session.id.to_string(),
        todo_open_required_count,
        todo_open_recommended_count,
        highest_priority: todos.iter().map(|todo| todo.priority).max().unwrap_or(0),
        has_blocking_required_todos: todo_open_required_count > 0,
        required_todo_ids_in_priority_order,
        todo_status_summary: todo_status_summary_counts(&todos),
        todos,
    }
}

fn todo_status_summary(payload: &FrontdoorGatewayTodosResponse) -> String {
    payload.todo_status_summary.clone()
}

fn todo_status_summary_counts(todos: &[FrontdoorGatewayTodoItem]) -> String {
    let mut open = 0usize;
    let mut blocked = 0usize;
    let mut in_progress = 0usize;
    let mut resolved = 0usize;
    for todo in todos {
        match todo.status.as_str() {
            "blocked" => blocked = blocked.saturating_add(1),
            "in_progress" => in_progress = in_progress.saturating_add(1),
            "resolved" => resolved = resolved.saturating_add(1),
            _ => open = open.saturating_add(1),
        }
    }
    format!("open={open};blocked={blocked};in_progress={in_progress};resolved={resolved}")
}

fn gateway_todo_priority(severity: &str, status: &str) -> u8 {
    match (severity, status) {
        ("required", "blocked") => 100,
        ("required", "open") => 95,
        ("required", "in_progress") => 90,
        ("required", "resolved") => 70,
        ("recommended", "blocked") => 60,
        ("recommended", "open") => 50,
        ("recommended", "in_progress") => 45,
        ("recommended", "resolved") => 20,
        _ => 10,
    }
}

fn evaluate_funding_preflight(
    session: &ProvisioningSession,
    config: &FrontdoorUserConfig,
) -> FundingPreflightState {
    let wallet_bound = validate_wallet_association(config, &session.wallet_address).is_ok();
    let auth_ready_default = if config.verification_backend == "eigencloud_primary"
        && config.verification_eigencloud_auth_scheme == "api_key"
    {
        config
            .eigencloud_auth_key
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    } else {
        true
    };

    let gas_ready = preflight_override_bool(config, "gas_ready").unwrap_or(true);
    let fee_ready = preflight_override_bool(config, "fee_ready").unwrap_or(true);
    let auth_ready = preflight_override_bool(config, "auth_ready").unwrap_or(auth_ready_default);
    let policy_ready =
        preflight_override_bool(config, "policy_ready").unwrap_or(config.accept_terms);

    let checks = vec![
        FundingPreflightCheckState {
            check_id: "wallet_binding".to_string(),
            status: if wallet_bound { "passed" } else { "failed" }.to_string(),
            detail: if wallet_bound {
                "Connected wallet and config wallet association match.".to_string()
            } else {
                "Connected wallet does not satisfy custody-mode wallet association.".to_string()
            },
        },
        FundingPreflightCheckState {
            check_id: "auth_material".to_string(),
            status: if auth_ready { "passed" } else { "failed" }.to_string(),
            detail: if auth_ready {
                "Verification backend auth requirements satisfied.".to_string()
            } else {
                "Missing or invalid verification auth material.".to_string()
            },
        },
        FundingPreflightCheckState {
            check_id: "gas_budget".to_string(),
            status: if gas_ready { "passed" } else { "failed" }.to_string(),
            detail: if gas_ready {
                "Gas readiness checks passed.".to_string()
            } else {
                "Insufficient gas readiness for provisioning.".to_string()
            },
        },
        FundingPreflightCheckState {
            check_id: "platform_fee".to_string(),
            status: if fee_ready { "passed" } else { "failed" }.to_string(),
            detail: if fee_ready {
                "Platform fee readiness checks passed.".to_string()
            } else {
                "Insufficient platform fee readiness for provisioning.".to_string()
            },
        },
        FundingPreflightCheckState {
            check_id: "policy".to_string(),
            status: if policy_ready { "passed" } else { "failed" }.to_string(),
            detail: if policy_ready {
                "Policy acceptance and gating checks passed.".to_string()
            } else {
                "Policy checks failed (accept_terms or policy override).".to_string()
            },
        },
    ];

    let failure_category = if !wallet_bound {
        Some("policy".to_string())
    } else if !auth_ready {
        Some("auth".to_string())
    } else if !policy_ready {
        Some("policy".to_string())
    } else if !gas_ready {
        Some("gas".to_string())
    } else if !fee_ready {
        Some("fee".to_string())
    } else {
        None
    };

    FundingPreflightState {
        status: if failure_category.is_none() {
            "passed".to_string()
        } else {
            "failed".to_string()
        },
        failure_category,
        checks,
        updated_at: Utc::now(),
    }
}

fn preflight_override_bool(config: &FrontdoorUserConfig, key: &str) -> Option<bool> {
    config
        .domain_overrides
        .get("frontdoor_preflight")
        .and_then(|value| value.as_object())
        .and_then(|map| map.get(key))
        .and_then(|value| value.as_bool())
}

fn render_session_response(session: &ProvisioningSession) -> FrontdoorSessionResponse {
    let config = session.config.as_ref();
    let todos = build_gateway_todos(session);
    let verification_level = verification_assurance_level(config);
    FrontdoorSessionResponse {
        session_id: session.id.to_string(),
        wallet_address: session.wallet_address.clone(),
        privy_user_id: session.privy_user_id.clone(),
        version: session.version,
        status: session.status.as_str().to_string(),
        detail: session.detail.clone(),
        provisioning_source: session.provisioning_source.as_str().to_string(),
        dedicated_instance: session.provisioning_source.dedicated_instance(),
        launched_on_eigencloud: session_launched_on_eigencloud(session).unwrap_or(false),
        verification_backend: config
            .map(|c| c.verification_backend.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        verification_level,
        verification_fallback_enabled: config
            .map(|c| c.verification_fallback_enabled)
            .unwrap_or(false),
        verification_fallback_require_signed_receipts: config
            .map(|c| c.verification_fallback_require_signed_receipts)
            .unwrap_or(false),
        instance_url: session.instance_url.clone(),
        app_url: session.app_url.clone(),
        verify_url: session.verify_url.clone(),
        eigen_app_id: session.eigen_app_id.clone(),
        error: session.error.clone(),
        created_at: session.created_at.to_rfc3339(),
        updated_at: session.updated_at.to_rfc3339(),
        expires_at: session.expires_at.to_rfc3339(),
        profile_name: config.map(|c| c.profile_name.clone()),
        todo_open_required_count: todos.todo_open_required_count,
        todo_open_recommended_count: todos.todo_open_recommended_count,
        todo_status_summary: todos.todo_status_summary,
        runtime_state: session.runtime_state.as_str().to_string(),
        funding_preflight_status: session.funding_preflight.status.clone(),
        funding_preflight_failure_category: session.funding_preflight.failure_category.clone(),
    }
}

fn render_session_summary(session: &ProvisioningSession) -> FrontdoorSessionSummaryResponse {
    let config = session.config.as_ref();
    let todos = build_gateway_todos(session);
    let verification_level = verification_assurance_level(config);
    FrontdoorSessionSummaryResponse {
        session_ref: public_session_ref(session),
        wallet_address: session.wallet_address.clone(),
        version: session.version,
        status: session.status.as_str().to_string(),
        detail: session.detail.clone(),
        provisioning_source: session.provisioning_source.as_str().to_string(),
        dedicated_instance: session.provisioning_source.dedicated_instance(),
        launched_on_eigencloud: session_launched_on_eigencloud(session).unwrap_or(false),
        verification_backend: config
            .map(|c| c.verification_backend.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        verification_level,
        verification_fallback_enabled: config
            .map(|c| c.verification_fallback_enabled)
            .unwrap_or(false),
        verification_fallback_require_signed_receipts: config
            .map(|c| c.verification_fallback_require_signed_receipts)
            .unwrap_or(false),
        created_at: session.created_at.to_rfc3339(),
        updated_at: session.updated_at.to_rfc3339(),
        expires_at: session.expires_at.to_rfc3339(),
        profile_name: config.map(|c| c.profile_name.clone()),
        todo_open_required_count: todos.todo_open_required_count,
        todo_open_recommended_count: todos.todo_open_recommended_count,
        todo_status_summary: todos.todo_status_summary,
        runtime_state: session.runtime_state.as_str().to_string(),
        funding_preflight_status: session.funding_preflight.status.clone(),
        funding_preflight_failure_category: session.funding_preflight.failure_category.clone(),
    }
}

fn public_session_ref(session: &ProvisioningSession) -> String {
    format!("v{}", session.version)
}

fn session_launched_on_eigencloud(session: &ProvisioningSession) -> Option<bool> {
    if session.instance_url.is_none()
        && session.app_url.is_none()
        && session.verify_url.is_none()
        && session.eigen_app_id.is_none()
    {
        return None;
    }

    let from_app_id = session
        .eigen_app_id
        .as_deref()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let from_verify_url = session
        .verify_url
        .as_deref()
        .map(looks_like_eigencloud_url)
        .unwrap_or(false);
    let from_app_url = session
        .app_url
        .as_deref()
        .map(looks_like_eigencloud_url)
        .unwrap_or(false);
    let from_instance_url = session
        .instance_url
        .as_deref()
        .map(looks_like_eigencloud_url)
        .unwrap_or(false);

    Some(from_app_id || from_verify_url || from_app_url || from_instance_url)
}

fn looks_like_eigencloud_url(candidate: &str) -> bool {
    let Ok(url) = Url::parse(candidate) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    host == "verify-sepolia.eigencloud.xyz"
        || host == "verify.eigencloud.xyz"
        || host == "eigencloud.xyz"
        || host.ends_with(".eigencloud.xyz")
}

fn execute_provision_output(
    stdout: &str,
    verify_base_url: Option<&str>,
) -> Option<ProvisioningResult> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let mut instance_url = v
            .get("instance_url")
            .or_else(|| v.get("gateway_url"))
            .or_else(|| v.get("url"))
            .and_then(|x| x.as_str())
            .map(|v| v.to_string());
        let mut app_url = v
            .get("app_url")
            .or_else(|| v.get("eigen_app_console_url"))
            .and_then(|x| x.as_str())
            .map(|v| v.to_string());
        let mut verify_url = v
            .get("verify_url")
            .or_else(|| v.get("eigen_verify_url"))
            .or_else(|| v.get("eigen_app_url"))
            .and_then(|x| x.as_str())
            .map(|v| v.to_string());
        let eigen_app_id = v
            .get("eigen_app_id")
            .or_else(|| v.get("app_id"))
            .and_then(|x| x.as_str())
            .map(|v| v.to_string());
        if verify_url.is_none() {
            if let Some(app_id) = eigen_app_id.as_deref() {
                verify_url = build_verify_app_url(verify_base_url, app_id);
            } else if let Some(instance) = instance_url.as_deref()
                && looks_like_verify_url(instance)
            {
                verify_url = Some(instance.to_string());
            }
        }
        let should_infer_app_url = app_url.is_none()
            && (instance_url.is_none()
                || instance_url
                    .as_deref()
                    .map(looks_like_verify_url)
                    .unwrap_or(false));
        if should_infer_app_url {
            app_url = verify_url
                .as_deref()
                .and_then(|url| build_app_url_from_verify_url(url, eigen_app_id.as_deref()));
        }
        if let Some(instance) = instance_url.as_deref()
            && looks_like_verify_url(instance)
            && app_url.is_some()
        {
            instance_url = app_url.clone();
        }
        if instance_url.is_none() {
            instance_url = app_url.clone().or_else(|| verify_url.clone());
        }
        if let Some(instance_url) = instance_url {
            return Some(ProvisioningResult {
                instance_url,
                app_url,
                verify_url,
                eigen_app_id,
            });
        }
    }

    for line in trimmed.lines().rev() {
        let candidate = line.trim();
        if candidate.starts_with("http://") || candidate.starts_with("https://") {
            let verify_url = if looks_like_verify_url(candidate) {
                Some(candidate.to_string())
            } else {
                None
            };
            return Some(ProvisioningResult {
                instance_url: candidate.to_string(),
                app_url: None,
                verify_url,
                eigen_app_id: None,
            });
        }
    }

    None
}

fn build_verify_app_url(base: Option<&str>, app_id: &str) -> Option<String> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        return None;
    }
    let base = base.unwrap_or("https://verify-sepolia.eigencloud.xyz/app");
    Some(format!("{}/{}", base.trim_end_matches('/'), app_id))
}

fn build_app_url_from_verify_url(verify_url: &str, app_id: Option<&str>) -> Option<String> {
    let mut url = Url::parse(verify_url).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let rewritten_host = if host == "verify-sepolia.eigencloud.xyz" {
        "sepolia.eigencloud.xyz".to_string()
    } else if host == "verify-mainnet.eigencloud.xyz" || host == "verify.eigencloud.xyz" {
        "mainnet.eigencloud.xyz".to_string()
    } else if let Some(stripped) = host.strip_prefix("verify-") {
        stripped.to_string()
    } else if let Some(stripped) = host.strip_prefix("verify.") {
        stripped.to_string()
    } else {
        return None;
    };
    url.set_host(Some(&rewritten_host)).ok()?;

    if let Some(app_id) = app_id.map(str::trim).filter(|v| !v.is_empty()) {
        url.set_path(&format!("/app/{app_id}"));
    }

    let rendered = url.to_string();
    Some(rendered.trim_end_matches('/').to_string())
}

fn looks_like_verify_url(candidate: &str) -> bool {
    if let Ok(url) = Url::parse(candidate)
        && let Some(host) = url.host_str()
    {
        let host = host.to_ascii_lowercase();
        return host == "verify-sepolia.eigencloud.xyz"
            || host == "verify-mainnet.eigencloud.xyz"
            || host == "verify.eigencloud.xyz";
    }
    false
}

struct ProvisionCommandInput<'a> {
    session_id: Uuid,
    wallet: &'a str,
    privy_user_id: Option<&'a str>,
    privy_identity_token: Option<&'a str>,
    privy_access_token: Option<&'a str>,
    chain_id: u64,
    version: u64,
    config: &'a FrontdoorUserConfig,
    verify_base_url: Option<&'a str>,
}

#[derive(Debug, Clone)]
struct ProvisionCommandLog {
    source: String,
    stream: String,
    line: String,
}

fn classify_provision_log_source(line: &str) -> &'static str {
    let lower = line.to_ascii_lowercase();
    if lower.contains("railway") {
        "railway"
    } else if lower.contains("ironclaw") {
        "ironclaw"
    } else if lower.contains("ecloud")
        || lower.contains("eigencloud")
        || lower.contains("eigenda")
        || lower.contains("verify-sepolia")
    {
        "ecloud"
    } else {
        "provision"
    }
}

fn build_provision_command(
    template: &str,
    input: &ProvisionCommandInput<'_>,
) -> Result<Command, String> {
    let config_json = serde_json::to_string(input.config)
        .map_err(|e| format!("config serialization failed: {e}"))?;
    let config_b64 =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(config_json.as_bytes());
    let eigencloud_auth_key = input.config.eigencloud_auth_key.clone().unwrap_or_default();
    let verify_app_base_url = input.verify_base_url.unwrap_or("").to_string();

    let replacements = [
        (
            "{session_id}",
            "ENCLAGENT_FRONTDOOR_SESSION_ID",
            input.session_id.to_string(),
        ),
        (
            "{wallet_address}",
            "ENCLAGENT_FRONTDOOR_WALLET_ADDRESS",
            input.wallet.to_string(),
        ),
        (
            "{privy_user_id}",
            "ENCLAGENT_FRONTDOOR_PRIVY_USER_ID",
            input.privy_user_id.unwrap_or("").to_string(),
        ),
        (
            "{privy_identity_token}",
            "ENCLAGENT_FRONTDOOR_PRIVY_IDENTITY_TOKEN",
            input.privy_identity_token.unwrap_or("").to_string(),
        ),
        (
            "{privy_access_token}",
            "ENCLAGENT_FRONTDOOR_PRIVY_ACCESS_TOKEN",
            input.privy_access_token.unwrap_or("").to_string(),
        ),
        (
            "{chain_id}",
            "ENCLAGENT_FRONTDOOR_CHAIN_ID",
            input.chain_id.to_string(),
        ),
        (
            "{version}",
            "ENCLAGENT_FRONTDOOR_VERSION",
            input.version.to_string(),
        ),
        (
            "{config_version}",
            "ENCLAGENT_FRONTDOOR_CONFIG_VERSION",
            input.config.config_version.to_string(),
        ),
        (
            "{profile_domain}",
            "ENCLAGENT_FRONTDOOR_PROFILE_DOMAIN",
            input.config.profile_domain.clone(),
        ),
        (
            "{domain_overrides_json}",
            "ENCLAGENT_FRONTDOOR_DOMAIN_OVERRIDES_JSON",
            serde_json::to_string(&input.config.domain_overrides)
                .unwrap_or_else(|_| "{}".to_string()),
        ),
        (
            "{profile_name}",
            "ENCLAGENT_FRONTDOOR_PROFILE_NAME",
            input.config.profile_name.clone(),
        ),
        (
            "{custody_mode}",
            "ENCLAGENT_FRONTDOOR_CUSTODY_MODE",
            input.config.custody_mode.clone(),
        ),
        (
            "{operator_wallet_address}",
            "ENCLAGENT_FRONTDOOR_OPERATOR_WALLET_ADDRESS",
            input
                .config
                .operator_wallet_address
                .clone()
                .unwrap_or_default(),
        ),
        (
            "{user_wallet_address}",
            "ENCLAGENT_FRONTDOOR_USER_WALLET_ADDRESS",
            input.config.user_wallet_address.clone().unwrap_or_default(),
        ),
        (
            "{vault_address}",
            "ENCLAGENT_FRONTDOOR_VAULT_ADDRESS",
            input.config.vault_address.clone().unwrap_or_default(),
        ),
        (
            "{gateway_auth_key}",
            "ENCLAGENT_FRONTDOOR_GATEWAY_AUTH_KEY",
            input.config.gateway_auth_key.clone(),
        ),
        (
            "{eigencloud_auth_key}",
            "ENCLAGENT_FRONTDOOR_EIGENCLOUD_AUTH_KEY",
            eigencloud_auth_key,
        ),
        (
            "{verification_backend}",
            "ENCLAGENT_FRONTDOOR_VERIFICATION_BACKEND",
            input.config.verification_backend.clone(),
        ),
        (
            "{verification_eigencloud_endpoint}",
            "ENCLAGENT_FRONTDOOR_VERIFICATION_EIGENCLOUD_ENDPOINT",
            input
                .config
                .verification_eigencloud_endpoint
                .clone()
                .unwrap_or_default(),
        ),
        (
            "{verification_eigencloud_auth_scheme}",
            "ENCLAGENT_FRONTDOOR_VERIFICATION_EIGENCLOUD_AUTH_SCHEME",
            input.config.verification_eigencloud_auth_scheme.clone(),
        ),
        (
            "{verification_eigencloud_timeout_ms}",
            "ENCLAGENT_FRONTDOOR_VERIFICATION_EIGENCLOUD_TIMEOUT_MS",
            input.config.verification_eigencloud_timeout_ms.to_string(),
        ),
        (
            "{verification_fallback_enabled}",
            "ENCLAGENT_FRONTDOOR_VERIFICATION_FALLBACK_ENABLED",
            if input.config.verification_fallback_enabled {
                "true".to_string()
            } else {
                "false".to_string()
            },
        ),
        (
            "{verification_fallback_signing_key_id}",
            "ENCLAGENT_FRONTDOOR_VERIFICATION_FALLBACK_SIGNING_KEY_ID",
            input
                .config
                .verification_fallback_signing_key_id
                .clone()
                .unwrap_or_default(),
        ),
        (
            "{verification_fallback_chain_path}",
            "ENCLAGENT_FRONTDOOR_VERIFICATION_FALLBACK_CHAIN_PATH",
            input
                .config
                .verification_fallback_chain_path
                .clone()
                .unwrap_or_default(),
        ),
        (
            "{verification_fallback_require_signed_receipts}",
            "ENCLAGENT_FRONTDOOR_VERIFICATION_FALLBACK_REQUIRE_SIGNED_RECEIPTS",
            if input.config.verification_fallback_require_signed_receipts {
                "true".to_string()
            } else {
                "false".to_string()
            },
        ),
        (
            "{verify_app_base_url}",
            "ENCLAGENT_FRONTDOOR_VERIFY_APP_BASE_URL",
            verify_app_base_url,
        ),
        (
            "{inference_summary}",
            "ENCLAGENT_FRONTDOOR_INFERENCE_SUMMARY",
            input.config.inference_summary.clone().unwrap_or_default(),
        ),
        (
            "{inference_confidence}",
            "ENCLAGENT_FRONTDOOR_INFERENCE_CONFIDENCE",
            input
                .config
                .inference_confidence
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ),
        (
            "{config_json}",
            "ENCLAGENT_FRONTDOOR_CONFIG_JSON",
            config_json.clone(),
        ),
        ("{config_b64}", "ENCLAGENT_FRONTDOOR_CONFIG_B64", config_b64),
    ];

    let mut cmd = template.to_string();
    for (placeholder, env_key, _) in &replacements {
        let env_ref = format!("${{{env_key}}}");
        let quoted_env_ref = format!("\"${{{env_key}}}\"");
        cmd = cmd.replace(&format!("'{}'", placeholder), &quoted_env_ref);
        cmd = cmd.replace(&format!("\"{}\"", placeholder), &quoted_env_ref);
        cmd = cmd.replace(placeholder, &env_ref);
    }

    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(cmd);
    for (_, env_key, value) in &replacements {
        command.env(env_key, value);
    }
    Ok(command)
}

async fn execute_provision_command_with_stream<F, Fut>(
    template: &str,
    input: &ProvisionCommandInput<'_>,
    mut on_log: F,
) -> Result<ProvisioningResult, String>
where
    F: FnMut(ProvisionCommandLog) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut command = build_provision_command(template, input)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to execute provision command: {e}"))?;

    let mut stdout_lines = child
        .stdout
        .take()
        .map(|stream| BufReader::new(stream).lines());
    let mut stderr_lines = child
        .stderr
        .take()
        .map(|stream| BufReader::new(stream).lines());
    let mut stdout_complete = stdout_lines.is_none();
    let mut stderr_complete = stderr_lines.is_none();
    let mut stdout = String::new();
    let mut stderr = String::new();
    let started_at = Instant::now();
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(20));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;

    while !(stdout_complete && stderr_complete) {
        tokio::select! {
            maybe_line = async {
                if let Some(lines) = stdout_lines.as_mut() {
                    lines.next_line().await
                } else {
                    Ok(None)
                }
            }, if !stdout_complete => {
                match maybe_line {
                    Ok(Some(line)) => {
                        stdout.push_str(&line);
                        stdout.push('\n');
                        on_log(ProvisionCommandLog {
                            source: classify_provision_log_source(&line).to_string(),
                            stream: "stdout".to_string(),
                            line,
                        }).await;
                    }
                    Ok(None) => {
                        stdout_complete = true;
                    }
                    Err(err) => {
                        let detail = format!("stdout stream error: {err}");
                        stderr.push_str(&detail);
                        stderr.push('\n');
                        on_log(ProvisionCommandLog {
                            source: "provision".to_string(),
                            stream: "stderr".to_string(),
                            line: detail,
                        }).await;
                        stdout_complete = true;
                    }
                }
            }
            maybe_line = async {
                if let Some(lines) = stderr_lines.as_mut() {
                    lines.next_line().await
                } else {
                    Ok(None)
                }
            }, if !stderr_complete => {
                match maybe_line {
                    Ok(Some(line)) => {
                        stderr.push_str(&line);
                        stderr.push('\n');
                        on_log(ProvisionCommandLog {
                            source: classify_provision_log_source(&line).to_string(),
                            stream: "stderr".to_string(),
                            line,
                        }).await;
                    }
                    Ok(None) => {
                        stderr_complete = true;
                    }
                    Err(err) => {
                        let detail = format!("stderr stream error: {err}");
                        stderr.push_str(&detail);
                        stderr.push('\n');
                        on_log(ProvisionCommandLog {
                            source: "provision".to_string(),
                            stream: "stderr".to_string(),
                            line: detail,
                        }).await;
                        stderr_complete = true;
                    }
                }
            }
            _ = heartbeat.tick() => {
                let elapsed_secs = started_at.elapsed().as_secs();
                on_log(ProvisionCommandLog {
                    source: "ecloud".to_string(),
                    stream: "heartbeat".to_string(),
                    line: format!(
                        "EigenCloud build + IronClaw runtime provisioning in progress ({}s elapsed)",
                        elapsed_secs
                    ),
                }).await;
            }
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("failed waiting for provision command: {e}"))?;
    if !status.success() {
        return Err(format!(
            "provision command failed with status {}: {}",
            status,
            stderr.trim()
        ));
    }

    let result = execute_provision_output(&stdout, input.verify_base_url)
        .or_else(|| execute_provision_output(&stderr, input.verify_base_url));
    let Some(result) = result else {
        return Err(
            "provision command succeeded but did not return an instance url in stdout".to_string(),
        );
    };
    Ok(result)
}

fn load_wallet_store(path: &PathBuf) -> Result<WalletSessionStore, String> {
    let data = match std::fs::read(path) {
        Ok(v) => v,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(err) => return Err(format!("failed reading store: {err}")),
    };
    serde_json::from_slice(&data).map_err(|e| format!("failed parsing store json: {e}"))
}

fn persist_wallet_store(path: &PathBuf, store: &WalletSessionStore) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed creating store dir: {e}"))?;
    }
    let serialized = serde_json::to_vec_pretty(store)
        .map_err(|e| format!("failed serializing store json: {e}"))?;
    std::fs::write(path, serialized).map_err(|e| format!("failed writing store: {e}"))
}

fn purge_expired_sessions(state: &mut FrontdoorState) {
    let now = Utc::now();
    let mut expired_ids = Vec::new();
    for (id, session) in &mut state.sessions {
        if session.expires_at < now
            && matches!(
                session.status,
                SessionStatus::AwaitingSignature | SessionStatus::Provisioning
            )
        {
            session.status = SessionStatus::Expired;
            session.updated_at = now;
            session.detail = "Session expired".to_string();
        }
        if session.expires_at + chrono::Duration::hours(6) < now {
            expired_ids.push(*id);
        }
    }
    for id in expired_ids {
        state.sessions.remove(&id);
    }
}

fn random_nonce(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn is_signature_like(signature: &str) -> bool {
    let s = signature.trim();
    if !s.starts_with("0x") {
        return false;
    }
    let hex = &s[2..];
    if hex.len() != 130 {
        return false;
    }
    hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn verify_wallet_signature(
    message: &str,
    signature_hex: &str,
    expected_wallet: &str,
) -> Result<(), String> {
    let signature_bytes = decode_hex_prefixed(signature_hex)?;
    if signature_bytes.len() != 65 {
        return Err("signature must decode to 65 bytes".to_string());
    }

    let sig = EcdsaSignature::try_from(&signature_bytes[..64])
        .map_err(|e| format!("invalid ECDSA signature bytes: {e}"))?;
    let recovery_id = normalize_recovery_id(signature_bytes[64])?;
    let prehash = eip191_personal_sign_hash(message);
    let verifying_key = VerifyingKey::recover_from_prehash(&prehash, &sig, recovery_id)
        .map_err(|e| format!("failed recovering signer from signature: {e}"))?;
    let recovered_wallet = ethereum_address_from_verifying_key(&verifying_key)?;
    let expected = normalize_wallet_address(expected_wallet)
        .ok_or_else(|| "wallet_address must be a valid EVM address".to_string())?;
    if recovered_wallet != expected {
        return Err("signature does not match wallet_address".to_string());
    }
    Ok(())
}

fn normalize_recovery_id(raw: u8) -> Result<RecoveryId, String> {
    let id = match raw {
        27 | 28 => raw - 27,
        0 | 1 => raw,
        _ => return Err("signature recovery id must be 0/1 or 27/28".to_string()),
    };
    RecoveryId::try_from(id).map_err(|_| "signature recovery id is invalid".to_string())
}

fn eip191_personal_sign_hash(message: &str) -> [u8; 32] {
    let bytes = message.as_bytes();
    let prefix = format!("\x19Ethereum Signed Message:\n{}", bytes.len());
    let mut hasher = Keccak256::new();
    hasher.update(prefix.as_bytes());
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn ethereum_address_from_verifying_key(key: &VerifyingKey) -> Result<String, String> {
    let encoded = key.to_encoded_point(false);
    let pubkey = encoded.as_bytes();
    if pubkey.len() != 65 || pubkey[0] != 0x04 {
        return Err("unexpected recovered public key format".to_string());
    }

    let mut hasher = Keccak256::new();
    hasher.update(&pubkey[1..]);
    let digest = hasher.finalize();
    let address = &digest[12..];
    Ok(format!("0x{}", encode_hex_lower(address)))
}

fn decode_hex_prefixed(value: &str) -> Result<Vec<u8>, String> {
    let trimmed = value.trim();
    if !trimmed.starts_with("0x") {
        return Err("hex string must be 0x-prefixed".to_string());
    }
    let hex = &trimmed[2..];
    if hex.is_empty() {
        return Err("hex string is empty".to_string());
    }
    if !hex.len().is_multiple_of(2) {
        return Err("hex string must have an even number of characters".to_string());
    }

    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for pair in bytes.chunks_exact(2) {
        let hi = decode_hex_nibble(pair[0]).ok_or_else(|| "invalid hex character".to_string())?;
        let lo = decode_hex_nibble(pair[1]).ok_or_else(|| "invalid hex character".to_string())?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn encode_hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(nibble_to_hex(b >> 4));
        out.push(nibble_to_hex(b & 0x0f));
    }
    out
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => '0',
    }
}

fn normalize_wallet_address(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if !trimmed.starts_with("0x") || trimmed.len() != 42 {
        return None;
    }
    let hex = &trimmed[2..];
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

fn message_matches(candidate: &str, expected: &str) -> bool {
    candidate.trim() == expected.trim()
}

fn mandatory_frontdoor_steps() -> Vec<String> {
    vec![
        "connect_wallet_with_privy".to_string(),
        "confirm_onboarding_plan".to_string(),
        "sign_gasless_authorization_transaction".to_string(),
        "configure_runtime_profile_and_risk".to_string(),
        "set_gateway_auth_key".to_string(),
        "accept_risk_and_terms".to_string(),
    ]
}

fn frontdoor_policy_templates() -> Vec<FrontdoorPolicyTemplate> {
    vec![
        FrontdoorPolicyTemplate {
            template_id: "general_safe_baseline".to_string(),
            title: "General Safe Baseline".to_string(),
            domain: "general".to_string(),
            objective:
                "Run non-trading automation with conservative permissions and verifiable receipts."
                    .to_string(),
            rationale:
                "Preserves broad utility while enforcing signed fallback verification and minimal data sharing."
                    .to_string(),
            module_plan: vec![
                "general".to_string(),
                "developer".to_string(),
                "research".to_string(),
                "business_ops".to_string(),
            ],
            risk_profile: FrontdoorPolicyTemplateRiskProfile {
                posture: "conservative".to_string(),
                max_position_size_usd: 1_000,
                max_leverage: 2,
                max_slippage_bps: 50,
            },
            config: FrontdoorPolicyTemplateConfig {
                paper_live_policy: "paper_only".to_string(),
                custody_mode: "user_wallet".to_string(),
                verification_backend: "eigencloud_primary".to_string(),
                verification_fallback_require_signed_receipts: true,
                information_sharing_scope: "signals_only".to_string(),
            },
        },
        FrontdoorPolicyTemplate {
            template_id: "developer_build_pipeline".to_string(),
            title: "Developer Build Pipeline".to_string(),
            domain: "developer".to_string(),
            objective:
                "Support coding workflows with stronger auditability and deterministic runtime controls."
                    .to_string(),
            rationale:
                "Keeps execution in paper mode while allowing broader artifact traceability for debugging."
                    .to_string(),
            module_plan: vec![
                "general".to_string(),
                "developer".to_string(),
                "research".to_string(),
                "communications".to_string(),
            ],
            risk_profile: FrontdoorPolicyTemplateRiskProfile {
                posture: "moderate".to_string(),
                max_position_size_usd: 5_000,
                max_leverage: 2,
                max_slippage_bps: 80,
            },
            config: FrontdoorPolicyTemplateConfig {
                paper_live_policy: "paper_first".to_string(),
                custody_mode: "user_wallet".to_string(),
                verification_backend: "eigencloud_primary".to_string(),
                verification_fallback_require_signed_receipts: true,
                information_sharing_scope: "full_audit".to_string(),
            },
        },
        FrontdoorPolicyTemplate {
            template_id: "hyperliquid_paper_operator".to_string(),
            title: "Hyperliquid Paper Operator".to_string(),
            domain: "hyperliquid".to_string(),
            objective:
                "Exercise Hyperliquid automation in paper mode with explicit risk caps and signed receipts."
                    .to_string(),
            rationale:
                "Maintains trading-module optionality while blocking unsafe live defaults and preserving fallback verification."
                    .to_string(),
            module_plan: vec![
                "general".to_string(),
                "business_ops".to_string(),
                "research".to_string(),
                "communications".to_string(),
                "hyperliquid_addon".to_string(),
            ],
            risk_profile: FrontdoorPolicyTemplateRiskProfile {
                posture: "strict_trading".to_string(),
                max_position_size_usd: 25_000,
                max_leverage: 3,
                max_slippage_bps: 40,
            },
            config: FrontdoorPolicyTemplateConfig {
                paper_live_policy: "paper_only".to_string(),
                custody_mode: "user_wallet".to_string(),
                verification_backend: "eigencloud_primary".to_string(),
                verification_fallback_require_signed_receipts: true,
                information_sharing_scope: "signals_and_execution".to_string(),
            },
        },
        FrontdoorPolicyTemplate {
            template_id: "eigenda_receipt_guardrail".to_string(),
            title: "EigenDA Receipt Guardrail".to_string(),
            domain: "eigenda".to_string(),
            objective:
                "Prioritize verifiable receipt continuity for EigenDA-linked workflows under degraded backend conditions."
                    .to_string(),
            rationale:
                "Favors fallback survivability with signed receipt requirements and lower sharing defaults."
                    .to_string(),
            module_plan: vec![
                "general".to_string(),
                "research".to_string(),
                "business_ops".to_string(),
                "communications".to_string(),
                "eigenda_addon".to_string(),
            ],
            risk_profile: FrontdoorPolicyTemplateRiskProfile {
                posture: "receipt_hardened".to_string(),
                max_position_size_usd: 2_500,
                max_leverage: 2,
                max_slippage_bps: 60,
            },
            config: FrontdoorPolicyTemplateConfig {
                paper_live_policy: "paper_first".to_string(),
                custody_mode: "user_wallet".to_string(),
                verification_backend: "fallback_only".to_string(),
                verification_fallback_require_signed_receipts: true,
                information_sharing_scope: "signals_only".to_string(),
            },
        },
    ]
}

fn frontdoor_domain_profiles() -> Vec<FrontdoorDomainProfile> {
    let core_modules = vec![
        "general".to_string(),
        "developer".to_string(),
        "creative".to_string(),
        "research".to_string(),
        "business_ops".to_string(),
        "communications".to_string(),
    ];

    vec![
        FrontdoorDomainProfile {
            domain: "general".to_string(),
            default_modules: core_modules.clone(),
            required_fields: vec![
                "config_version".to_string(),
                "profile_domain".to_string(),
                "profile_name".to_string(),
                "gateway_auth_key".to_string(),
                "verification_backend".to_string(),
                "verification_eigencloud_auth_scheme".to_string(),
                "verification_eigencloud_timeout_ms".to_string(),
                "accept_terms".to_string(),
            ],
            optional_fields: vec![
                "domain_overrides".to_string(),
                "inference_summary".to_string(),
                "inference_confidence".to_string(),
                "inference_warnings".to_string(),
                "enable_memory".to_string(),
            ],
        },
        FrontdoorDomainProfile {
            domain: "developer".to_string(),
            default_modules: core_modules.clone(),
            required_fields: vec![
                "config_version".to_string(),
                "profile_domain".to_string(),
                "profile_name".to_string(),
                "gateway_auth_key".to_string(),
                "verification_backend".to_string(),
                "accept_terms".to_string(),
            ],
            optional_fields: vec!["domain_overrides".to_string(), "enable_memory".to_string()],
        },
        FrontdoorDomainProfile {
            domain: "creative".to_string(),
            default_modules: core_modules.clone(),
            required_fields: vec![
                "config_version".to_string(),
                "profile_domain".to_string(),
                "profile_name".to_string(),
                "gateway_auth_key".to_string(),
                "verification_backend".to_string(),
                "accept_terms".to_string(),
            ],
            optional_fields: vec!["domain_overrides".to_string(), "enable_memory".to_string()],
        },
        FrontdoorDomainProfile {
            domain: "research".to_string(),
            default_modules: core_modules.clone(),
            required_fields: vec![
                "config_version".to_string(),
                "profile_domain".to_string(),
                "profile_name".to_string(),
                "gateway_auth_key".to_string(),
                "verification_backend".to_string(),
                "accept_terms".to_string(),
            ],
            optional_fields: vec!["domain_overrides".to_string(), "enable_memory".to_string()],
        },
        FrontdoorDomainProfile {
            domain: "business_ops".to_string(),
            default_modules: core_modules.clone(),
            required_fields: vec![
                "config_version".to_string(),
                "profile_domain".to_string(),
                "profile_name".to_string(),
                "gateway_auth_key".to_string(),
                "verification_backend".to_string(),
                "accept_terms".to_string(),
            ],
            optional_fields: vec!["domain_overrides".to_string(), "enable_memory".to_string()],
        },
        FrontdoorDomainProfile {
            domain: "communications".to_string(),
            default_modules: core_modules,
            required_fields: vec![
                "config_version".to_string(),
                "profile_domain".to_string(),
                "profile_name".to_string(),
                "gateway_auth_key".to_string(),
                "verification_backend".to_string(),
                "accept_terms".to_string(),
            ],
            optional_fields: vec!["domain_overrides".to_string(), "enable_memory".to_string()],
        },
        FrontdoorDomainProfile {
            domain: "hyperliquid".to_string(),
            default_modules: vec![
                "general".to_string(),
                "business_ops".to_string(),
                "research".to_string(),
                "communications".to_string(),
                "hyperliquid_addon".to_string(),
            ],
            required_fields: vec![
                "config_version".to_string(),
                "profile_domain".to_string(),
                "profile_name".to_string(),
                "hyperliquid_network".to_string(),
                "paper_live_policy".to_string(),
                "max_position_size_usd".to_string(),
                "leverage_cap".to_string(),
                "max_allocation_usd".to_string(),
                "per_trade_notional_cap_usd".to_string(),
                "max_leverage".to_string(),
                "max_slippage_bps".to_string(),
                "symbol_allowlist".to_string(),
                "custody_mode".to_string(),
                "gateway_auth_key".to_string(),
                "verification_backend".to_string(),
                "accept_terms".to_string(),
            ],
            optional_fields: vec![
                "operator_wallet_address".to_string(),
                "user_wallet_address".to_string(),
                "vault_address".to_string(),
                "eigencloud_auth_key".to_string(),
                "verification_fallback_chain_path".to_string(),
            ],
        },
        FrontdoorDomainProfile {
            domain: "eigenda".to_string(),
            default_modules: vec![
                "general".to_string(),
                "research".to_string(),
                "business_ops".to_string(),
                "communications".to_string(),
                "eigenda_addon".to_string(),
            ],
            required_fields: vec![
                "config_version".to_string(),
                "profile_domain".to_string(),
                "profile_name".to_string(),
                "gateway_auth_key".to_string(),
                "verification_backend".to_string(),
                "accept_terms".to_string(),
            ],
            optional_fields: vec![
                "domain_overrides".to_string(),
                "enable_memory".to_string(),
                "verification_fallback_chain_path".to_string(),
            ],
        },
    ]
}

fn default_frontdoor_user_config(
    wallet: &str,
    gateway_auth_key: Option<&str>,
    domain: &str,
) -> FrontdoorUserConfig {
    FrontdoorUserConfig {
        config_version: FRONTDOOR_CURRENT_CONFIG_VERSION,
        profile_domain: domain.to_string(),
        domain_overrides: HashMap::new(),
        inference_summary: None,
        inference_confidence: None,
        inference_warnings: Vec::new(),
        profile_name: "launchpad_profile".to_string(),
        hyperliquid_network: "testnet".to_string(),
        paper_live_policy: "paper_only".to_string(),
        hyperliquid_api_base_url: None,
        hyperliquid_ws_url: None,
        request_timeout_ms: 10_000,
        max_retries: 3,
        retry_backoff_ms: 500,
        max_position_size_usd: 1_000,
        leverage_cap: 2,
        max_allocation_usd: 1_000,
        per_trade_notional_cap_usd: 100,
        max_leverage: 2,
        max_slippage_bps: 50,
        symbol_allowlist: vec!["BTC".to_string(), "ETH".to_string()],
        symbol_denylist: Vec::new(),
        custody_mode: "user_wallet".to_string(),
        operator_wallet_address: None,
        user_wallet_address: Some(wallet.to_string()),
        vault_address: None,
        information_sharing_scope: "signals_only".to_string(),
        kill_switch_enabled: true,
        kill_switch_behavior: "pause_agent".to_string(),
        enable_memory: true,
        gateway_auth_key: gateway_auth_key
            .filter(|v| is_valid_gateway_auth_key(v))
            .map(str::to_string)
            .unwrap_or_else(generate_gateway_auth_key),
        eigencloud_auth_key: None,
        verification_backend: "eigencloud_primary".to_string(),
        verification_eigencloud_endpoint: None,
        verification_eigencloud_auth_scheme: "bearer".to_string(),
        verification_eigencloud_timeout_ms: 5_000,
        verification_fallback_enabled: true,
        verification_fallback_signing_key_id: None,
        verification_fallback_chain_path: None,
        verification_fallback_require_signed_receipts: true,
        accept_terms: true,
    }
}

fn generate_gateway_auth_key() -> String {
    format!("lc_{}", random_nonce(24))
}

fn normalize_domain_name(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("profile_domain is required".to_string());
    }
    if normalized.len() > 32 {
        return Err("profile_domain must be <= 32 chars".to_string());
    }
    if !normalized
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(
            "profile_domain must contain only lowercase letters, digits, '-' or '_'".to_string(),
        );
    }
    Ok(normalized)
}

fn is_valid_gateway_auth_key(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() >= 16
        && trimmed.len() <= 128
        && trimmed.is_ascii()
        && !trimmed.chars().any(char::is_whitespace)
}

fn onboarding_gateway_auth_key_supplied(value: &str) -> bool {
    let trimmed = value.trim();
    is_valid_gateway_auth_key(trimmed)
        || trimmed.eq_ignore_ascii_case(ONBOARDING_GATEWAY_AUTH_FROM_CONFIG_MARKER)
}

fn apply_intent_overrides(
    config: &mut FrontdoorUserConfig,
    intent: &str,
    connected_wallet: &str,
    assumptions: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    let trimmed_intent = intent.trim();
    if trimmed_intent.is_empty() {
        assumptions.push("No intent provided; secure defaults were applied.".to_string());
        return;
    }

    let lower = trimmed_intent.to_ascii_lowercase();
    config.inference_summary = Some(trimmed_intent.to_string());
    config.inference_confidence = Some(0.68);
    if config.profile_name.trim().is_empty()
        || config
            .profile_name
            .trim()
            .eq_ignore_ascii_case("launchpad_profile")
    {
        config.profile_name = derive_profile_name_from_intent(trimmed_intent, connected_wallet);
        assumptions.push("Generated profile_name from intent and connected wallet.".to_string());
    }

    if contains_any_lower(
        &lower,
        &["conservative", "low risk", "capital preservation", "safer"],
    ) {
        config.paper_live_policy = "paper_only".to_string();
        config.max_position_size_usd = 500;
        config.max_allocation_usd = 600;
        config.per_trade_notional_cap_usd = 75;
        config.leverage_cap = 2;
        config.max_leverage = 2;
        config.max_slippage_bps = 40;
        assumptions.push("Applied conservative risk limits from intent.".to_string());
    }

    if contains_any_lower(
        &lower,
        &["aggressive", "high risk", "high conviction", "degen"],
    ) {
        config.paper_live_policy = "paper_first".to_string();
        config.max_position_size_usd = 5_000;
        config.max_allocation_usd = 10_000;
        config.per_trade_notional_cap_usd = 1_500;
        config.leverage_cap = 10;
        config.max_leverage = 8;
        config.max_slippage_bps = 140;
        warnings.push("Aggressive profile inferred; review leverage and cap limits.".to_string());
    }

    if contains_any_lower(&lower, &["mainnet"]) {
        config.hyperliquid_network = "mainnet".to_string();
        assumptions.push("Selected mainnet network from intent.".to_string());
    } else if contains_any_lower(&lower, &["testnet"]) {
        config.hyperliquid_network = "testnet".to_string();
        assumptions.push("Selected testnet network from intent.".to_string());
    }

    if contains_any_lower(&lower, &["paper only", "paper-only"]) {
        config.paper_live_policy = "paper_only".to_string();
    } else if contains_any_lower(&lower, &["paper first", "paper-first"]) {
        config.paper_live_policy = "paper_first".to_string();
    } else if contains_any_lower(
        &lower,
        &[
            "live allowed",
            "allow live",
            "live trading",
            "go live",
            "production trading",
        ],
    ) {
        config.paper_live_policy = "live_allowed".to_string();
        warnings.push(
            "Intent requested live execution. Signer/policy gates still enforce runtime safety."
                .to_string(),
        );
    }

    if contains_any_lower(
        &lower,
        &[
            "copytrading",
            "copy trading",
            "copy-trading",
            "follow trader",
        ],
    ) {
        config.information_sharing_scope = "signals_and_execution".to_string();
        assumptions
            .push("Enabled signals_and_execution sharing for copy-trading intent.".to_string());
    }

    if contains_any_lower(&lower, &["full audit", "audit trail", "strict audit"]) {
        config.information_sharing_scope = "full_audit".to_string();
        assumptions.push("Enabled full_audit information sharing.".to_string());
    }

    if contains_any_lower(
        &lower,
        &[
            "disable memory",
            "no memory",
            "memory off",
            "without memory",
        ],
    ) {
        config.enable_memory = false;
    } else if contains_any_lower(&lower, &["enable memory", "memory on", "with memory"]) {
        config.enable_memory = true;
    }

    if contains_any_lower(&lower, &["dual wallet", "dual mode", "dual custody"]) {
        config.custody_mode = "dual_mode".to_string();
        assumptions.push("Switched custody mode to dual_mode from intent.".to_string());
    } else if contains_any_lower(
        &lower,
        &["operator wallet", "operator custody", "managed wallet"],
    ) {
        config.custody_mode = "operator_wallet".to_string();
        assumptions.push("Switched custody mode to operator_wallet from intent.".to_string());
    } else if contains_any_lower(
        &lower,
        &["user wallet", "self custody", "self-custody", "own wallet"],
    ) {
        config.custody_mode = "user_wallet".to_string();
        assumptions.push("Switched custody mode to user_wallet from intent.".to_string());
    }

    if contains_any_lower(
        &lower,
        &["fallback only", "offline verification", "signed fallback"],
    ) {
        config.verification_backend = "fallback_only".to_string();
        config.verification_fallback_enabled = true;
        assumptions.push("Using fallback-only verification mode from intent.".to_string());
    } else if contains_any_lower(&lower, &["eigencloud", "eigen cloud", "attestation"]) {
        config.verification_backend = "eigencloud_primary".to_string();
    }

    let mut symbols = infer_symbols_from_intent(trimmed_intent);
    if contains_any_lower(&lower, &["btc-only", "btc only", "bitcoin only"]) {
        symbols = vec!["BTC".to_string()];
    } else if symbols.is_empty() && contains_any_lower(&lower, &["multi-asset", "diversified"]) {
        symbols = vec![
            "BTC".to_string(),
            "ETH".to_string(),
            "SOL".to_string(),
            "AVAX".to_string(),
        ];
    }
    if !symbols.is_empty() {
        config.symbol_allowlist = symbols;
        config.symbol_denylist.clear();
        assumptions.push("Updated symbol_allowlist from inferred intent assets.".to_string());
    }

    if config.user_wallet_address.is_none() {
        config.user_wallet_address = Some(connected_wallet.to_string());
    }
}

fn infer_symbols_from_intent(intent: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for token in intent.split(|c: char| !c.is_ascii_alphanumeric()) {
        let raw = token.trim();
        if raw.len() < 2 || raw.len() > 12 {
            continue;
        }
        let lower = raw.to_ascii_lowercase();
        let mapped = match lower.as_str() {
            "bitcoin" | "btc" => Some("BTC"),
            "ethereum" | "eth" => Some("ETH"),
            "solana" | "sol" => Some("SOL"),
            "avalanche" | "avax" => Some("AVAX"),
            "sui" => Some("SUI"),
            "xrp" => Some("XRP"),
            "doge" | "dogecoin" => Some("DOGE"),
            "ada" | "cardano" => Some("ADA"),
            "arb" | "arbitrum" => Some("ARB"),
            "apt" | "aptos" => Some("APT"),
            "link" | "chainlink" => Some("LINK"),
            "monad" | "mon" => Some("MON"),
            _ => {
                if raw
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                {
                    Some(raw)
                } else {
                    None
                }
            }
        };
        if let Some(symbol) = mapped
            && !symbols.iter().any(|v: &String| v == symbol)
        {
            symbols.push(symbol.to_string());
        }
    }
    symbols
}

fn derive_profile_name_from_intent(intent: &str, connected_wallet: &str) -> String {
    const PROFILE_NAME_STOP_WORDS: &[&str] = &[
        "with",
        "that",
        "this",
        "from",
        "into",
        "your",
        "user",
        "launch",
        "agent",
        "session",
        "profile",
        "and",
        "for",
        "the",
    ];

    let wallet_hex = connected_wallet
        .trim()
        .trim_start_matches("0x")
        .to_ascii_lowercase();
    let wallet_tail = if wallet_hex.len() >= 6 {
        &wallet_hex[wallet_hex.len() - 6..]
    } else {
        "wallet"
    };

    let mut parts = Vec::new();
    for token in intent.split(|c: char| !c.is_ascii_alphanumeric()) {
        let normalized = token.trim().to_ascii_lowercase();
        if normalized.len() < 3 {
            continue;
        }
        if PROFILE_NAME_STOP_WORDS.contains(&normalized.as_str()) {
            continue;
        }
        if !parts.iter().any(|part: &String| part == &normalized) {
            parts.push(normalized);
        }
        if parts.len() >= 4 {
            break;
        }
    }

    if parts.is_empty() {
        parts.push("enclagent".to_string());
    }

    let mut candidate = format!("{}-{}", parts.join("-"), wallet_tail);
    if candidate.len() > 64 {
        candidate = candidate.chars().take(64).collect();
        candidate = candidate.trim_end_matches('-').to_string();
    }
    if candidate.trim().is_empty() {
        return format!("enclagent-{}", wallet_tail);
    }
    candidate
}

fn contains_any_lower(haystack: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|needle| haystack.contains(needle))
}

fn normalize_suggested_config(
    config: &mut FrontdoorUserConfig,
    connected_wallet: &str,
    assumptions: &mut Vec<String>,
) {
    if !FRONTDOOR_SUPPORTED_CONFIG_VERSIONS.contains(&config.config_version) {
        config.config_version = FRONTDOOR_CURRENT_CONFIG_VERSION;
        assumptions.push("Normalized config_version to current contract version.".to_string());
    }

    match normalize_domain_name(&config.profile_domain) {
        Ok(domain) => config.profile_domain = domain,
        Err(_) => {
            config.profile_domain = "general".to_string();
            assumptions.push("Normalized invalid profile_domain to general.".to_string());
        }
    }

    let hyperliquid_profile = config.profile_domain == "hyperliquid";

    if config.profile_name.trim().is_empty()
        || config
            .profile_name
            .trim()
            .eq_ignore_ascii_case("launchpad_profile")
    {
        let seed = config.inference_summary.as_deref().unwrap_or("enclagent session");
        config.profile_name = derive_profile_name_from_intent(seed, connected_wallet);
        assumptions.push("Generated profile_name from intent and connected wallet.".to_string());
    }
    if config.profile_name.len() > 64 {
        config.profile_name = config.profile_name.chars().take(64).collect();
    }

    if hyperliquid_profile {
        if config.hyperliquid_network != "mainnet" && config.hyperliquid_network != "testnet" {
            config.hyperliquid_network = "testnet".to_string();
        }
        if !matches!(
            config.paper_live_policy.as_str(),
            "paper_only" | "paper_first" | "live_allowed"
        ) {
            config.paper_live_policy = "paper_only".to_string();
        }
    } else {
        config.hyperliquid_network = "testnet".to_string();
        config.paper_live_policy = "paper_only".to_string();
        config.symbol_allowlist.clear();
        config.symbol_denylist.clear();
    }

    config.request_timeout_ms = config.request_timeout_ms.clamp(1_000, 120_000);
    config.max_retries = config.max_retries.min(10);
    config.retry_backoff_ms = config.retry_backoff_ms.min(30_000);

    if hyperliquid_profile {
        config.max_position_size_usd = config.max_position_size_usd.max(1);
        config.leverage_cap = config.leverage_cap.clamp(1, 20);
        config.max_allocation_usd = config.max_allocation_usd.max(1);
        config.per_trade_notional_cap_usd = config.per_trade_notional_cap_usd.max(1);
        if config.per_trade_notional_cap_usd > config.max_allocation_usd {
            config.per_trade_notional_cap_usd = config.max_allocation_usd;
        }
        config.max_leverage = config.max_leverage.clamp(1, 20);
        if config.max_leverage > config.leverage_cap {
            config.max_leverage = config.leverage_cap;
        }
        config.max_slippage_bps = config.max_slippage_bps.clamp(1, 5_000);

        if config.symbol_allowlist.is_empty() {
            config.symbol_allowlist = vec!["BTC".to_string(), "ETH".to_string()];
        }
    }
    if !matches!(
        config.custody_mode.as_str(),
        "operator_wallet" | "user_wallet" | "dual_mode"
    ) {
        config.custody_mode = "user_wallet".to_string();
    }

    if (config.custody_mode == "user_wallet" || config.custody_mode == "dual_mode")
        && config.user_wallet_address.is_none()
    {
        config.user_wallet_address = Some(connected_wallet.to_string());
    }
    if (config.custody_mode == "operator_wallet" || config.custody_mode == "dual_mode")
        && config.operator_wallet_address.is_none()
    {
        config.custody_mode = "user_wallet".to_string();
        assumptions.push(
            "Operator wallet missing; normalized custody_mode to user_wallet for safe launch."
                .to_string(),
        );
    }

    if !matches!(
        config.information_sharing_scope.as_str(),
        "none" | "signals_only" | "signals_and_execution" | "full_audit"
    ) {
        config.information_sharing_scope = "signals_only".to_string();
    }
    if !matches!(
        config.kill_switch_behavior.as_str(),
        "pause_agent" | "cancel_open_orders" | "cancel_and_flatten"
    ) {
        config.kill_switch_behavior = "pause_agent".to_string();
    }

    if !is_valid_gateway_auth_key(&config.gateway_auth_key) {
        config.gateway_auth_key = generate_gateway_auth_key();
    }

    if !matches!(
        config.verification_backend.as_str(),
        "eigencloud_primary" | "fallback_only"
    ) {
        config.verification_backend = "eigencloud_primary".to_string();
    }
    if config.verification_backend == "fallback_only" {
        config.verification_fallback_enabled = true;
    }
    if !matches!(
        config.verification_eigencloud_auth_scheme.as_str(),
        "bearer" | "api_key"
    ) {
        config.verification_eigencloud_auth_scheme = "bearer".to_string();
    }
    config.verification_eigencloud_timeout_ms =
        config.verification_eigencloud_timeout_ms.clamp(1, 120_000);

    if hyperliquid_profile
        && config.hyperliquid_network == "mainnet"
        && config.paper_live_policy == "live_allowed"
        && config.max_position_size_usd > 1_000_000
    {
        config.max_position_size_usd = 1_000_000;
    }

    config.accept_terms = true;
}

fn validate_user_config(config: &FrontdoorUserConfig) -> Result<(), String> {
    if !FRONTDOOR_SUPPORTED_CONFIG_VERSIONS.contains(&config.config_version) {
        return Err(format!(
            "config_version must be one of {}",
            FRONTDOOR_SUPPORTED_CONFIG_VERSIONS
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let domain = normalize_domain_name(&config.profile_domain)?;
    let hyperliquid_profile = domain == "hyperliquid";
    if config.domain_overrides.len() > 32 {
        return Err("domain_overrides must include at most 32 keys".to_string());
    }
    for (key, value) in &config.domain_overrides {
        if key.trim().is_empty() {
            return Err("domain_overrides keys must be non-empty".to_string());
        }
        if key.len() > 64 {
            return Err("domain_overrides keys must be <= 64 chars".to_string());
        }
        let serialized = serde_json::to_string(value)
            .map_err(|e| format!("domain_overrides value serialization failed: {e}"))?;
        if serialized.len() > 4_096 {
            return Err(format!(
                "domain_overrides value for key '{key}' exceeds 4096 bytes"
            ));
        }
    }
    if let Some(summary) = config.inference_summary.as_deref()
        && summary.len() > 2_048
    {
        return Err("inference_summary must be <= 2048 chars".to_string());
    }
    if let Some(confidence) = config.inference_confidence
        && !(0.0..=1.0).contains(&confidence)
    {
        return Err("inference_confidence must be between 0.0 and 1.0".to_string());
    }
    if config.inference_warnings.len() > 32 {
        return Err("inference_warnings must include at most 32 entries".to_string());
    }
    if config.inference_warnings.iter().any(|v| v.len() > 512) {
        return Err("inference_warnings entries must be <= 512 chars".to_string());
    }

    if config.profile_name.trim().is_empty() {
        return Err("profile_name is required".to_string());
    }
    if config.profile_name.len() > 64 {
        return Err("profile_name must be <= 64 chars".to_string());
    }

    if hyperliquid_profile {
        let network = config.hyperliquid_network.trim().to_ascii_lowercase();
        if network != "testnet" && network != "mainnet" {
            return Err("hyperliquid_network must be testnet or mainnet".to_string());
        }

        let policy = config.paper_live_policy.trim().to_ascii_lowercase();
        if policy != "paper_only" && policy != "paper_first" && policy != "live_allowed" {
            return Err(
                "paper_live_policy must be paper_only, paper_first, or live_allowed".to_string(),
            );
        }
        if network == "mainnet"
            && policy == "live_allowed"
            && config.max_position_size_usd > 1_000_000
        {
            return Err(
                "mainnet live_allowed sessions require max_position_size_usd <= 1000000"
                    .to_string(),
            );
        }

        if let Some(api) = config.hyperliquid_api_base_url.as_deref() {
            validate_optional_url(api, &["http", "https"], "hyperliquid_api_base_url")?;
        }
        if let Some(ws) = config.hyperliquid_ws_url.as_deref() {
            validate_optional_url(ws, &["ws", "wss"], "hyperliquid_ws_url")?;
        }
        if config.request_timeout_ms < 1_000 || config.request_timeout_ms > 120_000 {
            return Err("request_timeout_ms must be between 1000 and 120000".to_string());
        }
        if config.max_retries > 10 {
            return Err("max_retries must be between 0 and 10".to_string());
        }
        if config.retry_backoff_ms > 30_000 {
            return Err("retry_backoff_ms must be <= 30000".to_string());
        }

        if config.max_position_size_usd == 0 {
            return Err("max_position_size_usd must be > 0".to_string());
        }
        if config.leverage_cap == 0 || config.leverage_cap > 20 {
            return Err("leverage_cap must be between 1 and 20".to_string());
        }

        if config.max_allocation_usd == 0 {
            return Err("max_allocation_usd must be > 0".to_string());
        }
        if config.per_trade_notional_cap_usd == 0 {
            return Err("per_trade_notional_cap_usd must be > 0".to_string());
        }
        if config.per_trade_notional_cap_usd > config.max_allocation_usd {
            return Err("per_trade_notional_cap_usd must be <= max_allocation_usd".to_string());
        }
        if config.max_leverage == 0 || config.max_leverage > 20 {
            return Err("max_leverage must be between 1 and 20".to_string());
        }
        if config.max_leverage > config.leverage_cap {
            return Err("max_leverage must be <= leverage_cap".to_string());
        }
        if config.max_slippage_bps == 0 || config.max_slippage_bps > 5_000 {
            return Err("max_slippage_bps must be between 1 and 5000".to_string());
        }

        let allowlist = normalize_symbols(&config.symbol_allowlist, "symbol_allowlist", true)?;
        let denylist = normalize_symbols(&config.symbol_denylist, "symbol_denylist", false)?;
        if !allowlist.is_disjoint(&denylist) {
            return Err("symbol_allowlist and symbol_denylist must not overlap".to_string());
        }
    }

    let custody_mode = config.custody_mode.trim().to_ascii_lowercase();
    if custody_mode != "operator_wallet"
        && custody_mode != "user_wallet"
        && custody_mode != "dual_mode"
    {
        return Err("custody_mode must be operator_wallet, user_wallet, or dual_mode".to_string());
    }
    let operator_wallet = normalize_optional_wallet(
        config.operator_wallet_address.as_deref(),
        "operator_wallet_address",
    )?;
    let user_wallet =
        normalize_optional_wallet(config.user_wallet_address.as_deref(), "user_wallet_address")?;
    normalize_optional_wallet(config.vault_address.as_deref(), "vault_address")?;
    if (custody_mode == "operator_wallet" || custody_mode == "dual_mode")
        && operator_wallet.is_none()
    {
        return Err(
            "operator_wallet_address is required for custody_mode operator_wallet/dual_mode"
                .to_string(),
        );
    }
    if (custody_mode == "user_wallet" || custody_mode == "dual_mode") && user_wallet.is_none() {
        return Err(
            "user_wallet_address is required for custody_mode user_wallet/dual_mode".to_string(),
        );
    }

    let information_sharing_scope = config.information_sharing_scope.trim().to_ascii_lowercase();
    if information_sharing_scope != "none"
        && information_sharing_scope != "signals_only"
        && information_sharing_scope != "signals_and_execution"
        && information_sharing_scope != "full_audit"
    {
        return Err("information_sharing_scope must be none, signals_only, signals_and_execution, or full_audit".to_string());
    }

    let kill_switch_behavior = config.kill_switch_behavior.trim().to_ascii_lowercase();
    if kill_switch_behavior != "pause_agent"
        && kill_switch_behavior != "cancel_open_orders"
        && kill_switch_behavior != "cancel_and_flatten"
    {
        return Err(
            "kill_switch_behavior must be pause_agent, cancel_open_orders, or cancel_and_flatten"
                .to_string(),
        );
    }

    let gateway_auth_key = config.gateway_auth_key.trim();
    if !is_valid_gateway_auth_key(gateway_auth_key) {
        return Err("gateway_auth_key must be printable ASCII with no whitespace".to_string());
    }
    if let Some(token) = config.eigencloud_auth_key.as_deref()
        && !token.trim().is_empty()
        && token.trim().len() < 8
    {
        return Err("eigencloud_auth_key, when set, must be at least 8 chars".to_string());
    }
    let verification_backend = config.verification_backend.trim().to_ascii_lowercase();
    if verification_backend != "eigencloud_primary" && verification_backend != "fallback_only" {
        return Err("verification_backend must be eigencloud_primary or fallback_only".to_string());
    }
    if let Some(endpoint) = config.verification_eigencloud_endpoint.as_deref() {
        validate_optional_url(
            endpoint,
            &["http", "https"],
            "verification_eigencloud_endpoint",
        )?;
    }
    let verification_auth_scheme = config
        .verification_eigencloud_auth_scheme
        .trim()
        .to_ascii_lowercase();
    if verification_auth_scheme != "bearer" && verification_auth_scheme != "api_key" {
        return Err("verification_eigencloud_auth_scheme must be bearer or api_key".to_string());
    }
    if config.verification_eigencloud_timeout_ms == 0
        || config.verification_eigencloud_timeout_ms > 120_000
    {
        return Err("verification_eigencloud_timeout_ms must be between 1 and 120000".to_string());
    }
    if verification_backend == "fallback_only" && !config.verification_fallback_enabled {
        return Err(
            "verification_fallback_enabled must be true when verification_backend=fallback_only"
                .to_string(),
        );
    }
    if let Some(value) = config.verification_fallback_signing_key_id.as_deref()
        && value.len() > 128
    {
        return Err("verification_fallback_signing_key_id must be <= 128 chars".to_string());
    }
    if let Some(value) = config.verification_fallback_chain_path.as_deref() {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(
                "verification_fallback_chain_path must be omitted or non-empty".to_string(),
            );
        }
        if trimmed.contains(['\n', '\r']) {
            return Err("verification_fallback_chain_path must not include newlines".to_string());
        }
    }

    if !config.accept_terms {
        return Err("accept_terms must be true".to_string());
    }
    Ok(())
}

fn validate_wallet_association(
    config: &FrontdoorUserConfig,
    connected_wallet: &str,
) -> Result<(), String> {
    let connected = normalize_wallet_address(connected_wallet)
        .ok_or_else(|| "connected wallet is not a valid EVM address".to_string())?;
    let custody_mode = config.custody_mode.trim().to_ascii_lowercase();
    if custody_mode != "user_wallet" && custody_mode != "dual_mode" {
        return Ok(());
    }
    let configured =
        normalize_optional_wallet(config.user_wallet_address.as_deref(), "user_wallet_address")?;
    if configured.as_deref() != Some(connected.as_str()) {
        return Err(
            "user_wallet_address must match the connected wallet for user_wallet/dual_mode"
                .to_string(),
        );
    }
    Ok(())
}

fn normalize_optional_wallet(value: Option<&str>, field: &str) -> Result<Option<String>, String> {
    let Some(raw) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    normalize_wallet_address(raw)
        .map(Some)
        .ok_or_else(|| format!("{field} must be a 0x-prefixed 40-hex address"))
}

fn validate_optional_url(value: &str, schemes: &[&str], field: &str) -> Result<(), String> {
    let parsed = Url::parse(value).map_err(|e| format!("{field} is not a valid URL: {e}"))?;
    if !schemes.contains(&parsed.scheme()) {
        return Err(format!(
            "{field} must use {} scheme",
            if schemes.len() == 1 {
                schemes[0].to_string()
            } else {
                schemes.join(" or ")
            }
        ));
    }
    if parsed.host_str().is_none() {
        return Err(format!("{field} must include a hostname"));
    }
    Ok(())
}

fn normalize_symbols(
    symbols: &[String],
    field: &str,
    required_non_empty: bool,
) -> Result<HashSet<String>, String> {
    let mut normalized = HashSet::new();
    for symbol in symbols {
        let s = symbol.trim().to_ascii_uppercase();
        if s.is_empty() {
            continue;
        }
        if s.len() > 24 {
            return Err(format!("{field} symbol '{s}' is too long"));
        }
        if !s.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(format!(
                "{field} symbol '{s}' must be alphanumeric (example: BTC, ETH)"
            ));
        }
        normalized.insert(s);
    }
    if required_non_empty && normalized.is_empty() {
        return Err(format!("{field} must include at least one symbol"));
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use k256::ecdsa::SigningKey;
    use tempfile::tempdir;

    use crate::channels::web::types::{
        FrontdoorChallengeRequest, FrontdoorOnboardingChatRequest, FrontdoorRuntimeControlRequest,
        FrontdoorUserConfig, FrontdoorVerifyRequest,
    };

    #[test]
    fn wallet_normalization_and_validation() {
        let valid = "0x9431Cf5DA0CE60664661341db650763B08286B18";
        let normalized = normalize_wallet_address(valid).expect("valid wallet");
        assert_eq!(normalized, "0x9431cf5da0ce60664661341db650763b08286b18");
        assert!(normalize_wallet_address("0x123").is_none());
        assert!(normalize_wallet_address("9431cf5da0ce60664661341db650763b08286b18").is_none());
    }

    #[test]
    fn signature_shape_validation() {
        let sig = format!("0x{}", "a".repeat(130));
        assert!(is_signature_like(&sig));
        assert!(!is_signature_like("0xabc"));
        assert!(!is_signature_like("abc"));
        assert!(!is_signature_like(&format!("0x{}", "z".repeat(130))));
    }

    #[test]
    fn verifies_wallet_signature_recovery() {
        let private_key = decode_hex_prefixed(
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
        )
        .expect("private key");
        let signing_key = SigningKey::from_slice(&private_key).expect("signing key");
        let wallet =
            ethereum_address_from_verifying_key(signing_key.verifying_key()).expect("wallet");
        let message = "Enclagent test message";
        let prehash = eip191_personal_sign_hash(message);
        let (sig, recid) = signing_key
            .sign_prehash_recoverable(&prehash)
            .expect("sign message");
        let mut sig_bytes = sig.to_bytes().to_vec();
        sig_bytes.push(recid.to_byte() + 27);
        let signature = format!("0x{}", encode_hex_lower(&sig_bytes));

        verify_wallet_signature(message, &signature, &wallet).expect("signature should verify");
        assert!(
            verify_wallet_signature(
                message,
                &signature,
                "0x0000000000000000000000000000000000000001"
            )
            .is_err()
        );
    }

    #[test]
    fn frontdoor_flow_reaches_ready_state_after_valid_signature() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let tmp = tempdir().expect("tempdir");
            let store_path = tmp.path().join("wallet_sessions.json");
            let service = FrontdoorService::new_for_tests(
                FrontdoorConfig {
                    require_privy: false,
                    privy_app_id: None,
                    privy_client_id: None,
                    provision_command: None,
                    default_instance_url: Some(
                        "https://session.example/gateway?token=demo".to_string(),
                    ),
                    allow_default_instance_fallback: true,
                    verify_app_base_url: Some(
                        "https://verify-sepolia.eigencloud.xyz/app".to_string(),
                    ),
                    session_ttl_secs: 900,
                    poll_interval_ms: 100,
                },
                store_path,
            );

            let private_key = decode_hex_prefixed(
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
            )
            .expect("private key");
            let signing_key = SigningKey::from_slice(&private_key).expect("signing key");
            let wallet =
                ethereum_address_from_verifying_key(signing_key.verifying_key()).expect("wallet");

            let challenge = service
                .create_challenge(FrontdoorChallengeRequest {
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    chain_id: Some(1),
                })
                .await
                .expect("challenge");
            let session_uuid = Uuid::parse_str(&challenge.session_id).expect("session uuid");

            let prehash = eip191_personal_sign_hash(&challenge.message);
            let (sig, recid) = signing_key
                .sign_prehash_recoverable(&prehash)
                .expect("sign challenge");
            let mut sig_bytes = sig.to_bytes().to_vec();
            sig_bytes.push(recid.to_byte() + 27);
            let signature = format!("0x{}", encode_hex_lower(&sig_bytes));

            service
                .clone()
                .verify_and_start(FrontdoorVerifyRequest {
                    session_id: challenge.session_id.clone(),
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    privy_identity_token: None,
                    privy_access_token: None,
                    message: challenge.message.clone(),
                    signature,
                    config: sample_user_config(&wallet),
                })
                .await
                .expect("verify and start");

            let mut ready = None;
            for _ in 0..40 {
                let session = service
                    .get_session(session_uuid)
                    .await
                    .expect("session should exist");
                if session.status == "ready" {
                    ready = Some(session);
                    break;
                }
                assert_ne!(session.status, "failed", "session failed unexpectedly");
                assert_ne!(session.status, "expired", "session expired unexpectedly");
                tokio::time::sleep(Duration::from_millis(25)).await;
            }

            let ready = ready.expect("session should become ready");
            assert_eq!(
                ready.instance_url.as_deref(),
                Some("https://session.example/gateway?token=demo")
            );
            assert_eq!(ready.wallet_address, wallet);
            assert_eq!(ready.profile_name.as_deref(), Some("demo_profile"));
            assert_eq!(ready.provisioning_source, "default_instance_url");
            assert!(!ready.dedicated_instance);
            assert!(!ready.launched_on_eigencloud);
            assert_eq!(ready.verification_backend, "eigencloud_primary");
            assert_eq!(ready.verification_level, "primary_plus_signed_fallback");
        });
    }

    #[test]
    fn frontdoor_verify_is_idempotent_after_ready() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let tmp = tempdir().expect("tempdir");
            let store_path = tmp.path().join("wallet_sessions.json");
            let service = FrontdoorService::new_for_tests(
                FrontdoorConfig {
                    require_privy: false,
                    privy_app_id: None,
                    privy_client_id: None,
                    provision_command: None,
                    default_instance_url: Some(
                        "https://session.example/gateway?token=demo".to_string(),
                    ),
                    allow_default_instance_fallback: true,
                    verify_app_base_url: Some(
                        "https://verify-sepolia.eigencloud.xyz/app".to_string(),
                    ),
                    session_ttl_secs: 900,
                    poll_interval_ms: 100,
                },
                store_path,
            );

            let private_key = decode_hex_prefixed(
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
            )
            .expect("private key");
            let signing_key = SigningKey::from_slice(&private_key).expect("signing key");
            let wallet =
                ethereum_address_from_verifying_key(signing_key.verifying_key()).expect("wallet");

            let challenge = service
                .create_challenge(FrontdoorChallengeRequest {
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    chain_id: Some(1),
                })
                .await
                .expect("challenge");
            let session_uuid = Uuid::parse_str(&challenge.session_id).expect("session uuid");

            let prehash = eip191_personal_sign_hash(&challenge.message);
            let (sig, recid) = signing_key
                .sign_prehash_recoverable(&prehash)
                .expect("sign challenge");
            let mut sig_bytes = sig.to_bytes().to_vec();
            sig_bytes.push(recid.to_byte() + 27);
            let signature = format!("0x{}", encode_hex_lower(&sig_bytes));

            let verify_response = service
                .clone()
                .verify_and_start(FrontdoorVerifyRequest {
                    session_id: challenge.session_id.clone(),
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    privy_identity_token: None,
                    privy_access_token: None,
                    message: challenge.message.clone(),
                    signature: signature.clone(),
                    config: sample_user_config(&wallet),
                })
                .await
                .expect("first verify and start");
            assert_eq!(verify_response.status, "provisioning");

            for _ in 0..40 {
                let session = service
                    .get_session(session_uuid)
                    .await
                    .expect("session should exist");
                if session.status == "ready" {
                    break;
                }
                assert_ne!(session.status, "failed", "session failed unexpectedly");
                assert_ne!(session.status, "expired", "session expired unexpectedly");
                tokio::time::sleep(Duration::from_millis(25)).await;
            }

            let timeline_before = service
                .session_timeline(session_uuid)
                .await
                .expect("timeline before replay");
            let provisioning_started_before = timeline_before
                .events
                .iter()
                .filter(|event| event.event_type == "provisioning_started")
                .count();
            assert_eq!(provisioning_started_before, 1);

            let replay_response = service
                .clone()
                .verify_and_start(FrontdoorVerifyRequest {
                    session_id: challenge.session_id,
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    privy_identity_token: None,
                    privy_access_token: None,
                    message: challenge.message,
                    signature,
                    config: sample_user_config(&wallet),
                })
                .await
                .expect("replay verify should be idempotent");
            assert_eq!(replay_response.status, "ready");

            tokio::time::sleep(Duration::from_millis(50)).await;

            let timeline_after = service
                .session_timeline(session_uuid)
                .await
                .expect("timeline after replay");
            let provisioning_started_after = timeline_after
                .events
                .iter()
                .filter(|event| event.event_type == "provisioning_started")
                .count();
            assert_eq!(provisioning_started_after, 1);
        });
    }

    #[test]
    fn frontdoor_privy_mode_accepts_wallet_signature_without_siwe_tokens() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let tmp = tempdir().expect("tempdir");
            let store_path = tmp.path().join("wallet_sessions.json");
            let service = FrontdoorService::new_for_tests(
                FrontdoorConfig {
                    require_privy: true,
                    privy_app_id: Some("app_123".to_string()),
                    privy_client_id: None,
                    provision_command: None,
                    default_instance_url: Some(
                        "https://session.example/gateway?token=demo".to_string(),
                    ),
                    allow_default_instance_fallback: true,
                    verify_app_base_url: None,
                    session_ttl_secs: 900,
                    poll_interval_ms: 100,
                },
                store_path,
            );

            let private_key = decode_hex_prefixed(
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
            )
            .expect("private key");
            let signing_key = SigningKey::from_slice(&private_key).expect("signing key");
            let wallet =
                ethereum_address_from_verifying_key(signing_key.verifying_key()).expect("wallet");

            let challenge = service
                .create_challenge(FrontdoorChallengeRequest {
                    wallet_address: wallet.clone(),
                    privy_user_id: Some(format!("wallet:{wallet}")),
                    chain_id: Some(1),
                })
                .await
                .expect("challenge");

            let prehash = eip191_personal_sign_hash(&challenge.message);
            let (sig, recid) = signing_key
                .sign_prehash_recoverable(&prehash)
                .expect("sign challenge");
            let mut sig_bytes = sig.to_bytes().to_vec();
            sig_bytes.push(recid.to_byte() + 27);
            let signature = format!("0x{}", encode_hex_lower(&sig_bytes));

            service
                .clone()
                .verify_and_start(FrontdoorVerifyRequest {
                    session_id: challenge.session_id,
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    privy_identity_token: None,
                    privy_access_token: None,
                    message: challenge.message,
                    signature,
                    config: sample_user_config(&wallet),
                })
                .await
                .expect("verify and start");
        });
    }

    #[test]
    fn intent_can_skip_dedicated_spinup_when_shared_fallback_is_available() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let tmp = tempdir().expect("tempdir");
            let store_path = tmp.path().join("wallet_sessions.json");
            let service = FrontdoorService::new_for_tests(
                FrontdoorConfig {
                    require_privy: false,
                    privy_app_id: None,
                    privy_client_id: None,
                    provision_command: Some(
                        "printf 'https://dedicated.example/gateway?token=dedicated\\n'".to_string(),
                    ),
                    default_instance_url: Some(
                        "https://shared.example/gateway?token=shared".to_string(),
                    ),
                    allow_default_instance_fallback: true,
                    verify_app_base_url: None,
                    session_ttl_secs: 900,
                    poll_interval_ms: 100,
                },
                store_path,
            );

            let private_key = decode_hex_prefixed(
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
            )
            .expect("private key");
            let signing_key = SigningKey::from_slice(&private_key).expect("signing key");
            let wallet =
                ethereum_address_from_verifying_key(signing_key.verifying_key()).expect("wallet");

            let challenge = service
                .create_challenge(FrontdoorChallengeRequest {
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    chain_id: Some(1),
                })
                .await
                .expect("challenge");
            let session_uuid = Uuid::parse_str(&challenge.session_id).expect("session uuid");

            let prehash = eip191_personal_sign_hash(&challenge.message);
            let (sig, recid) = signing_key
                .sign_prehash_recoverable(&prehash)
                .expect("sign challenge");
            let mut sig_bytes = sig.to_bytes().to_vec();
            sig_bytes.push(recid.to_byte() + 27);
            let signature = format!("0x{}", encode_hex_lower(&sig_bytes));

            let mut cfg = sample_user_config(&wallet);
            cfg.inference_summary =
                Some("shared runtime only, no enclave spinup; reuse existing instance".to_string());
            service
                .clone()
                .verify_and_start(FrontdoorVerifyRequest {
                    session_id: challenge.session_id.clone(),
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    privy_identity_token: None,
                    privy_access_token: None,
                    message: challenge.message.clone(),
                    signature,
                    config: cfg,
                })
                .await
                .expect("verify and start");

            let mut ready = None;
            for _ in 0..40 {
                let session = service
                    .get_session(session_uuid)
                    .await
                    .expect("session should exist");
                if session.status == "ready" {
                    ready = Some(session);
                    break;
                }
                assert_ne!(session.status, "failed", "session failed unexpectedly");
                assert_ne!(session.status, "expired", "session expired unexpectedly");
                tokio::time::sleep(Duration::from_millis(25)).await;
            }

            let ready = ready.expect("session should become ready");
            assert_eq!(ready.provisioning_source, "default_instance_url");
            assert_eq!(
                ready.instance_url.as_deref(),
                Some("https://shared.example/gateway?token=shared")
            );
            assert!(!ready.dedicated_instance);
        });
    }

    #[test]
    fn provisioning_decision_prefers_shared_runtime_for_shared_hints() {
        let decision = decide_provisioning_decision(Some("No enclave please; shared runtime only"));
        assert!(decision.prefer_shared_runtime);
    }

    #[test]
    fn provisioning_decision_defaults_to_dedicated_when_hint_is_absent() {
        let decision = decide_provisioning_decision(Some("Launch isolated dedicated enclave"));
        assert!(!decision.prefer_shared_runtime);
    }

    #[test]
    fn extract_provision_result_from_command_output() {
        let json =
            r#"{"instance_url":"https://session.example/gateway?token=abc","app_id":"0xabc"}"#;
        let result =
            execute_provision_output(json, Some("https://verify-sepolia.eigencloud.xyz/app"))
                .expect("json output");
        assert_eq!(
            result.instance_url,
            "https://session.example/gateway?token=abc"
        );
        assert_eq!(
            result.verify_url.as_deref(),
            Some("https://verify-sepolia.eigencloud.xyz/app/0xabc")
        );
        assert_eq!(result.eigen_app_id.as_deref(), Some("0xabc"));
        assert!(result.app_url.is_none());

        let json_with_app_only =
            r#"{"app_url":"https://sepolia.eigencloud.xyz/app/0xdef","app_id":"0xdef"}"#;
        let result = execute_provision_output(
            json_with_app_only,
            Some("https://verify-sepolia.eigencloud.xyz/app"),
        )
        .expect("json output with app url");
        assert_eq!(
            result.instance_url,
            "https://sepolia.eigencloud.xyz/app/0xdef"
        );
        assert_eq!(
            result.app_url.as_deref(),
            Some("https://sepolia.eigencloud.xyz/app/0xdef")
        );
        assert_eq!(
            result.verify_url.as_deref(),
            Some("https://verify-sepolia.eigencloud.xyz/app/0xdef")
        );
        assert_eq!(result.eigen_app_id.as_deref(), Some("0xdef"));

        let plain = "line1\nhttps://foo.example/path\n";
        let result = execute_provision_output(plain, None).expect("plain url");
        assert_eq!(result.instance_url, "https://foo.example/path");
        assert!(result.app_url.is_none());
        assert!(result.verify_url.is_none());

        let verify_only = r#"{"instance_url":"https://verify-sepolia.eigencloud.xyz/app/0x1234","verify_url":"https://verify-sepolia.eigencloud.xyz/app/0x1234","app_id":"0x1234"}"#;
        let result = execute_provision_output(verify_only, None).expect("verify-only output");
        assert_eq!(
            result.instance_url,
            "https://sepolia.eigencloud.xyz/app/0x1234"
        );
        assert_eq!(
            result.app_url.as_deref(),
            Some("https://sepolia.eigencloud.xyz/app/0x1234")
        );
        assert_eq!(
            result.verify_url.as_deref(),
            Some("https://verify-sepolia.eigencloud.xyz/app/0x1234")
        );
    }

    #[test]
    fn build_provision_command_expands_single_quoted_placeholders() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let wallet = "0xe10e3def5348cb4151a8b99beebfd43646bade59".to_string();
            let config = sample_user_config(&wallet);
            let session_id = Uuid::new_v4();
            let input = ProvisionCommandInput {
                session_id,
                wallet: &wallet,
                privy_user_id: None,
                privy_identity_token: None,
                privy_access_token: None,
                chain_id: 1,
                version: 1,
                config: &config,
                verify_base_url: Some("https://verify-sepolia.eigencloud.xyz/app"),
            };

            let mut command = build_provision_command(
                "printf '%s|%s|%s' '{wallet_address}' '{session_id}' '{config_b64}'",
                &input,
            )
            .expect("command");
            let output = command.output().await.expect("command output");
            assert!(output.status.success());
            let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
            let parts: Vec<&str> = stdout.trim().split('|').collect();
            assert_eq!(parts.len(), 3);
            assert_eq!(parts[0], wallet);
            assert_eq!(parts[1], session_id.to_string());

            let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(parts[2])
                .expect("decode config_b64");
            let decoded_cfg: FrontdoorUserConfig =
                serde_json::from_slice(&decoded).expect("decoded config json");
            assert_eq!(decoded_cfg.profile_name, config.profile_name);
            assert_eq!(decoded_cfg.gateway_auth_key, config.gateway_auth_key);
        });
    }

    #[test]
    fn suggest_config_maps_intent_to_validated_config() {
        let tmp = tempdir().expect("tempdir");
        let service = FrontdoorService::new_for_tests(
            FrontdoorConfig {
                require_privy: false,
                privy_app_id: None,
                privy_client_id: None,
                provision_command: None,
                default_instance_url: None,
                allow_default_instance_fallback: false,
                verify_app_base_url: None,
                session_ttl_secs: 900,
                poll_interval_ms: 1000,
            },
            tmp.path().join("wallet_sessions.json"),
        );

        let suggested = service
            .suggest_config(crate::channels::web::types::FrontdoorSuggestConfigRequest {
                wallet_address: "0x9431Cf5DA0CE60664661341db650763B08286B18".to_string(),
                intent: "conservative BTC-only paper trading with full audit".to_string(),
                domain: Some("hyperliquid".to_string()),
                gateway_auth_key: Some("supersecuregatewaykey01".to_string()),
                base_config: None,
            })
            .expect("suggest config");

        assert!(suggested.validated);
        assert_eq!(suggested.config.profile_domain, "hyperliquid");
        assert_eq!(suggested.config.paper_live_policy, "paper_only");
        assert_eq!(suggested.config.symbol_allowlist, vec!["BTC".to_string()]);
        assert_eq!(suggested.config.information_sharing_scope, "full_audit");
        assert_ne!(suggested.config.profile_name, "launchpad_profile");
        assert!(suggested.config.profile_name.contains("btc"));
    }

    #[test]
    fn suggest_config_enforces_connected_wallet_for_user_and_dual_custody() {
        let tmp = tempdir().expect("tempdir");
        let service = FrontdoorService::new_for_tests(
            FrontdoorConfig {
                require_privy: false,
                privy_app_id: None,
                privy_client_id: None,
                provision_command: None,
                default_instance_url: None,
                allow_default_instance_fallback: false,
                verify_app_base_url: None,
                session_ttl_secs: 900,
                poll_interval_ms: 1000,
            },
            tmp.path().join("wallet_sessions.json"),
        );

        let connected_wallet = "0x9431Cf5DA0CE60664661341db650763B08286B18";
        let mismatched_wallet = "0x8ba1f109551bD432803012645Ac136ddd64DBA72";

        let base_user_wallet = sample_user_config(mismatched_wallet);
        let user_wallet_err = service
            .suggest_config(crate::channels::web::types::FrontdoorSuggestConfigRequest {
                wallet_address: connected_wallet.to_string(),
                intent: "use my own wallet".to_string(),
                domain: Some("hyperliquid".to_string()),
                gateway_auth_key: None,
                base_config: Some(base_user_wallet),
            })
            .expect_err("mismatched user_wallet should fail");
        assert_eq!(
            user_wallet_err,
            "user_wallet_address must match the connected wallet for user_wallet/dual_mode"
        );

        let mut base_dual_mode = sample_user_config(mismatched_wallet);
        base_dual_mode.custody_mode = "dual_mode".to_string();
        base_dual_mode.operator_wallet_address =
            Some("0x1111111111111111111111111111111111111111".to_string());

        let dual_mode_err = service
            .suggest_config(crate::channels::web::types::FrontdoorSuggestConfigRequest {
                wallet_address: connected_wallet.to_string(),
                intent: "dual mode".to_string(),
                domain: Some("hyperliquid".to_string()),
                gateway_auth_key: None,
                base_config: Some(base_dual_mode),
            })
            .expect_err("mismatched dual_mode user wallet should fail");
        assert_eq!(
            dual_mode_err,
            "user_wallet_address must match the connected wallet for user_wallet/dual_mode"
        );
    }

    #[test]
    fn operator_and_dual_custody_require_operator_wallet_in_validation() {
        let connected_wallet = "0x9431Cf5DA0CE60664661341db650763B08286B18";

        let mut operator_wallet_mode = sample_user_config(connected_wallet);
        operator_wallet_mode.custody_mode = "operator_wallet".to_string();
        operator_wallet_mode.operator_wallet_address = None;
        let operator_wallet_err = validate_user_config(&operator_wallet_mode)
            .expect_err("operator_wallet must require operator wallet");
        assert_eq!(
            operator_wallet_err,
            "operator_wallet_address is required for custody_mode operator_wallet/dual_mode"
        );

        let mut dual_mode = sample_user_config(connected_wallet);
        dual_mode.custody_mode = "dual_mode".to_string();
        dual_mode.operator_wallet_address = None;
        let dual_mode_err =
            validate_user_config(&dual_mode).expect_err("dual_mode must require operator wallet");
        assert_eq!(
            dual_mode_err,
            "operator_wallet_address is required for custody_mode operator_wallet/dual_mode"
        );
    }

    #[test]
    fn suggest_config_normalizes_missing_operator_wallet_in_operator_or_dual_modes() {
        let tmp = tempdir().expect("tempdir");
        let service = FrontdoorService::new_for_tests(
            FrontdoorConfig {
                require_privy: false,
                privy_app_id: None,
                privy_client_id: None,
                provision_command: None,
                default_instance_url: None,
                allow_default_instance_fallback: false,
                verify_app_base_url: None,
                session_ttl_secs: 900,
                poll_interval_ms: 1000,
            },
            tmp.path().join("wallet_sessions.json"),
        );

        let connected_wallet = "0x9431Cf5DA0CE60664661341db650763B08286B18";
        for intent in ["operator wallet", "dual mode"] {
            let mut base = sample_user_config(connected_wallet);
            base.operator_wallet_address = None;

            let suggested = service
                .suggest_config(crate::channels::web::types::FrontdoorSuggestConfigRequest {
                    wallet_address: connected_wallet.to_string(),
                    intent: intent.to_string(),
                    domain: Some("hyperliquid".to_string()),
                    gateway_auth_key: None,
                    base_config: Some(base),
                })
                .expect("suggest config should normalize missing operator wallet");

            assert_eq!(suggested.config.custody_mode, "user_wallet");
            assert!(
                suggested
                    .assumptions
                    .iter()
                    .any(|assumption| assumption
                        == "Operator wallet missing; normalized custody_mode to user_wallet for safe launch.")
            );
            let normalized_wallet = normalize_wallet_address(
                suggested
                    .config
                    .user_wallet_address
                    .as_deref()
                    .expect("suggested config must include user wallet"),
            )
            .expect("suggested wallet must be valid");
            assert_eq!(
                normalized_wallet,
                "0x9431cf5da0ce60664661341db650763b08286b18"
            );
        }
    }

    #[test]
    fn suggest_config_normalizes_hyperliquid_fields_for_non_hyperliquid_domains() {
        let tmp = tempdir().expect("tempdir");
        let service = FrontdoorService::new_for_tests(
            FrontdoorConfig {
                require_privy: false,
                privy_app_id: None,
                privy_client_id: None,
                provision_command: None,
                default_instance_url: None,
                allow_default_instance_fallback: false,
                verify_app_base_url: None,
                session_ttl_secs: 900,
                poll_interval_ms: 1000,
            },
            tmp.path().join("wallet_sessions.json"),
        );

        let connected_wallet = "0x9431Cf5DA0CE60664661341db650763B08286B18";
        let mut base = sample_user_config(connected_wallet);
        base.hyperliquid_network = "mainnet".to_string();
        base.paper_live_policy = "live_allowed".to_string();
        base.symbol_allowlist = vec!["BTC".to_string()];
        base.symbol_denylist = vec!["ETH".to_string()];

        let suggested = service
            .suggest_config(crate::channels::web::types::FrontdoorSuggestConfigRequest {
                wallet_address: connected_wallet.to_string(),
                intent: "mainnet live trading with btc-only".to_string(),
                domain: Some("general".to_string()),
                gateway_auth_key: None,
                base_config: Some(base),
            })
            .expect("suggest config");

        assert!(suggested.validated);
        assert_eq!(suggested.config.profile_domain, "general");
        assert_eq!(suggested.config.hyperliquid_network, "testnet");
        assert_eq!(suggested.config.paper_live_policy, "paper_only");
        assert!(suggested.config.symbol_allowlist.is_empty());
        assert!(suggested.config.symbol_denylist.is_empty());
    }

    #[test]
    fn suggest_config_keeps_addon_domains_explicit() {
        let tmp = tempdir().expect("tempdir");
        let service = FrontdoorService::new_for_tests(
            FrontdoorConfig {
                require_privy: false,
                privy_app_id: None,
                privy_client_id: None,
                provision_command: None,
                default_instance_url: None,
                allow_default_instance_fallback: false,
                verify_app_base_url: None,
                session_ttl_secs: 900,
                poll_interval_ms: 1000,
            },
            tmp.path().join("wallet_sessions.json"),
        );

        let connected_wallet = "0x9431Cf5DA0CE60664661341db650763B08286B18";
        let general_suggested = service
            .suggest_config(crate::channels::web::types::FrontdoorSuggestConfigRequest {
                wallet_address: connected_wallet.to_string(),
                intent: "hyperliquid eigenda mainnet workflow".to_string(),
                domain: None,
                gateway_auth_key: None,
                base_config: None,
            })
            .expect("general suggest config");
        assert_eq!(general_suggested.config.profile_domain, "general");

        let eigenda_suggested = service
            .suggest_config(crate::channels::web::types::FrontdoorSuggestConfigRequest {
                wallet_address: connected_wallet.to_string(),
                intent: "receipt continuity".to_string(),
                domain: Some("eigenda".to_string()),
                gateway_auth_key: None,
                base_config: None,
            })
            .expect("eigenda suggest config");
        assert_eq!(eigenda_suggested.config.profile_domain, "eigenda");
    }

    #[test]
    fn domain_profiles_keep_addon_modules_explicit() {
        let profiles = frontdoor_domain_profiles();
        let general = profiles
            .iter()
            .find(|profile| profile.domain == "general")
            .expect("general profile");
        let hyperliquid = profiles
            .iter()
            .find(|profile| profile.domain == "hyperliquid")
            .expect("hyperliquid profile");
        let eigenda = profiles
            .iter()
            .find(|profile| profile.domain == "eigenda")
            .expect("eigenda profile");

        assert!(
            !general
                .default_modules
                .iter()
                .any(|module| module == "hyperliquid_addon" || module == "eigenda_addon")
        );
        assert!(
            hyperliquid
                .default_modules
                .iter()
                .any(|module| module == "hyperliquid_addon")
        );
        assert!(
            !hyperliquid
                .default_modules
                .iter()
                .any(|module| module == "eigenda_addon")
        );
        assert!(
            eigenda
                .default_modules
                .iter()
                .any(|module| module == "eigenda_addon")
        );
        assert!(
            !eigenda
                .default_modules
                .iter()
                .any(|module| module == "hyperliquid_addon")
        );
    }

    #[test]
    fn config_contract_reports_supported_versions() {
        let tmp = tempdir().expect("tempdir");
        let service = FrontdoorService::new_for_tests(
            FrontdoorConfig {
                require_privy: false,
                privy_app_id: None,
                privy_client_id: None,
                provision_command: None,
                default_instance_url: None,
                allow_default_instance_fallback: false,
                verify_app_base_url: None,
                session_ttl_secs: 900,
                poll_interval_ms: 1000,
            },
            tmp.path().join("wallet_sessions.json"),
        );
        let contract = service.config_contract();
        assert_eq!(
            contract.current_config_version,
            FRONTDOOR_CURRENT_CONFIG_VERSION
        );
        assert!(
            contract
                .supported_config_versions
                .contains(&FRONTDOOR_CURRENT_CONFIG_VERSION)
        );
        assert!(
            contract
                .mandatory_steps
                .contains(&"configure_runtime_profile_and_risk".to_string())
        );
    }

    #[test]
    fn policy_template_library_exposes_common_objective_presets() {
        let tmp = tempdir().expect("tempdir");
        let service = FrontdoorService::new_for_tests(
            FrontdoorConfig {
                require_privy: false,
                privy_app_id: None,
                privy_client_id: None,
                provision_command: None,
                default_instance_url: None,
                allow_default_instance_fallback: false,
                verify_app_base_url: None,
                session_ttl_secs: 900,
                poll_interval_ms: 1000,
            },
            tmp.path().join("wallet_sessions.json"),
        );

        let library = service.policy_template_library();
        assert!(!library.templates.is_empty());
        assert!(
            library
                .templates
                .iter()
                .any(|template| template.template_id == "general_safe_baseline")
        );
        assert!(
            library
                .templates
                .iter()
                .any(|template| template.template_id == "hyperliquid_paper_operator")
        );
    }

    #[test]
    fn list_sessions_filters_by_wallet() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let tmp = tempdir().expect("tempdir");
            let service = FrontdoorService::new_for_tests(
                FrontdoorConfig {
                    require_privy: false,
                    privy_app_id: None,
                    privy_client_id: None,
                    provision_command: None,
                    default_instance_url: None,
                    allow_default_instance_fallback: false,
                    verify_app_base_url: None,
                    session_ttl_secs: 900,
                    poll_interval_ms: 1000,
                },
                tmp.path().join("wallet_sessions.json"),
            );

            let wallet_a = "0x9431Cf5DA0CE60664661341db650763B08286B18".to_string();
            let wallet_b = "0x8ba1f109551bD432803012645Ac136ddd64DBA72".to_string();

            service
                .create_challenge(FrontdoorChallengeRequest {
                    wallet_address: wallet_a.clone(),
                    privy_user_id: None,
                    chain_id: Some(1),
                })
                .await
                .expect("challenge a");
            service
                .create_challenge(FrontdoorChallengeRequest {
                    wallet_address: wallet_b.clone(),
                    privy_user_id: None,
                    chain_id: Some(1),
                })
                .await
                .expect("challenge b");

            let (total, sessions) = service
                .list_sessions(Some(&wallet_a), 10)
                .await
                .expect("list sessions");
            assert_eq!(total, 1);
            assert_eq!(sessions.len(), 1);
            assert_eq!(
                sessions[0].wallet_address,
                "0x9431cf5da0ce60664661341db650763b08286b18"
            );
            assert_eq!(sessions[0].provisioning_source, "unknown");
        });
    }

    #[test]
    fn experience_manifest_includes_state_inputs() {
        let tmp = tempdir().expect("tempdir");
        let service = FrontdoorService::new_for_tests(
            FrontdoorConfig {
                require_privy: false,
                privy_app_id: None,
                privy_client_id: None,
                provision_command: None,
                default_instance_url: None,
                allow_default_instance_fallback: false,
                verify_app_base_url: None,
                session_ttl_secs: 900,
                poll_interval_ms: 1000,
            },
            tmp.path().join("wallet_sessions.json"),
        );
        let manifest = service.experience_manifest();
        assert_eq!(manifest.manifest_version, 1);
        assert!(manifest.steps.len() >= 4);
        assert!(
            manifest
                .steps
                .iter()
                .all(|step| !step.state_inputs.is_empty())
        );
        assert!(
            manifest
                .steps
                .iter()
                .any(|step| step.step_id == "verification"
                    && step.state_inputs.iter().any(|field| field == "signature"))
        );
    }

    #[test]
    fn onboarding_timeline_runtime_todos_and_preflight_are_deterministic() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let tmp = tempdir().expect("tempdir");
            let service = FrontdoorService::new_for_tests(
                FrontdoorConfig {
                    require_privy: false,
                    privy_app_id: None,
                    privy_client_id: None,
                    provision_command: None,
                    default_instance_url: Some("https://session.example/gateway".to_string()),
                    allow_default_instance_fallback: true,
                    verify_app_base_url: Some(
                        "https://verify-sepolia.eigencloud.xyz/app".to_string(),
                    ),
                    session_ttl_secs: 900,
                    poll_interval_ms: 100,
                },
                tmp.path().join("wallet_sessions.json"),
            );

            let private_key = decode_hex_prefixed(
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
            )
            .expect("private key");
            let signing_key = SigningKey::from_slice(&private_key).expect("signing key");
            let wallet =
                ethereum_address_from_verifying_key(signing_key.verifying_key()).expect("wallet");

            let challenge = service
                .create_challenge(FrontdoorChallengeRequest {
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    chain_id: Some(1),
                })
                .await
                .expect("challenge");
            let session_uuid = Uuid::parse_str(&challenge.session_id).expect("session uuid");

            let onboarding = service
                .onboarding_state(session_uuid)
                .await
                .expect("onboarding");
            assert_eq!(onboarding.current_step, "capture_objective");
            assert_eq!(onboarding.transcript.len(), 1);

            let empty_chat = service
                .onboarding_chat(FrontdoorOnboardingChatRequest {
                    session_id: challenge.session_id.clone(),
                    message: "   ".to_string(),
                })
                .await;
            assert!(empty_chat.is_err());

            let chat = service
                .onboarding_chat(FrontdoorOnboardingChatRequest {
                    session_id: challenge.session_id.clone(),
                    message: "Launch conservative strategy profile".to_string(),
                })
                .await
                .expect("chat");
            assert_eq!(chat.state.current_step, "propose_plan");
            assert!(!chat.state.completed);

            let confirm_plan = service
                .onboarding_chat(FrontdoorOnboardingChatRequest {
                    session_id: challenge.session_id.clone(),
                    message: "confirm plan".to_string(),
                })
                .await
                .expect("confirm plan");
            assert_eq!(confirm_plan.state.current_step, "collect_required_variables");
            assert!(!confirm_plan.state.missing_fields.is_empty());

            let fill_required = service
                .onboarding_chat(FrontdoorOnboardingChatRequest {
                    session_id: challenge.session_id.clone(),
                    message: "profile_name=demo_profile, gateway_auth_key=__from_config__, accept_terms=true".to_string(),
                })
                .await
                .expect("fill required");
            assert_eq!(fill_required.state.current_step, "confirm_and_sign");
            assert!(fill_required.state.missing_fields.is_empty());

            let confirm_sign = service
                .onboarding_chat(FrontdoorOnboardingChatRequest {
                    session_id: challenge.session_id.clone(),
                    message: "confirm sign".to_string(),
                })
                .await
                .expect("confirm sign");
            assert_eq!(confirm_sign.state.current_step, "ready_to_sign");
            assert!(confirm_sign.state.completed);

            let timeline = service
                .session_timeline(session_uuid)
                .await
                .expect("timeline");
            assert!(timeline.events.len() >= 3);
            let mut prev = 0u64;
            for event in &timeline.events {
                assert!(event.seq_id > prev);
                prev = event.seq_id;
            }

            let prehash = eip191_personal_sign_hash(&challenge.message);
            let (sig, recid) = signing_key
                .sign_prehash_recoverable(&prehash)
                .expect("sign challenge");
            let mut sig_bytes = sig.to_bytes().to_vec();
            sig_bytes.push(recid.to_byte() + 27);
            let signature = format!("0x{}", encode_hex_lower(&sig_bytes));

            service
                .clone()
                .verify_and_start(FrontdoorVerifyRequest {
                    session_id: challenge.session_id.clone(),
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    privy_identity_token: None,
                    privy_access_token: None,
                    message: challenge.message.clone(),
                    signature,
                    config: sample_user_config(&wallet),
                })
                .await
                .expect("verify and start");

            let verification = service
                .verification_explanation(session_uuid)
                .await
                .expect("verification explanation");
            assert_eq!(verification.backend, "eigencloud_primary");
            assert_eq!(verification.level, "primary_plus_signed_fallback");

            let preflight = service
                .funding_preflight(session_uuid)
                .await
                .expect("funding preflight");
            assert_eq!(preflight.status, "passed");
            assert!(
                preflight
                    .checks
                    .iter()
                    .all(|check| !check.check_id.is_empty() && !check.status.is_empty())
            );

            let invalid_runtime = service
                .runtime_control(
                    session_uuid,
                    FrontdoorRuntimeControlRequest {
                        action: "invalid".to_string(),
                        actor: None,
                    },
                )
                .await;
            assert!(invalid_runtime.is_err());

            let runtime = service
                .runtime_control(
                    session_uuid,
                    FrontdoorRuntimeControlRequest {
                        action: "pause".to_string(),
                        actor: Some("operator".to_string()),
                    },
                )
                .await
                .expect("runtime control");
            assert!(runtime.status == "applied" || runtime.status == "noop");
            assert_eq!(runtime.runtime_state, "paused");

            let todos = service
                .gateway_todos_for_session(session_uuid)
                .await
                .expect("gateway todos");
            assert!(!todos.todos.is_empty());
            assert!(
                todos
                    .todos
                    .iter()
                    .all(|todo| !todo.todo_id.is_empty() && !todo.owner.is_empty())
            );

            let session_payload = service.get_session(session_uuid).await.expect("session");
            let json = serde_json::to_value(session_payload).expect("session json");
            let obj = json.as_object().expect("object");
            assert!(obj.contains_key("launched_on_eigencloud"));
            assert!(obj.contains_key("verification_backend"));
            assert!(obj.contains_key("verification_level"));
            assert!(obj.contains_key("verification_fallback_enabled"));
            assert!(obj.contains_key("verification_fallback_require_signed_receipts"));
        });
    }

    #[test]
    fn operator_vs_public_monitor_payloads_are_separated() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let tmp = tempdir().expect("tempdir");
            let service = FrontdoorService::new_for_tests(
                FrontdoorConfig {
                    require_privy: false,
                    privy_app_id: None,
                    privy_client_id: None,
                    provision_command: None,
                    default_instance_url: None,
                    allow_default_instance_fallback: false,
                    verify_app_base_url: None,
                    session_ttl_secs: 900,
                    poll_interval_ms: 1000,
                },
                tmp.path().join("wallet_sessions.json"),
            );

            let wallet = "0x9431Cf5DA0CE60664661341db650763B08286B18".to_string();
            let challenge = service
                .create_challenge(FrontdoorChallengeRequest {
                    wallet_address: wallet.clone(),
                    privy_user_id: None,
                    chain_id: Some(1),
                })
                .await
                .expect("challenge");

            let (public_total, public_sessions) = service
                .list_sessions(Some(&wallet), 10)
                .await
                .expect("public sessions");
            assert_eq!(public_total, 1);
            assert_eq!(public_sessions.len(), 1);
            assert_eq!(public_sessions[0].session_ref, "v1");
            assert_eq!(public_sessions[0].verification_backend, "unknown");
            assert_eq!(public_sessions[0].verification_level, "unknown");
            assert!(!public_sessions[0].verification_fallback_enabled);
            assert!(!public_sessions[0].verification_fallback_require_signed_receipts);
            assert!(!public_sessions[0].launched_on_eigencloud);

            let public_json = serde_json::to_value(&public_sessions[0]).expect("public json");
            let public_obj = public_json.as_object().expect("public object");
            assert!(!public_obj.contains_key("session_id"));
            assert!(!public_obj.contains_key("instance_url"));
            assert!(!public_obj.contains_key("app_url"));
            assert!(!public_obj.contains_key("verify_url"));
            assert!(!public_obj.contains_key("eigen_app_id"));
            assert!(!public_obj.contains_key("error"));

            let (operator_total, operator_sessions) = service
                .list_sessions_full(Some(&wallet), 10)
                .await
                .expect("operator sessions");
            assert_eq!(operator_total, 1);
            assert_eq!(operator_sessions.len(), 1);
            assert_eq!(operator_sessions[0].session_id, challenge.session_id);
            assert_eq!(operator_sessions[0].verification_backend, "unknown");
            assert_eq!(operator_sessions[0].verification_level, "unknown");

            let operator_json = serde_json::to_value(&operator_sessions[0]).expect("operator json");
            let operator_obj = operator_json.as_object().expect("operator object");
            assert!(operator_obj.contains_key("session_id"));
            assert!(operator_obj.contains_key("launched_on_eigencloud"));
            assert!(operator_obj.contains_key("verification_backend"));
            assert!(operator_obj.contains_key("verification_level"));
            assert!(operator_obj.contains_key("verification_fallback_enabled"));
            assert!(operator_obj.contains_key("verification_fallback_require_signed_receipts"));
        });
    }

    #[test]
    fn onboarding_gateway_auth_marker_counts_as_resolved_requirement() {
        let mut captured = HashMap::new();
        captured.insert("profile_name".to_string(), "demo_profile".to_string());
        captured.insert(
            "gateway_auth_key".to_string(),
            "__from_config__".to_string(),
        );
        captured.insert("accept_terms".to_string(), "true".to_string());

        let payload = build_onboarding_step3_payload(&captured);
        assert_eq!(payload.unresolved_required_count, 0);
        let gateway = payload
            .required_variables
            .iter()
            .find(|item| item.field == "gateway_auth_key")
            .expect("gateway_auth_key required variable");
        assert_eq!(gateway.status, "resolved");
    }

    fn sample_user_config(wallet: &str) -> FrontdoorUserConfig {
        FrontdoorUserConfig {
            config_version: FRONTDOOR_CURRENT_CONFIG_VERSION,
            profile_domain: "hyperliquid".to_string(),
            domain_overrides: HashMap::new(),
            inference_summary: None,
            inference_confidence: None,
            inference_warnings: Vec::new(),
            profile_name: "demo_profile".to_string(),
            hyperliquid_network: "testnet".to_string(),
            paper_live_policy: "paper_only".to_string(),
            hyperliquid_api_base_url: None,
            hyperliquid_ws_url: None,
            request_timeout_ms: 10_000,
            max_retries: 3,
            retry_backoff_ms: 500,
            max_position_size_usd: 1_000,
            leverage_cap: 2,
            max_allocation_usd: 1_000,
            per_trade_notional_cap_usd: 100,
            max_leverage: 2,
            max_slippage_bps: 50,
            symbol_allowlist: vec!["BTC".to_string(), "ETH".to_string()],
            symbol_denylist: vec![],
            custody_mode: "user_wallet".to_string(),
            operator_wallet_address: None,
            user_wallet_address: Some(wallet.to_string()),
            vault_address: None,
            information_sharing_scope: "signals_only".to_string(),
            kill_switch_enabled: true,
            kill_switch_behavior: "pause_agent".to_string(),
            enable_memory: true,
            gateway_auth_key: "supersecuregatewaykey01".to_string(),
            eigencloud_auth_key: None,
            verification_backend: "eigencloud_primary".to_string(),
            verification_eigencloud_endpoint: None,
            verification_eigencloud_auth_scheme: "bearer".to_string(),
            verification_eigencloud_timeout_ms: 5_000,
            verification_fallback_enabled: true,
            verification_fallback_signing_key_id: None,
            verification_fallback_chain_path: None,
            verification_fallback_require_signed_receipts: true,
            accept_terms: true,
        }
    }
}
