//! Frontdoor orchestration for wallet-authenticated enclave provisioning.
//!
//! This service powers the user-facing "frontdoor" flow:
//! 1) create signing challenge
//! 2) verify signed intent payload
//! 3) provision per-user enclave through a configurable command
//! 4) return redirect URL for the dedicated instance

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use chrono::{DateTime, Utc};
use k256::ecdsa::{RecoveryId, Signature as EcdsaSignature, VerifyingKey};
use rand::{Rng, distributions::Alphanumeric};
use sha3::{Digest, Keccak256};
use tokio::process::Command;
use tokio::sync::RwLock;
use url::Url;
use uuid::Uuid;

use crate::channels::web::types::{
    FrontdoorBootstrapResponse, FrontdoorChallengeRequest, FrontdoorChallengeResponse,
    FrontdoorConfigContractResponse, FrontdoorConfigDefaults, FrontdoorConfigEnums,
    FrontdoorDomainProfile, FrontdoorSessionResponse, FrontdoorSessionSummaryResponse,
    FrontdoorSuggestConfigRequest, FrontdoorSuggestConfigResponse, FrontdoorUserConfig,
    FrontdoorVerifyRequest, FrontdoorVerifyResponse,
};

#[derive(Debug, Clone)]
pub struct FrontdoorConfig {
    pub require_privy: bool,
    pub privy_app_id: Option<String>,
    pub privy_client_id: Option<String>,
    pub provision_command: Option<String>,
    pub default_instance_url: Option<String>,
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
    instance_url: Option<String>,
    verify_url: Option<String>,
    eigen_app_id: Option<String>,
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
        let command_configured =
            is_non_empty_config_value(self.config.provision_command.as_deref());
        let default_url_configured =
            is_non_empty_config_value(self.config.default_instance_url.as_deref());
        let provisioning_backend = if command_configured {
            ProvisioningSource::Command.as_str().to_string()
        } else if default_url_configured {
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
            supported_domains: vec![
                "general".to_string(),
                "developer".to_string(),
                "creative".to_string(),
                "research".to_string(),
                "business_ops".to_string(),
                "communications".to_string(),
                "hyperliquid".to_string(),
                "eigenda".to_string(),
            ],
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

        let session = ProvisioningSession {
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
            instance_url: None,
            verify_url: None,
            eigen_app_id: None,
            error: None,
            created_at: now,
            updated_at: now,
            expires_at,
        };
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
            verify_wallet_signature(&req.message, &req.signature, &wallet)?;

            session.config = Some(req.config.clone());
            session.privy_identity_token = req.privy_identity_token.clone();
            session.privy_access_token = req.privy_access_token.clone();
            session.status = SessionStatus::Provisioning;
            session.updated_at = Utc::now();
            session.error = None;
            session.detail = "Provisioning dedicated enclave...".to_string();
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

    async fn run_provision(self: Arc<Self>, session_id: Uuid) {
        let (
            wallet,
            privy_user,
            privy_identity_token,
            privy_access_token,
            chain_id,
            version,
            cfg,
            command,
            default_url,
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
                self.config.provision_command.clone(),
                self.config.default_instance_url.clone(),
                self.config.verify_app_base_url.clone(),
            )
        };

        let (result, provisioning_source) = if let Some(cmd_template) = command {
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
            (
                execute_provision_command(&cmd_template, &command_input).await,
                ProvisioningSource::Command,
            )
        } else if let Some(url) = default_url {
            (
                Ok(ProvisioningResult {
                    instance_url: url.clone(),
                    verify_url: if looks_like_verify_url(&url) {
                        Some(url)
                    } else {
                        None
                    },
                    eigen_app_id: None,
                }),
                ProvisioningSource::DefaultInstanceUrl,
            )
        } else {
            (
                Err(
                    "No provisioning backend configured (set GATEWAY_FRONTDOOR_PROVISION_COMMAND or GATEWAY_FRONTDOOR_DEFAULT_INSTANCE_URL)"
                        .to_string(),
                ),
                ProvisioningSource::Unconfigured,
            )
        };

        let mut state = self.state.write().await;
        let mut wallet_record: Option<WalletSessionRecord> = None;
        {
            let Some(session) = state.sessions.get_mut(&session_id) else {
                return;
            };
            session.provisioning_source = provisioning_source;

            match result {
                Ok(provisioned) => {
                    session.status = SessionStatus::Ready;
                    session.instance_url = Some(provisioned.instance_url.clone());
                    session.verify_url = provisioned.verify_url.clone();
                    session.eigen_app_id = provisioned.eigen_app_id.clone();
                    session.error = None;
                    session.detail = "Enclave is live. Redirect ready.".to_string();
                    session.updated_at = Utc::now();

                    tracing::info!(
                        session_id = %session.id,
                        wallet = %session.wallet_address,
                        provisioning_source = provisioning_source.as_str(),
                        verification_level = %verification_assurance_level(session.config.as_ref()),
                        eigen_app_id = ?session.eigen_app_id,
                        verify_url = ?session.verify_url,
                        launched_on_eigencloud = ?session_launched_on_eigencloud(session),
                        "Frontdoor provisioning completed"
                    );

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

                    tracing::warn!(
                        session_id = %session.id,
                        wallet = %session.wallet_address,
                        provisioning_source = provisioning_source.as_str(),
                        verification_level = %verification_assurance_level(session.config.as_ref()),
                        error = ?session.error,
                        "Frontdoor provisioning failed"
                    );
                }
            }
        }

        if let Some(record) = wallet_record {
            state.wallets.insert(wallet.clone(), record);
            let store = WalletSessionStore {
                wallets: state.wallets.clone(),
            };
            if let Err(err) = persist_wallet_store(&self.store_path, &store) {
                tracing::warn!("Failed to persist frontdoor wallet session store: {err}");
            }
        }
    }
}

fn default_wallet_store_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".enclagent")
        .join("frontdoor")
        .join("wallet_sessions.json")
}

#[derive(Debug, Clone)]
struct ProvisioningResult {
    instance_url: String,
    verify_url: Option<String>,
    eigen_app_id: Option<String>,
}

fn is_non_empty_config_value(value: Option<&str>) -> bool {
    value.map(|v| !v.trim().is_empty()).unwrap_or(false)
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

fn render_session_response(session: &ProvisioningSession) -> FrontdoorSessionResponse {
    let config = session.config.as_ref();
    FrontdoorSessionResponse {
        session_id: session.id.to_string(),
        wallet_address: session.wallet_address.clone(),
        privy_user_id: session.privy_user_id.clone(),
        version: session.version,
        status: session.status.as_str().to_string(),
        detail: session.detail.clone(),
        provisioning_source: session.provisioning_source.as_str().to_string(),
        dedicated_instance: session.provisioning_source.dedicated_instance(),
        launched_on_eigencloud: session_launched_on_eigencloud(session),
        verification_backend: config.map(|c| c.verification_backend.clone()),
        verification_level: config.map(|_| verification_assurance_level(config)),
        verification_fallback_enabled: config.map(|c| c.verification_fallback_enabled),
        verification_fallback_require_signed_receipts: config
            .map(|c| c.verification_fallback_require_signed_receipts),
        instance_url: session.instance_url.clone(),
        verify_url: session.verify_url.clone(),
        eigen_app_id: session.eigen_app_id.clone(),
        error: session.error.clone(),
        created_at: session.created_at.to_rfc3339(),
        updated_at: session.updated_at.to_rfc3339(),
        expires_at: session.expires_at.to_rfc3339(),
        profile_name: config.map(|c| c.profile_name.clone()),
    }
}

fn render_session_summary(session: &ProvisioningSession) -> FrontdoorSessionSummaryResponse {
    let config = session.config.as_ref();
    FrontdoorSessionSummaryResponse {
        session_ref: session.id.to_string().chars().take(8).collect(),
        wallet_address: session.wallet_address.clone(),
        version: session.version,
        status: session.status.as_str().to_string(),
        detail: session.detail.clone(),
        provisioning_source: session.provisioning_source.as_str().to_string(),
        dedicated_instance: session.provisioning_source.dedicated_instance(),
        launched_on_eigencloud: session_launched_on_eigencloud(session),
        verification_backend: config.map(|c| c.verification_backend.clone()),
        verification_level: config.map(|_| verification_assurance_level(config)),
        verification_fallback_enabled: config.map(|c| c.verification_fallback_enabled),
        verification_fallback_require_signed_receipts: config
            .map(|c| c.verification_fallback_require_signed_receipts),
        created_at: session.created_at.to_rfc3339(),
        updated_at: session.updated_at.to_rfc3339(),
        expires_at: session.expires_at.to_rfc3339(),
        profile_name: config.map(|c| c.profile_name.clone()),
    }
}

fn session_launched_on_eigencloud(session: &ProvisioningSession) -> Option<bool> {
    if session.instance_url.is_none()
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
    let from_instance_url = session
        .instance_url
        .as_deref()
        .map(looks_like_eigencloud_url)
        .unwrap_or(false);

    Some(from_app_id || from_verify_url || from_instance_url)
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
            .or_else(|| v.get("url"))
            .or_else(|| v.get("gateway_url"))
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
        if instance_url.is_none() {
            instance_url = verify_url.clone();
        }
        if let Some(instance_url) = instance_url {
            return Some(ProvisioningResult {
                instance_url,
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

fn looks_like_verify_url(candidate: &str) -> bool {
    if let Ok(url) = Url::parse(candidate)
        && let Some(host) = url.host_str()
    {
        let host = host.to_ascii_lowercase();
        return host == "verify-sepolia.eigencloud.xyz" || host == "verify.eigencloud.xyz";
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

async fn execute_provision_command(
    template: &str,
    input: &ProvisionCommandInput<'_>,
) -> Result<ProvisioningResult, String> {
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
        cmd = cmd.replace(placeholder, &format!("${{{env_key}}}"));
    }

    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(cmd);
    for (_, env_key, value) in &replacements {
        command.env(env_key, value);
    }

    let output = command
        .output()
        .await
        .map_err(|e| format!("failed to execute provision command: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "provision command failed with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(result) = execute_provision_output(&stdout, input.verify_base_url) else {
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
        "sign_gasless_authorization_transaction".to_string(),
        "configure_runtime_profile_and_risk".to_string(),
        "set_gateway_auth_key".to_string(),
        "accept_risk_and_terms".to_string(),
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

    if config.profile_name.trim().is_empty() {
        config.profile_name = "launchpad_profile".to_string();
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
        FrontdoorChallengeRequest, FrontdoorUserConfig, FrontdoorVerifyRequest,
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
            assert_eq!(ready.launched_on_eigencloud, Some(false));
            assert_eq!(
                ready.verification_backend.as_deref(),
                Some("eigencloud_primary")
            );
            assert_eq!(
                ready.verification_level.as_deref(),
                Some("primary_plus_signed_fallback")
            );
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

        let plain = "line1\nhttps://foo.example/path\n";
        let result = execute_provision_output(plain, None).expect("plain url");
        assert_eq!(result.instance_url, "https://foo.example/path");
        assert!(result.verify_url.is_none());
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
