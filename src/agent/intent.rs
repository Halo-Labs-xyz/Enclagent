//! Typed runtime artifacts for the intent -> execution -> verification pipeline.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Runtime contract version for intent/execution/verification artifacts.
pub const INTENT_ARTIFACT_CONTRACT_VERSION: &str = "v1";

/// Hash algorithm used for deterministic artifact digests.
pub const INTENT_ARTIFACT_HASH_ALGORITHM: &str = "blake3";

/// Structured strategy payload attached to an intent.
pub type StrategyPayload = BTreeMap<String, serde_json::Value>;

/// Structured risk-limit payload attached to an intent.
pub type RiskLimitsPayload = BTreeMap<String, serde_json::Value>;

/// Validation failures for intent pipeline artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactValidationError {
    EmptyField(&'static str),
    NilIdentifier(&'static str),
    NonPositiveNumber(&'static str),
    InvalidHash(&'static str),
    InvalidValue(&'static str),
}

impl std::fmt::Display for ArtifactValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyField(name) => write!(f, "{} cannot be empty", name),
            Self::NilIdentifier(name) => write!(f, "{} cannot be nil UUID", name),
            Self::NonPositiveNumber(name) => write!(f, "{} must be greater than zero", name),
            Self::InvalidHash(name) => write!(f, "{} must be a 64-char lowercase hex hash", name),
            Self::InvalidValue(name) => write!(f, "{} has an invalid value", name),
        }
    }
}

impl std::error::Error for ArtifactValidationError {}

fn validate_hash(field: &'static str, value: &str) -> Result<(), ArtifactValidationError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(ArtifactValidationError::InvalidHash(field));
    }
    Ok(())
}

/// Information-sharing scope for copytrading initialization.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InformationSharingScope {
    None,
    SignalsOnly,
    SignalsAndExecution,
    FullAudit,
}

/// Fixed initialization profile for WS-10 copytrading sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopyTradingInitializationProfile {
    pub max_allocation_usd: Decimal,
    pub per_trade_notional_cap_usd: Decimal,
    pub max_leverage: Decimal,
    #[serde(default)]
    pub symbol_allowlist: Vec<String>,
    #[serde(default)]
    pub symbol_denylist: Vec<String>,
    pub max_slippage_bps: u32,
    pub information_sharing_scope: InformationSharingScope,
}

impl CopyTradingInitializationProfile {
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.max_allocation_usd <= Decimal::ZERO {
            return Err(ArtifactValidationError::NonPositiveNumber(
                "max_allocation_usd",
            ));
        }
        if self.per_trade_notional_cap_usd <= Decimal::ZERO {
            return Err(ArtifactValidationError::NonPositiveNumber(
                "per_trade_notional_cap_usd",
            ));
        }
        if self.max_leverage <= Decimal::ZERO {
            return Err(ArtifactValidationError::NonPositiveNumber("max_leverage"));
        }
        if self.max_slippage_bps == 0 {
            return Err(ArtifactValidationError::InvalidValue("max_slippage_bps"));
        }
        if self
            .symbol_allowlist
            .iter()
            .any(|symbol| symbol.trim().is_empty())
        {
            return Err(ArtifactValidationError::EmptyField("symbol_allowlist"));
        }
        if self
            .symbol_denylist
            .iter()
            .any(|symbol| symbol.trim().is_empty())
        {
            return Err(ArtifactValidationError::EmptyField("symbol_denylist"));
        }
        Ok(())
    }
}

/// Identity of a copytrading signal provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalProviderIdentity {
    pub provider_id: String,
    pub payout_address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

impl SignalProviderIdentity {
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.provider_id.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("provider_id"));
        }
        if self.payout_address.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("payout_address"));
        }
        Ok(())
    }
}

/// Fee schedule for a signal provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalFeeSchedule {
    pub fixed_fee_bps: u32,
    pub performance_fee_bps: u32,
    pub max_fee_usd: Decimal,
}

impl SignalFeeSchedule {
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.max_fee_usd <= Decimal::ZERO {
            return Err(ArtifactValidationError::NonPositiveNumber("max_fee_usd"));
        }
        Ok(())
    }
}

/// Attribution record linking a mirrored action to a provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderRevenueAttribution {
    pub provider: SignalProviderIdentity,
    pub signal_id: String,
    pub signal_hash: String,
    pub attribution_weight_bps: u32,
    pub fee_schedule: SignalFeeSchedule,
}

impl ProviderRevenueAttribution {
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.provider.validate()?;
        self.fee_schedule.validate()?;
        if self.signal_id.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("signal_id"));
        }
        validate_hash("signal_hash", &self.signal_hash)?;
        if self.attribution_weight_bps == 0 {
            return Err(ArtifactValidationError::InvalidValue(
                "attribution_weight_bps",
            ));
        }
        Ok(())
    }
}

/// Per-provider split in a settlement receipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderSettlementSplit {
    pub provider_id: String,
    pub gross_fee_usd: Decimal,
    pub pnl_share_usd: Decimal,
}

impl ProviderSettlementSplit {
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.provider_id.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("provider_id"));
        }
        if self.gross_fee_usd < Decimal::ZERO {
            return Err(ArtifactValidationError::InvalidValue("gross_fee_usd"));
        }
        Ok(())
    }
}

/// Revenue-share settlement receipt linked to a mirrored execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RevenueShareSettlementReceipt {
    pub settlement_id: Uuid,
    pub intent_id: Uuid,
    pub receipt_id: Uuid,
    #[serde(default)]
    pub provider_splits: Vec<ProviderSettlementSplit>,
    pub total_pnl_usd: Decimal,
    pub total_fee_usd: Decimal,
    pub settled_at: DateTime<Utc>,
}

impl RevenueShareSettlementReceipt {
    pub fn hash(&self) -> Result<String, serde_json::Error> {
        deterministic_hash(self)
    }

    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.settlement_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("settlement_id"));
        }
        if self.intent_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("intent_id"));
        }
        if self.receipt_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("receipt_id"));
        }
        if self.total_fee_usd < Decimal::ZERO {
            return Err(ArtifactValidationError::InvalidValue("total_fee_usd"));
        }
        for split in &self.provider_splits {
            split.validate()?;
        }
        Ok(())
    }
}

/// A2A contract for signal publication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct A2aSignalPublicationContract {
    pub signal_id: String,
    pub provider: SignalProviderIdentity,
    pub symbol: String,
    pub side: ExecutionSide,
    pub confidence_bps: u32,
    pub price_ref: Decimal,
    pub signal_hash: String,
    pub published_at: DateTime<Utc>,
}

impl A2aSignalPublicationContract {
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.signal_id.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("signal_id"));
        }
        self.provider.validate()?;
        if self.symbol.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("symbol"));
        }
        if self.confidence_bps == 0 {
            return Err(ArtifactValidationError::InvalidValue("confidence_bps"));
        }
        if self.price_ref <= Decimal::ZERO {
            return Err(ArtifactValidationError::NonPositiveNumber("price_ref"));
        }
        validate_hash("signal_hash", &self.signal_hash)?;
        Ok(())
    }
}

/// A2A contract for policy negotiation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct A2aPolicyNegotiationContract {
    pub negotiation_id: Uuid,
    pub requester_agent_id: String,
    pub source_agent_id: String,
    pub natural_language_policy: String,
    pub compiled_policy_hash: String,
    pub negotiated_profile: CopyTradingInitializationProfile,
    pub fee_schedule: SignalFeeSchedule,
    pub negotiated_at: DateTime<Utc>,
}

impl A2aPolicyNegotiationContract {
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.negotiation_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("negotiation_id"));
        }
        if self.requester_agent_id.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("requester_agent_id"));
        }
        if self.source_agent_id.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("source_agent_id"));
        }
        if self.natural_language_policy.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField(
                "natural_language_policy",
            ));
        }
        validate_hash("compiled_policy_hash", &self.compiled_policy_hash)?;
        self.negotiated_profile.validate()?;
        self.fee_schedule.validate()?;
        Ok(())
    }
}

/// A2A contract for execution intents produced by copytrading policy checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct A2aExecutionIntentContract {
    pub execution_intent_id: Uuid,
    pub intent_id: Uuid,
    pub source_signal_hash: String,
    pub policy_hash: String,
    pub profile_hash: String,
    pub wallet_attestation_hash: String,
    pub expected_notional: Decimal,
    pub expected_leverage: Decimal,
    pub created_at: DateTime<Utc>,
}

impl A2aExecutionIntentContract {
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.execution_intent_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier(
                "execution_intent_id",
            ));
        }
        if self.intent_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("intent_id"));
        }
        validate_hash("source_signal_hash", &self.source_signal_hash)?;
        validate_hash("policy_hash", &self.policy_hash)?;
        validate_hash("profile_hash", &self.profile_hash)?;
        validate_hash("wallet_attestation_hash", &self.wallet_attestation_hash)?;
        if self.expected_notional <= Decimal::ZERO {
            return Err(ArtifactValidationError::NonPositiveNumber(
                "expected_notional",
            ));
        }
        if self.expected_leverage <= Decimal::ZERO {
            return Err(ArtifactValidationError::NonPositiveNumber(
                "expected_leverage",
            ));
        }
        Ok(())
    }
}

/// WS-10 A2A message contracts for copytrading control plane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum A2aCopyTradingMessage {
    SignalPublication(A2aSignalPublicationContract),
    PolicyNegotiation(A2aPolicyNegotiationContract),
    ExecutionIntent(A2aExecutionIntentContract),
}

impl A2aCopyTradingMessage {
    pub fn hash(&self) -> Result<String, serde_json::Error> {
        deterministic_hash(self)
    }

    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        match self {
            Self::SignalPublication(contract) => contract.validate(),
            Self::PolicyNegotiation(contract) => contract.validate(),
            Self::ExecutionIntent(contract) => contract.validate(),
        }
    }
}

/// Canonical intent artifact emitted before execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntentEnvelope {
    pub intent_id: Uuid,
    pub agent_id: String,
    pub user_id: String,
    #[serde(default)]
    pub strategy: StrategyPayload,
    #[serde(default)]
    pub risk_limits: RiskLimitsPayload,
    pub market_context_hash: String,
    pub created_at: DateTime<Utc>,
}

impl IntentEnvelope {
    /// Construct a new intent envelope.
    pub fn new(
        agent_id: impl Into<String>,
        user_id: impl Into<String>,
        strategy: StrategyPayload,
        risk_limits: RiskLimitsPayload,
        market_context_hash: impl Into<String>,
    ) -> Self {
        Self {
            intent_id: Uuid::new_v4(),
            agent_id: agent_id.into(),
            user_id: user_id.into(),
            strategy,
            risk_limits,
            market_context_hash: market_context_hash.into(),
            created_at: Utc::now(),
        }
    }

    /// Deterministic artifact hash based on canonical JSON serialization.
    pub fn hash(&self) -> Result<String, serde_json::Error> {
        deterministic_hash(self)
    }

    /// Validate required fields for runtime safety checks.
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.intent_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("intent_id"));
        }
        if self.agent_id.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("agent_id"));
        }
        if self.user_id.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("user_id"));
        }
        if self.market_context_hash.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("market_context_hash"));
        }
        Ok(())
    }
}

/// Execution mode for a receipt.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Paper,
    Live,
}

/// Direction of an execution fill.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionSide {
    Buy,
    Sell,
}

/// A simulated fill in paper mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimulatedFill {
    pub quantity: Decimal,
    pub price: Decimal,
}

/// Immutable execution artifact emitted for an accepted intent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionReceipt {
    pub receipt_id: Uuid,
    pub intent_id: Uuid,
    pub mode: ExecutionMode,
    pub symbol: String,
    pub side: ExecutionSide,
    pub notional: Decimal,
    pub price_ref: Decimal,
    #[serde(default)]
    pub simulated_fills: Vec<SimulatedFill>,
    pub decision_hash: String,
    pub created_at: DateTime<Utc>,
}

impl ExecutionReceipt {
    /// Construct a new execution receipt.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        intent_id: Uuid,
        mode: ExecutionMode,
        symbol: impl Into<String>,
        side: ExecutionSide,
        notional: Decimal,
        price_ref: Decimal,
        simulated_fills: Vec<SimulatedFill>,
        decision_hash: impl Into<String>,
    ) -> Self {
        Self {
            receipt_id: Uuid::new_v4(),
            intent_id,
            mode,
            symbol: symbol.into(),
            side,
            notional,
            price_ref,
            simulated_fills,
            decision_hash: decision_hash.into(),
            created_at: Utc::now(),
        }
    }

    /// Deterministic artifact hash based on canonical JSON serialization.
    pub fn hash(&self) -> Result<String, serde_json::Error> {
        deterministic_hash(self)
    }

    /// Validate required fields for runtime safety checks.
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.receipt_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("receipt_id"));
        }
        if self.intent_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("intent_id"));
        }
        if self.symbol.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("symbol"));
        }
        if self.notional <= Decimal::ZERO {
            return Err(ArtifactValidationError::NonPositiveNumber("notional"));
        }
        if self.price_ref <= Decimal::ZERO {
            return Err(ArtifactValidationError::NonPositiveNumber("price_ref"));
        }
        validate_hash("decision_hash", &self.decision_hash)?;
        Ok(())
    }
}

/// Verification backend that produced a record.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationBackend {
    EigencloudPrimary,
    SignedFallback,
}

/// Verification lifecycle state for a receipt.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Pending,
    Verified,
    Failed,
}

impl VerificationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Verified => "verified",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "verified" => Some(Self::Verified),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

impl std::str::FromStr for VerificationStatus {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or(())
    }
}

/// Verification artifact linked to an execution receipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationRecord {
    pub verification_id: Uuid,
    pub receipt_id: Uuid,
    pub backend: VerificationBackend,
    pub proof_ref: String,
    pub status: VerificationStatus,
    pub verified_at: DateTime<Utc>,
}

impl VerificationRecord {
    /// Construct a new verification record.
    pub fn new(
        receipt_id: Uuid,
        backend: VerificationBackend,
        proof_ref: impl Into<String>,
        status: VerificationStatus,
    ) -> Self {
        Self {
            verification_id: Uuid::new_v4(),
            receipt_id,
            backend,
            proof_ref: proof_ref.into(),
            status,
            verified_at: Utc::now(),
        }
    }

    /// Deterministic artifact hash based on canonical JSON serialization.
    pub fn hash(&self) -> Result<String, serde_json::Error> {
        deterministic_hash(self)
    }

    /// Validate required fields for runtime safety checks.
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.verification_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("verification_id"));
        }
        if self.receipt_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("receipt_id"));
        }
        if self.proof_ref.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("proof_ref"));
        }
        Ok(())
    }
}

/// Document used to compute the audit-chain hash.
#[derive(Serialize)]
struct AuditChainPayload<'a> {
    contract_version: &'static str,
    hash_algorithm: &'static str,
    signal_hash: Option<&'a str>,
    intent_hash: &'a str,
    receipt_hash: &'a str,
    verification_hash: Option<&'a str>,
    settlement_hash: Option<&'a str>,
}

/// Canonical audit record linking intent -> receipt -> verification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntentAuditRecord {
    pub intent_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_hash: Option<String>,
    pub intent_hash: String,
    pub receipt_id: Uuid,
    pub receipt_hash: String,
    pub verification_id: Option<Uuid>,
    pub verification_hash: Option<String>,
    pub verification_status: Option<VerificationStatus>,
    pub settlement_id: Option<Uuid>,
    pub settlement_hash: Option<String>,
    #[serde(default)]
    pub provider_attributions: Vec<ProviderRevenueAttribution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirrored_pnl_usd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revenue_share_fee_usd: Option<String>,
    pub workspace_path: String,
    pub chain_hash: String,
    pub created_at: DateTime<Utc>,
}

impl IntentAuditRecord {
    /// Intent workspace document directory.
    pub const WORKSPACE_PATH_PREFIX: &'static str = "audits/intents";

    /// Settings key for the latest audit record per user.
    pub const SETTINGS_KEY: &'static str = "audit.intent_chain.latest";

    /// Build a chain record from the normalized artifacts.
    pub fn from_artifacts(
        agent_id: Option<Uuid>,
        user_id: impl Into<String>,
        intent: &IntentEnvelope,
        receipt: &ExecutionReceipt,
        verification: Option<&VerificationRecord>,
    ) -> Result<Self, serde_json::Error> {
        let intent_hash = intent.hash()?;
        let receipt_hash = receipt.hash()?;
        let verification_hash = verification.map(|record| record.hash()).transpose()?;

        let chain_hash = deterministic_hash(&AuditChainPayload {
            contract_version: INTENT_ARTIFACT_CONTRACT_VERSION,
            hash_algorithm: INTENT_ARTIFACT_HASH_ALGORITHM,
            signal_hash: None,
            intent_hash: &intent_hash,
            receipt_hash: &receipt_hash,
            verification_hash: verification_hash.as_deref(),
            settlement_hash: None,
        })?;

        let workspace_path = format!("{}/{}.json", Self::WORKSPACE_PATH_PREFIX, intent.intent_id);

        Ok(Self {
            intent_id: intent.intent_id,
            agent_id,
            user_id: user_id.into(),
            signal_hash: None,
            intent_hash,
            receipt_id: receipt.receipt_id,
            receipt_hash,
            verification_id: verification.map(|record| record.verification_id),
            verification_hash,
            verification_status: verification.map(|record| record.status),
            settlement_id: None,
            settlement_hash: None,
            provider_attributions: Vec::new(),
            mirrored_pnl_usd: None,
            revenue_share_fee_usd: None,
            workspace_path,
            chain_hash,
            created_at: Utc::now(),
        })
    }

    /// Attach copytrading attribution + settlement lineage and recalculate chain hash.
    pub fn with_copytrade_lineage(
        mut self,
        signal_hash: Option<String>,
        settlement: Option<&RevenueShareSettlementReceipt>,
        provider_attributions: Vec<ProviderRevenueAttribution>,
    ) -> Result<Self, serde_json::Error> {
        if let Some(ref hash) = signal_hash
            && let Err(error) = validate_hash("signal_hash", hash)
        {
            return Err(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                error.to_string(),
            )));
        }

        let settlement_hash = settlement.map(|record| record.hash()).transpose()?;
        self.signal_hash = signal_hash;
        self.settlement_id = settlement.map(|record| record.settlement_id);
        self.settlement_hash = settlement_hash.clone();
        self.provider_attributions = provider_attributions;
        self.mirrored_pnl_usd =
            settlement.map(|record| record.total_pnl_usd.normalize().to_string());
        self.revenue_share_fee_usd =
            settlement.map(|record| record.total_fee_usd.normalize().to_string());

        self.chain_hash = deterministic_hash(&AuditChainPayload {
            contract_version: INTENT_ARTIFACT_CONTRACT_VERSION,
            hash_algorithm: INTENT_ARTIFACT_HASH_ALGORITHM,
            signal_hash: self.signal_hash.as_deref(),
            intent_hash: &self.intent_hash,
            receipt_hash: &self.receipt_hash,
            verification_hash: self.verification_hash.as_deref(),
            settlement_hash: settlement_hash.as_deref(),
        })?;

        Ok(self)
    }

    /// Path where this audit record should be stored inside the workspace.
    pub fn workspace_document_path(&self) -> &str {
        &self.workspace_path
    }

    /// Structured JSON payload to persist in workspace memory for replay.
    pub fn workspace_document_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "contract_version": INTENT_ARTIFACT_CONTRACT_VERSION,
            "hash_algorithm": INTENT_ARTIFACT_HASH_ALGORITHM,
            "user_id": self.user_id,
            "agent_id": self.agent_id,
            "signal_hash": self.signal_hash,
            "intent_id": self.intent_id,
            "intent_hash": self.intent_hash,
            "receipt_id": self.receipt_id,
            "receipt_hash": self.receipt_hash,
            "verification_id": self.verification_id,
            "verification_hash": self.verification_hash,
            "verification_status": self.verification_status,
            "settlement_id": self.settlement_id,
            "settlement_hash": self.settlement_hash,
            "provider_attributions": self.provider_attributions,
            "mirrored_pnl_usd": self.mirrored_pnl_usd,
            "revenue_share_fee_usd": self.revenue_share_fee_usd,
            "proof_lineage": {
                "signal_hash": self.signal_hash,
                "intent_hash": self.intent_hash,
                "execution_receipt_hash": self.receipt_hash,
                "verification_record_hash": self.verification_hash,
                "settlement_hash": self.settlement_hash,
            },
            "workspace_path": self.workspace_path,
            "chain_hash": self.chain_hash,
            "created_at": self.created_at,
        })
    }
}

/// Canonical artifact hashing used by WS-3 intent pipeline artifacts.
pub fn deterministic_hash<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(value)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    #[test]
    fn deterministic_hash_is_stable() {
        let mut strategy = StrategyPayload::new();
        strategy.insert("name".to_string(), serde_json::json!("mean_reversion"));
        strategy.insert("window".to_string(), serde_json::json!(14));

        let mut risk_limits = RiskLimitsPayload::new();
        risk_limits.insert("max_notional".to_string(), serde_json::json!(1000));

        let intent = IntentEnvelope {
            intent_id: Uuid::nil(),
            agent_id: "agent-1".to_string(),
            user_id: "user-1".to_string(),
            strategy,
            risk_limits,
            market_context_hash: "ctx".to_string(),
            created_at: DateTime::UNIX_EPOCH,
        };

        let h1 = intent.hash().unwrap();
        let h2 = intent.hash().unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn execution_receipt_hash_changes_with_payload() {
        let base = ExecutionReceipt {
            receipt_id: Uuid::nil(),
            intent_id: Uuid::nil(),
            mode: ExecutionMode::Paper,
            symbol: "BTC".to_string(),
            side: ExecutionSide::Buy,
            notional: dec!(100),
            price_ref: dec!(50000),
            simulated_fills: vec![SimulatedFill {
                quantity: dec!(0.002),
                price: dec!(50000),
            }],
            decision_hash: "abc".to_string(),
            created_at: DateTime::UNIX_EPOCH,
        };

        let mut changed = base.clone();
        changed.decision_hash = "def".to_string();

        assert_ne!(base.hash().unwrap(), changed.hash().unwrap());
    }

    #[test]
    fn intent_validation_rejects_empty_agent() {
        let mut intent = IntentEnvelope::new(
            "agent",
            "user",
            StrategyPayload::new(),
            RiskLimitsPayload::new(),
            "ctx",
        );
        intent.agent_id = "   ".to_string();
        assert_eq!(
            intent.validate().unwrap_err(),
            ArtifactValidationError::EmptyField("agent_id")
        );
    }

    #[test]
    fn receipt_validation_rejects_invalid_hash() {
        let receipt = ExecutionReceipt {
            receipt_id: Uuid::new_v4(),
            intent_id: Uuid::new_v4(),
            mode: ExecutionMode::Paper,
            symbol: "ETH-USD".to_string(),
            side: ExecutionSide::Buy,
            notional: dec!(10),
            price_ref: dec!(1000),
            simulated_fills: vec![],
            decision_hash: "not-a-hash".to_string(),
            created_at: Utc::now(),
        };
        assert_eq!(
            receipt.validate().unwrap_err(),
            ArtifactValidationError::InvalidHash("decision_hash")
        );
    }

    #[test]
    fn verification_validation_rejects_empty_proof() {
        let verification = VerificationRecord {
            verification_id: Uuid::new_v4(),
            receipt_id: Uuid::new_v4(),
            backend: VerificationBackend::SignedFallback,
            proof_ref: String::new(),
            status: VerificationStatus::Pending,
            verified_at: Utc::now(),
        };
        assert_eq!(
            verification.validate().unwrap_err(),
            ArtifactValidationError::EmptyField("proof_ref")
        );
    }

    #[test]
    fn copytrading_profile_validation_requires_positive_bounds() {
        let profile = CopyTradingInitializationProfile {
            max_allocation_usd: Decimal::ZERO,
            per_trade_notional_cap_usd: dec!(100),
            max_leverage: dec!(2),
            symbol_allowlist: vec!["BTC-USD".to_string()],
            symbol_denylist: vec![],
            max_slippage_bps: 25,
            information_sharing_scope: InformationSharingScope::SignalsOnly,
        };
        assert_eq!(
            profile.validate().unwrap_err(),
            ArtifactValidationError::NonPositiveNumber("max_allocation_usd")
        );
    }

    #[test]
    fn a2a_message_hash_is_stable() {
        let contract = A2aSignalPublicationContract {
            signal_id: "sig_1".to_string(),
            provider: SignalProviderIdentity {
                provider_id: "provider-alpha".to_string(),
                payout_address: "0xabc".to_string(),
                display_name: Some("Alpha".to_string()),
            },
            symbol: "BTC-USD".to_string(),
            side: ExecutionSide::Buy,
            confidence_bps: 8200,
            price_ref: dec!(50000),
            signal_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            published_at: DateTime::UNIX_EPOCH,
        };

        let msg = A2aCopyTradingMessage::SignalPublication(contract);
        let h1 = msg.hash().unwrap();
        let h2 = msg.hash().unwrap();
        assert_eq!(h1, h2);
    }
}
