//! Hyperliquid paper-execution tool surface.

use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::agent::{
    CopyTradingInitializationProfile, InformationSharingScope, compile_copytrade_policy,
};
use crate::context::JobContext;
use crate::tools::tool::{Tool, ToolError, ToolOutput};

pub const HYPERLIQUID_EXECUTE_TOOL_NAME: &str = "hyperliquid_execute";
const DEFAULT_TRADING_ENDPOINT: &str = "https://api.hyperliquid-testnet.xyz";
const TRADING_ENDPOINT_HOST_ALLOWLIST: &[&str] =
    &["api.hyperliquid.xyz", "api.hyperliquid-testnet.xyz"];
const VERIFICATION_ENDPOINT_HOST_ALLOWLIST: &[&str] = &[
    "verify.eigencloud.xyz",
    "verify.eigencloud.example",
    "localhost",
    "127.0.0.1",
];

/// Tool for deterministic Hyperliquid paper execution simulation.
#[derive(Debug, Default)]
pub struct HyperliquidExecuteTool;
impl HyperliquidExecuteTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Deserialize)]
struct HyperliquidExecuteParams {
    intent_id: String,
    symbol: String,
    side: String,
    notional: String,
    price_ref: String,
    #[serde(default)]
    leverage: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    paper_live_policy: Option<String>,
    #[serde(default)]
    live_policy_gate: Option<bool>,
    #[serde(default)]
    trading_endpoint: Option<String>,
    #[serde(default)]
    verification_endpoint: Option<String>,
    #[serde(default)]
    market_context_hash: Option<String>,
    #[serde(default)]
    risk_limits: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    source_signal_hash: Option<String>,
    #[serde(default)]
    wallet_attestation_hash: Option<String>,
    #[serde(default)]
    natural_language_policy: Option<String>,
    #[serde(default)]
    copytrading_profile: Option<CopyTradingProfileInput>,
}

#[derive(Debug, Clone, Deserialize)]
struct CopyTradingProfileInput {
    max_allocation_usd: String,
    per_trade_notional_cap_usd: String,
    max_leverage: String,
    #[serde(default)]
    symbol_allowlist: Vec<String>,
    #[serde(default)]
    symbol_denylist: Vec<String>,
    max_slippage_bps: u32,
    information_sharing_scope: String,
}

impl CopyTradingProfileInput {
    fn to_runtime(&self) -> Result<CopyTradingInitializationProfile, ToolError> {
        let information_sharing_scope = match self
            .information_sharing_scope
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "none" => InformationSharingScope::None,
            "signals_only" | "signals" => InformationSharingScope::SignalsOnly,
            "signals_and_execution" | "execution" => InformationSharingScope::SignalsAndExecution,
            "full_audit" | "full" => InformationSharingScope::FullAudit,
            other => {
                return Err(ToolError::InvalidParameters(format!(
                    "invalid information_sharing_scope '{}'",
                    other
                )));
            }
        };

        let profile = CopyTradingInitializationProfile {
            max_allocation_usd: parse_decimal("max_allocation_usd", &self.max_allocation_usd)?,
            per_trade_notional_cap_usd: parse_decimal(
                "per_trade_notional_cap_usd",
                &self.per_trade_notional_cap_usd,
            )?,
            max_leverage: parse_decimal("max_leverage", &self.max_leverage)?,
            symbol_allowlist: self
                .symbol_allowlist
                .iter()
                .map(|symbol| symbol.trim().to_ascii_uppercase())
                .filter(|symbol| !symbol.is_empty())
                .collect(),
            symbol_denylist: self
                .symbol_denylist
                .iter()
                .map(|symbol| symbol.trim().to_ascii_uppercase())
                .filter(|symbol| !symbol.is_empty())
                .collect(),
            max_slippage_bps: self.max_slippage_bps,
            information_sharing_scope,
        };
        profile
            .validate()
            .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
        Ok(profile)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ExecutionMode {
    Paper,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaperLivePolicy {
    PaperOnly,
    PaperFirst,
    LiveAllowed,
}

impl PaperLivePolicy {
    fn parse(raw: Option<&str>) -> Result<Self, ToolError> {
        match raw
            .unwrap_or("paper_first")
            .trim()
            .to_ascii_lowercase()
            .replace('-', "_")
            .as_str()
        {
            "paper" | "paper_only" => Ok(Self::PaperOnly),
            "paper_first" => Ok(Self::PaperFirst),
            "live" | "live_allowed" => Ok(Self::LiveAllowed),
            other => Err(ToolError::InvalidParameters(format!(
                "invalid paper_live_policy '{}': expected 'paper_only', 'paper_first', or 'live_allowed'",
                other
            ))),
        }
    }

    fn allows_live(self) -> bool {
        matches!(self, Self::LiveAllowed)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::PaperOnly => "paper_only",
            Self::PaperFirst => "paper_first",
            Self::LiveAllowed => "live_allowed",
        }
    }
}

impl ExecutionMode {
    fn parse(raw: Option<&str>) -> Result<Self, ToolError> {
        match raw.unwrap_or("paper").trim().to_ascii_lowercase().as_str() {
            "paper" => Ok(Self::Paper),
            "live" => Ok(Self::Live),
            other => Err(ToolError::InvalidParameters(format!(
                "invalid mode '{}': expected 'paper' or 'live'",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ExecutionSide {
    Buy,
    Sell,
}

impl ExecutionSide {
    fn parse(raw: &str) -> Result<Self, ToolError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "buy" => Ok(Self::Buy),
            "sell" => Ok(Self::Sell),
            other => Err(ToolError::InvalidParameters(format!(
                "invalid side '{}': expected 'buy' or 'sell'",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct SimulatedFill {
    quantity: String,
    price: String,
}

#[derive(Debug, Clone, Serialize)]
struct ExecutionReceipt {
    receipt_id: String,
    intent_id: String,
    mode: ExecutionMode,
    symbol: String,
    side: ExecutionSide,
    notional: String,
    price_ref: String,
    simulated_fills: Vec<SimulatedFill>,
    decision_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_signal_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet_attestation_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copytrade_checks: Option<CopytradeCheckReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    proof_lineage: Option<CopytradeProofLineage>,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct CopytradeCheckReport {
    policy_compiled: bool,
    wallet_attestation_verified: bool,
    source_signal_verified: bool,
    symbol_allowed: bool,
    leverage_allowed: bool,
    notional_allowed: bool,
    slippage_allowed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CopytradeProofLineage {
    signal_hash: String,
    intent_hash: String,
    execution_receipt_hash: String,
}

#[derive(Debug, Serialize)]
struct DecisionSeed<'a> {
    intent_id: &'a str,
    mode: ExecutionMode,
    symbol: &'a str,
    side: ExecutionSide,
    notional: &'a str,
    price_ref: &'a str,
    leverage: Option<&'a str>,
    market_context_hash: Option<&'a str>,
    risk_limits: &'a BTreeMap<String, serde_json::Value>,
    source_signal_hash: Option<&'a str>,
    wallet_attestation_hash: Option<&'a str>,
    policy_hash: Option<&'a str>,
    simulated_fills: &'a [SimulatedFill],
}

fn parse_decimal(name: &str, value: &str) -> Result<Decimal, ToolError> {
    Decimal::from_str(value)
        .map_err(|e| ToolError::InvalidParameters(format!("invalid {} '{}': {}", name, value, e)))
}

fn parse_symbol(raw: &str) -> Result<String, ToolError> {
    let normalized = raw.trim().to_ascii_uppercase();
    if normalized.is_empty() {
        return Err(ToolError::InvalidParameters(
            "symbol cannot be empty".to_string(),
        ));
    }
    if !normalized
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(ToolError::InvalidParameters(format!(
            "invalid symbol '{}': only [A-Z0-9_-] are allowed",
            raw
        )));
    }
    Ok(normalized)
}

fn policy_from_ctx_metadata(ctx: &JobContext) -> Option<&str> {
    ctx.metadata
        .get("hyperliquid_runtime")
        .and_then(|runtime| runtime.get("paper_live_policy"))
        .and_then(|value| value.as_str())
        .or_else(|| {
            ctx.metadata
                .get("paper_live_policy")
                .and_then(|value| value.as_str())
        })
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

fn validate_external_endpoint(
    endpoint_url: &str,
    host_allowlist: &[&str],
    scheme_allowlist: &[&str],
) -> Result<(), String> {
    let parsed = url::Url::parse(endpoint_url).map_err(|e| format!("URL parse failed: {e}"))?;

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
fn deterministic_fills(
    side: ExecutionSide,
    notional: Decimal,
    price_ref: Decimal,
) -> Result<Vec<SimulatedFill>, ToolError> {
    if notional <= Decimal::ZERO {
        return Err(ToolError::InvalidParameters(
            "notional must be greater than zero".to_string(),
        ));
    }
    if price_ref <= Decimal::ZERO {
        return Err(ToolError::InvalidParameters(
            "price_ref must be greater than zero".to_string(),
        ));
    }

    let quantity = (notional / price_ref).round_dp(8);
    let quantity_a = (quantity * dec!(0.6)).round_dp(8);
    let quantity_b = (quantity - quantity_a).round_dp(8);

    let up_slippage = dec!(1.0005);
    let down_slippage = dec!(0.9995);
    let (price_a, price_b) = match side {
        ExecutionSide::Buy => (price_ref * up_slippage, price_ref * dec!(1.0010)),
        ExecutionSide::Sell => (price_ref * down_slippage, price_ref * dec!(0.9990)),
    };

    Ok(vec![
        SimulatedFill {
            quantity: quantity_a.normalize().to_string(),
            price: price_a.round_dp(8).normalize().to_string(),
        },
        SimulatedFill {
            quantity: quantity_b.normalize().to_string(),
            price: price_b.round_dp(8).normalize().to_string(),
        },
    ])
}

fn decision_hash(seed: &DecisionSeed<'_>) -> Result<String, ToolError> {
    let payload = serde_json::to_vec(seed).map_err(|e| {
        ToolError::ExecutionFailed(format!("failed to serialize decision seed: {}", e))
    })?;
    Ok(blake3::hash(&payload).to_hex().to_string())
}

fn validate_hash(field: &str, value: &str) -> Result<(), ToolError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(ToolError::InvalidParameters(format!(
            "{} must be a 64-char lowercase hex hash",
            field
        )));
    }
    Ok(())
}

fn max_slippage_bps(fills: &[SimulatedFill], price_ref: Decimal) -> Result<Decimal, ToolError> {
    let mut max = Decimal::ZERO;
    for fill in fills {
        let fill_price = parse_decimal("simulated_fill.price", &fill.price)?;
        let slippage = ((fill_price - price_ref).abs() / price_ref * dec!(10000)).round_dp(4);
        if slippage > max {
            max = slippage;
        }
    }
    Ok(max)
}

#[async_trait]
impl Tool for HyperliquidExecuteTool {
    fn name(&self) -> &str {
        HYPERLIQUID_EXECUTE_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Simulates a deterministic Hyperliquid execution and returns an immutable receipt."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "intent_id": {
                    "type": "string",
                    "description": "Intent identifier from upstream intent ingestion."
                },
                "mode": {
                    "type": "string",
                    "enum": ["paper", "live"],
                    "description": "Execution mode. Defaults to paper."
                },
                "paper_live_policy": {
                    "type": "string",
                    "enum": ["paper_only", "paper_first", "live_allowed"],
                    "description": "Policy gate for live execution. Defaults to paper_first."
                },
                "live_policy_gate": {
                    "type": "boolean",
                    "description": "Explicit live execution acknowledgement. Required when mode=live."
                },
                "trading_endpoint": {
                    "type": "string",
                    "description": "Trading API endpoint to validate against allowlist."
                },
                "verification_endpoint": {
                    "type": "string",
                    "description": "Verification API endpoint to validate against allowlist."
                },
                "symbol": {
                    "type": "string",
                    "description": "Hyperliquid symbol (for example BTC-USD)."
                },
                "side": {
                    "type": "string",
                    "enum": ["buy", "sell"],
                    "description": "Execution side."
                },
                "notional": {
                    "type": "string",
                    "description": "Notional value as decimal string."
                },
                "price_ref": {
                    "type": "string",
                    "description": "Reference price as decimal string."
                },
                "leverage": {
                    "type": "string",
                    "description": "Optional leverage multiplier as decimal string. Defaults to 1."
                },
                "market_context_hash": {
                    "type": "string",
                    "description": "Optional market-context digest used by decisioning."
                },
                "risk_limits": {
                    "type": "object",
                    "description": "Optional normalized risk-limit envelope."
                },
                "source_signal_hash": {
                    "type": "string",
                    "description": "Required for copytrading mode: hash of source signal."
                },
                "wallet_attestation_hash": {
                    "type": "string",
                    "description": "Required for copytrading mode: verified wallet attestation hash."
                },
                "natural_language_policy": {
                    "type": "string",
                    "description": "Natural-language copytrading policy compiled into deterministic checks."
                },
                "copytrading_profile": {
                    "type": "object",
                    "description": "Fixed copytrading initialization profile used for deterministic policy enforcement."
                }
            },
            "required": ["intent_id", "symbol", "side", "notional", "price_ref"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let parsed: HyperliquidExecuteParams = serde_json::from_value(params).map_err(|e| {
            ToolError::InvalidParameters(format!("invalid hyperliquid_execute parameters: {}", e))
        })?;

        if parsed.intent_id.trim().is_empty() {
            return Err(ToolError::InvalidParameters(
                "intent_id cannot be empty".to_string(),
            ));
        }

        let mode = ExecutionMode::parse(parsed.mode.as_deref())?;
        let policy = PaperLivePolicy::parse(
            parsed
                .paper_live_policy
                .as_deref()
                .or_else(|| policy_from_ctx_metadata(ctx)),
        )?;

        let trading_endpoint = parsed
            .trading_endpoint
            .as_deref()
            .unwrap_or(DEFAULT_TRADING_ENDPOINT);
        validate_external_endpoint(
            trading_endpoint,
            TRADING_ENDPOINT_HOST_ALLOWLIST,
            &["https"],
        )
        .map_err(|reason| {
            ToolError::NotAuthorized(format!(
                "trading endpoint '{}' blocked by allowlist: {}",
                trading_endpoint, reason
            ))
        })?;

        if let Some(verification_endpoint) = parsed.verification_endpoint.as_deref() {
            validate_external_endpoint(
                verification_endpoint,
                VERIFICATION_ENDPOINT_HOST_ALLOWLIST,
                &["https"],
            )
            .map_err(|reason| {
                ToolError::NotAuthorized(format!(
                    "verification endpoint '{}' blocked by allowlist: {}",
                    verification_endpoint, reason
                ))
            })?;
        }

        if mode == ExecutionMode::Live && !policy.allows_live() {
            return Err(ToolError::NotAuthorized(format!(
                "live execution blocked by policy '{}'; requires 'live_allowed'",
                policy.as_str()
            )));
        }
        if mode == ExecutionMode::Live && !parsed.live_policy_gate.unwrap_or(false) {
            return Err(ToolError::NotAuthorized(
                "live execution blocked: explicit live_policy_gate=true is required".to_string(),
            ));
        }

        let symbol = parse_symbol(&parsed.symbol)?;
        let side = ExecutionSide::parse(&parsed.side)?;
        let notional = parse_decimal("notional", &parsed.notional)?;
        let price_ref = parse_decimal("price_ref", &parsed.price_ref)?;
        let leverage = parse_decimal("leverage", parsed.leverage.as_deref().unwrap_or("1"))?;

        let source_signal_hash = parsed
            .source_signal_hash
            .as_ref()
            .map(|hash| hash.trim().to_string())
            .filter(|hash| !hash.is_empty());
        if let Some(ref hash) = source_signal_hash {
            validate_hash("source_signal_hash", hash)?;
        }

        let wallet_attestation_hash = parsed
            .wallet_attestation_hash
            .as_ref()
            .map(|hash| hash.trim().to_string())
            .filter(|hash| !hash.is_empty());
        if let Some(ref hash) = wallet_attestation_hash {
            validate_hash("wallet_attestation_hash", hash)?;
        }

        let copytrade_requested = source_signal_hash.is_some()
            || wallet_attestation_hash.is_some()
            || parsed.natural_language_policy.is_some()
            || parsed.copytrading_profile.is_some();

        let mut compiled_policy_hash: Option<String> = None;
        let mut compiled_max_slippage_bps: Option<u32> = None;
        let mut copytrade_report: Option<CopytradeCheckReport> = None;
        let mut proof_lineage: Option<CopytradeProofLineage> = None;

        if copytrade_requested {
            let profile_input = parsed.copytrading_profile.as_ref().ok_or_else(|| {
                ToolError::InvalidParameters(
                    "copytrading_profile is required when copytrading controls are provided"
                        .to_string(),
                )
            })?;
            let profile = profile_input.to_runtime()?;
            let policy_text = parsed.natural_language_policy.as_deref().ok_or_else(|| {
                ToolError::InvalidParameters(
                    "natural_language_policy is required in copytrading mode".to_string(),
                )
            })?;
            let compiled_policy = compile_copytrade_policy(policy_text, &profile)
                .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;

            let signal_hash = source_signal_hash.as_deref().ok_or_else(|| {
                ToolError::InvalidParameters(
                    "source_signal_hash is required in copytrading mode".to_string(),
                )
            })?;
            let attestation_hash = wallet_attestation_hash.as_deref().ok_or_else(|| {
                ToolError::InvalidParameters(
                    "wallet_attestation_hash is required in copytrading mode".to_string(),
                )
            })?;

            let symbol_in_allowlist = profile.symbol_allowlist.iter().any(|item| item == &symbol);
            let symbol_in_denylist = profile.symbol_denylist.iter().any(|item| item == &symbol);

            if compiled_policy.enforce_symbol_allowlist && !symbol_in_allowlist {
                return Err(ToolError::ExecutionFailed(format!(
                    "copytrading policy rejected symbol '{}': not in allowlist",
                    symbol
                )));
            }
            if compiled_policy.enforce_symbol_denylist && symbol_in_denylist {
                return Err(ToolError::ExecutionFailed(format!(
                    "copytrading policy rejected symbol '{}': in denylist",
                    symbol
                )));
            }
            if leverage > compiled_policy.max_leverage {
                return Err(ToolError::ExecutionFailed(format!(
                    "copytrading policy rejected leverage {} > {}",
                    leverage.normalize(),
                    compiled_policy.max_leverage.normalize()
                )));
            }
            if notional > compiled_policy.max_notional_usd {
                return Err(ToolError::ExecutionFailed(format!(
                    "copytrading policy rejected notional {} > {}",
                    notional.normalize(),
                    compiled_policy.max_notional_usd.normalize()
                )));
            }
            validate_hash("source_signal_hash", signal_hash)?;
            validate_hash("wallet_attestation_hash", attestation_hash)?;
            compiled_policy_hash = Some(compiled_policy.policy_hash.clone());
            compiled_max_slippage_bps = Some(compiled_policy.max_slippage_bps);

            copytrade_report = Some(CopytradeCheckReport {
                policy_compiled: true,
                wallet_attestation_verified: true,
                source_signal_verified: true,
                symbol_allowed: true,
                leverage_allowed: true,
                notional_allowed: true,
                slippage_allowed: true,
            });
        }

        let fills = deterministic_fills(side, notional, price_ref)?;
        if let Some(ref mut report) = copytrade_report {
            let slippage = max_slippage_bps(&fills, price_ref)?;
            let max_bps = compiled_max_slippage_bps
                .or_else(|| {
                    parsed
                        .copytrading_profile
                        .as_ref()
                        .map(|profile| profile.max_slippage_bps)
                })
                .unwrap_or(25);
            if slippage > Decimal::from(max_bps) {
                report.slippage_allowed = false;
                return Err(ToolError::ExecutionFailed(format!(
                    "copytrading policy rejected slippage {} bps > {} bps",
                    slippage.normalize(),
                    max_bps
                )));
            }
        }

        let seed = DecisionSeed {
            intent_id: parsed.intent_id.as_str(),
            mode,
            symbol: symbol.as_str(),
            side,
            notional: parsed.notional.as_str(),
            price_ref: parsed.price_ref.as_str(),
            leverage: parsed.leverage.as_deref(),
            market_context_hash: parsed.market_context_hash.as_deref(),
            risk_limits: &parsed.risk_limits,
            source_signal_hash: source_signal_hash.as_deref(),
            wallet_attestation_hash: wallet_attestation_hash.as_deref(),
            policy_hash: compiled_policy_hash.as_deref(),
            simulated_fills: &fills,
        };
        let digest = decision_hash(&seed)?;
        let receipt_id = format!("rcpt_{}", &digest[..24]);
        let intent_hash = blake3::hash(parsed.intent_id.as_bytes())
            .to_hex()
            .to_string();
        if let Some(signal_hash) = source_signal_hash.clone() {
            proof_lineage = Some(CopytradeProofLineage {
                signal_hash,
                intent_hash,
                execution_receipt_hash: digest.clone(),
            });
        }

        let receipt = ExecutionReceipt {
            receipt_id,
            intent_id: parsed.intent_id,
            mode,
            symbol,
            side,
            notional: notional.normalize().to_string(),
            price_ref: price_ref.normalize().to_string(),
            simulated_fills: fills,
            decision_hash: digest,
            source_signal_hash,
            wallet_attestation_hash,
            policy_hash: compiled_policy_hash,
            copytrade_checks: copytrade_report,
            proof_lineage,
            created_at: Utc::now().to_rfc3339(),
        };

        let result = serde_json::to_value(receipt)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to encode receipt: {}", e)))?;
        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }

    fn requires_approval_for(&self, params: &serde_json::Value) -> bool {
        params
            .get("mode")
            .and_then(|v| v.as_str())
            .map(|m| m.eq_ignore_ascii_case("live"))
            .unwrap_or(false)
    }

    fn execution_timeout(&self) -> Duration {
        Duration::from_secs(15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tool_returns_deterministic_decision_hash() {
        let tool = HyperliquidExecuteTool::new();
        let ctx = JobContext::default();
        let params = serde_json::json!({
            "intent_id": "intent_123",
            "symbol": "BTC-USD",
            "side": "buy",
            "notional": "1000",
            "price_ref": "50000",
            "risk_limits": {
                "max_notional": "1200"
            }
        });

        let first = tool.execute(params.clone(), &ctx).await.unwrap();
        let second = tool.execute(params, &ctx).await.unwrap();

        let first_hash = first
            .result
            .get("decision_hash")
            .and_then(|v| v.as_str())
            .unwrap();
        let second_hash = second
            .result
            .get("decision_hash")
            .and_then(|v| v.as_str())
            .unwrap();

        assert_eq!(first_hash, second_hash);
    }

    #[tokio::test]
    async fn live_mode_requires_approval_override() {
        let tool = HyperliquidExecuteTool::new();
        assert!(tool.requires_approval_for(&serde_json::json!({ "mode": "live" })));
        assert!(!tool.requires_approval_for(&serde_json::json!({ "mode": "paper" })));
    }

    #[tokio::test]
    async fn live_mode_blocked_without_live_allowed_policy() {
        let tool = HyperliquidExecuteTool::new();
        let ctx = JobContext::default();
        let err = tool
            .execute(
                serde_json::json!({
                    "intent_id": "intent_live_blocked",
                    "mode": "live",
                    "live_policy_gate": true,
                    "symbol": "BTC-USD",
                    "side": "buy",
                    "notional": "100",
                    "price_ref": "50000"
                }),
                &ctx,
            )
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::NotAuthorized(_)));
        assert!(err.to_string().contains("live execution blocked by policy"));
    }

    #[tokio::test]
    async fn live_mode_requires_explicit_policy_gate_flag() {
        let tool = HyperliquidExecuteTool::new();
        let ctx = JobContext {
            metadata: serde_json::json!({
                "hyperliquid_runtime": {
                    "paper_live_policy": "live_allowed"
                }
            }),
            ..JobContext::default()
        };
        let err = tool
            .execute(
                serde_json::json!({
                    "intent_id": "intent_live_gate",
                    "mode": "live",
                    "symbol": "BTC-USD",
                    "side": "buy",
                    "notional": "100",
                    "price_ref": "50000"
                }),
                &ctx,
            )
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::NotAuthorized(_)));
        assert!(err.to_string().contains("live_policy_gate=true"));
    }

    #[tokio::test]
    async fn live_mode_allowed_with_live_policy_and_gate() {
        let tool = HyperliquidExecuteTool::new();
        let output = tool
            .execute(
                serde_json::json!({
                    "intent_id": "intent_live_allowed",
                    "mode": "live",
                    "paper_live_policy": "live_allowed",
                    "live_policy_gate": true,
                    "symbol": "BTC-USD",
                    "side": "buy",
                    "notional": "100",
                    "price_ref": "50000",
                    "trading_endpoint": "https://api.hyperliquid.xyz"
                }),
                &JobContext::default(),
            )
            .await
            .unwrap();

        assert_eq!(output.result["mode"], "live");
    }

    #[tokio::test]
    async fn blocks_non_allowlisted_trading_endpoint() {
        let tool = HyperliquidExecuteTool::new();
        let err = tool
            .execute(
                serde_json::json!({
                    "intent_id": "intent_bad_endpoint",
                    "symbol": "BTC-USD",
                    "side": "buy",
                    "notional": "100",
                    "price_ref": "50000",
                    "trading_endpoint": "https://evil.example"
                }),
                &JobContext::default(),
            )
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::NotAuthorized(_)));
        assert!(err.to_string().contains("trading endpoint"));
    }

    #[tokio::test]
    async fn blocks_non_allowlisted_verification_endpoint() {
        let tool = HyperliquidExecuteTool::new();
        let err = tool
            .execute(
                serde_json::json!({
                    "intent_id": "intent_bad_verify_endpoint",
                    "symbol": "BTC-USD",
                    "side": "buy",
                    "notional": "100",
                    "price_ref": "50000",
                    "verification_endpoint": "https://evil.example"
                }),
                &JobContext::default(),
            )
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::NotAuthorized(_)));
        assert!(err.to_string().contains("verification endpoint"));
    }

    #[tokio::test]
    async fn copytrading_mode_rejects_missing_attestation() {
        let tool = HyperliquidExecuteTool::new();
        let ctx = JobContext::default();
        let params = serde_json::json!({
            "intent_id": "intent_123",
            "symbol": "BTC-USD",
            "side": "buy",
            "notional": "100",
            "price_ref": "50000",
            "source_signal_hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "natural_language_policy": "max notional 100",
            "copytrading_profile": {
                "max_allocation_usd": "1000",
                "per_trade_notional_cap_usd": "100",
                "max_leverage": "2",
                "symbol_allowlist": ["BTC-USD"],
                "symbol_denylist": [],
                "max_slippage_bps": 25,
                "information_sharing_scope": "signals_only"
            }
        });

        let err = tool.execute(params, &ctx).await.unwrap_err();
        match err {
            ToolError::InvalidParameters(msg) => {
                assert!(msg.contains("wallet_attestation_hash is required"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
