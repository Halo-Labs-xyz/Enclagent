//! System commands and job handlers for the agent.
//!
//! Extracted from `agent_loop.rs` to isolate the /help, /model, /status,
//! and other command processing from the core agent loop.

use std::cmp::max;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::attestation::{
    build_copytrade_proof_lineage_hash, create_eigen_verification_job,
};
use crate::agent::session::Session;
use crate::agent::submission::SubmissionResult;
use crate::agent::{
    A2aExecutionIntentContract, Agent, CopyTradingInitializationProfile, ExecutionMode,
    ExecutionReceipt, ExecutionSide, InformationSharingScope, IntentAuditRecord, IntentEnvelope,
    MessageIntent, RiskLimitsPayload, SimulatedFill, StrategyPayload, VerificationBackend,
    VerificationRecord, VerificationStatus, compile_copytrade_policy, deterministic_hash,
};
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::context::JobContext;
use crate::error::{ConfigError, Error};
use crate::llm::ChatMessage;
use crate::secrets::{VerificationJobCredentialRef, VerificationJobProvider};
use crate::settings::Settings;

const COMMAND_SUGGESTIONS: &[&str] = &[
    "help",
    "model",
    "version",
    "tools",
    "ping",
    "debug",
    "positions",
    "exposure",
    "funding",
    "vault",
    "risk",
    "pause-agent",
    "resume-agent",
    "verify",
    "receipts",
    "copy-policy",
    "copy-status",
    "connectors",
    "heartbeat",
    "summarize",
    "suggest",
    "quit",
    "job",
    "status",
    "cancel",
    "list",
    "undo",
    "redo",
    "compact",
    "clear",
    "interrupt",
    "new",
    "thread",
    "resume",
];

const MAINNET_OVERRIDE_FLAG: &str = "--allow-mainnet";
const AUTO_ACTIVATE_DISABLE_FLAG: &str = "--no-auto-activate";
const INCLUDE_PNL_FLAG: &str = "--include-pnl";

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg.eq_ignore_ascii_case(flag))
}

fn strip_flag(args: &[String], flag: &str) -> Vec<String> {
    args.iter()
        .filter(|arg| !arg.eq_ignore_ascii_case(flag))
        .cloned()
        .collect()
}

fn parse_positive_usd_amount(raw: &str) -> Result<String, String> {
    let parsed = Decimal::from_str(raw.trim())
        .map_err(|_| format!("Invalid USD amount '{}': expected a decimal number.", raw))?;
    if parsed <= Decimal::ZERO {
        return Err("Amount must be greater than zero.".to_string());
    }
    if parsed.scale() > 6 {
        return Err("Amount supports at most 6 decimal places.".to_string());
    }
    Ok(parsed.round_dp(6).normalize().to_string())
}

fn is_hex_wallet_address(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 42 || !value.starts_with("0x") {
        return false;
    }
    bytes[2..].iter().all(|b| b.is_ascii_hexdigit())
}

fn render_json_block(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}

fn testnet_guard_error(network: &str, allow_mainnet: bool, operation: &str) -> Option<String> {
    if allow_mainnet || network.eq_ignore_ascii_case("testnet") {
        return None;
    }
    Some(format!(
        "{} is blocked while hyperliquid_runtime.network='{}'. Use testnet or pass {} explicitly.",
        operation, network, MAINNET_OVERRIDE_FLAG
    ))
}

fn canonicalize_system_command_name(command: &str) -> String {
    let lower = command.trim().trim_start_matches('/').to_ascii_lowercase();
    match lower.as_str() {
        "pos" | "position" => "positions".to_string(),
        "expo" => "exposure".to_string(),
        "fund" | "funds" => "funding".to_string(),
        "riskbook" => "risk".to_string(),
        "pause" | "pause_agent" | "pauseagent" | "agent-pause" => "pause-agent".to_string(),
        "resume_agent" | "resumeagent" | "agent-resume" => "resume-agent".to_string(),
        "verify-receipt" => "verify".to_string(),
        "receipt" | "receipt-log" => "receipts".to_string(),
        "copypolicy" | "copy_policy" => "copy-policy".to_string(),
        "copystatus" | "copy_status" => "copy-status".to_string(),
        _ => lower,
    }
}

fn parse_information_sharing_scope(value: &str) -> InformationSharingScope {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => InformationSharingScope::None,
        "signals_and_execution" | "execution" => InformationSharingScope::SignalsAndExecution,
        "full_audit" | "full" => InformationSharingScope::FullAudit,
        _ => InformationSharingScope::SignalsOnly,
    }
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();

    if a_chars.is_empty() {
        return b_chars.len();
    }
    if b_chars.is_empty() {
        return a_chars.len();
    }

    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0; b_chars.len() + 1];

    for (i, a_ch) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, b_ch) in b_chars.iter().enumerate() {
            let cost = usize::from(a_ch != b_ch);
            curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_chars.len()]
}

fn command_suggestions(command: &str) -> Vec<&'static str> {
    let normalized = canonicalize_system_command_name(command);
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut ranked: Vec<(&'static str, usize)> = COMMAND_SUGGESTIONS
        .iter()
        .map(|candidate| (*candidate, levenshtein_distance(&normalized, candidate)))
        .collect();
    ranked.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));

    let threshold = if normalized.len() <= 4 {
        1
    } else if normalized.len() <= 8 {
        2
    } else {
        max(3, normalized.len() / 3)
    };

    ranked
        .into_iter()
        .filter(|(candidate, distance)| {
            *distance <= threshold
                || candidate.starts_with(&normalized)
                || normalized.starts_with(*candidate)
        })
        .take(3)
        .map(|(candidate, _)| candidate)
        .collect()
}

fn unknown_command_message(command: &str) -> String {
    let suggestions = command_suggestions(command);
    if suggestions.is_empty() {
        return format!("Unknown command: {}. Try /help", command);
    }

    if suggestions.len() == 1 {
        return format!(
            "Unknown command: {}. Did you mean /{}? Try /help",
            command, suggestions[0]
        );
    }

    let formatted = suggestions
        .iter()
        .map(|suggestion| format!("/{}", suggestion))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Unknown command: {}. Did you mean one of: {}? Try /help",
        command, formatted
    )
}

fn mask_hex_address(address: Option<&str>) -> String {
    let Some(value) = address else {
        return "not configured".to_string();
    };
    if value.len() <= 12 {
        return value.to_string();
    }
    if value.starts_with("0x") && value.len() >= 12 {
        let head = &value[..6];
        let tail = &value[value.len() - 4..];
        return format!("{}...{}", head, tail);
    }
    value.to_string()
}

const VAULT_RUN_USAGE: &str = "Usage: /vault run <symbol> <buy|sell> <notional_usd> <price_ref> --attestation <64-hex> [--signal-hash <64-hex>] [--leverage <x>] [--mode <paper|live>] [--live-policy-gate] [--market-context-hash <64-hex>] [--strategy <name>] [--agent-id <uuid>]";

#[derive(Debug, Clone)]
struct VaultRunRequest {
    symbol: String,
    side: String,
    notional: Decimal,
    price_ref: Decimal,
    leverage: Option<Decimal>,
    mode: String,
    live_policy_gate: bool,
    wallet_attestation_hash: String,
    source_signal_hash: Option<String>,
    market_context_hash: Option<String>,
    strategy_name: String,
    agent_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
struct HyperliquidToolFill {
    quantity: String,
    price: String,
}

#[derive(Debug, Clone, Deserialize)]
struct HyperliquidToolReceipt {
    receipt_id: String,
    mode: String,
    symbol: String,
    side: String,
    notional: String,
    price_ref: String,
    simulated_fills: Vec<HyperliquidToolFill>,
    decision_hash: String,
    #[serde(default)]
    policy_hash: Option<String>,
    #[serde(default)]
    source_signal_hash: Option<String>,
    #[serde(default)]
    wallet_attestation_hash: Option<String>,
}

fn is_lower_hex_64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn parse_decimal_arg(name: &str, value: &str) -> Result<Decimal, String> {
    Decimal::from_str(value).map_err(|err| format!("invalid {name} '{}': {}", value, err))
}

fn take_flag_value(args: &[String], index: &mut usize, name: &str) -> Result<String, String> {
    let value = args
        .get(*index + 1)
        .ok_or_else(|| format!("{name} requires a value"))?
        .to_string();
    if value.starts_with("--") {
        return Err(format!("{name} requires a value"));
    }
    *index += 2;
    Ok(value)
}

fn parse_vault_run_args(args: &[String]) -> Result<VaultRunRequest, String> {
    let Some(command) = args.first() else {
        return Err(VAULT_RUN_USAGE.to_string());
    };
    if command != "run" && command != "execute" {
        return Err(VAULT_RUN_USAGE.to_string());
    }

    let mut index = 1usize;
    let mut positional = Vec::new();
    while index < args.len() && !args[index].starts_with("--") {
        positional.push(args[index].clone());
        index += 1;
    }

    if positional.len() < 4 {
        return Err(VAULT_RUN_USAGE.to_string());
    }

    let symbol = positional[0].trim().to_ascii_uppercase();
    let side = positional[1].trim().to_ascii_lowercase();
    if symbol.is_empty() {
        return Err("symbol cannot be empty".to_string());
    }
    if side != "buy" && side != "sell" {
        return Err("side must be 'buy' or 'sell'".to_string());
    }

    let notional = parse_decimal_arg("notional_usd", &positional[2])?;
    if notional <= Decimal::ZERO {
        return Err("notional_usd must be greater than zero".to_string());
    }
    let price_ref = parse_decimal_arg("price_ref", &positional[3])?;
    if price_ref <= Decimal::ZERO {
        return Err("price_ref must be greater than zero".to_string());
    }

    let mut leverage = None;
    let mut mode = "paper".to_string();
    let mut live_policy_gate = false;
    let mut wallet_attestation_hash: Option<String> = None;
    let mut source_signal_hash: Option<String> = None;
    let mut market_context_hash: Option<String> = None;
    let mut strategy_name = "vault_strategy_attested".to_string();
    let mut agent_id = None;

    while index < args.len() {
        match args[index].as_str() {
            "--attestation" | "--wallet-attestation-hash" => {
                let value = take_flag_value(args, &mut index, "--attestation")?;
                wallet_attestation_hash = Some(value);
            }
            "--signal-hash" => {
                let value = take_flag_value(args, &mut index, "--signal-hash")?;
                source_signal_hash = Some(value);
            }
            "--leverage" => {
                let value = take_flag_value(args, &mut index, "--leverage")?;
                let parsed = parse_decimal_arg("leverage", &value)?;
                if parsed <= Decimal::ZERO {
                    return Err("leverage must be greater than zero".to_string());
                }
                leverage = Some(parsed);
            }
            "--mode" => {
                let value = take_flag_value(args, &mut index, "--mode")?;
                let normalized = value.trim().to_ascii_lowercase();
                if normalized != "paper" && normalized != "live" {
                    return Err("mode must be 'paper' or 'live'".to_string());
                }
                mode = normalized;
            }
            "--live-policy-gate" => {
                live_policy_gate = true;
                index += 1;
            }
            "--market-context-hash" => {
                let value = take_flag_value(args, &mut index, "--market-context-hash")?;
                market_context_hash = Some(value);
            }
            "--strategy" => {
                let value = take_flag_value(args, &mut index, "--strategy")?;
                strategy_name = value.trim().to_string();
                if strategy_name.is_empty() {
                    return Err("strategy name cannot be empty".to_string());
                }
            }
            "--agent-id" => {
                let value = take_flag_value(args, &mut index, "--agent-id")?;
                agent_id = Some(
                    Uuid::parse_str(&value)
                        .map_err(|_| "agent-id must be a valid UUID".to_string())?,
                );
            }
            other => {
                return Err(format!(
                    "unknown /vault run option: {other}\n{VAULT_RUN_USAGE}"
                ));
            }
        }
    }

    let wallet_attestation_hash = wallet_attestation_hash
        .ok_or_else(|| format!("attested execution requires --attestation\n{VAULT_RUN_USAGE}"))?;
    if !is_lower_hex_64(&wallet_attestation_hash) {
        return Err("attestation hash must be a 64-char lowercase hex value".to_string());
    }
    if let Some(ref signal_hash) = source_signal_hash
        && !is_lower_hex_64(signal_hash)
    {
        return Err("signal hash must be a 64-char lowercase hex value".to_string());
    }
    if let Some(ref hash) = market_context_hash
        && !is_lower_hex_64(hash)
    {
        return Err("market_context_hash must be a 64-char lowercase hex value".to_string());
    }
    if mode == "live" && !live_policy_gate {
        return Err("live mode requires --live-policy-gate".to_string());
    }

    Ok(VaultRunRequest {
        symbol,
        side,
        notional,
        price_ref,
        leverage,
        mode,
        live_policy_gate,
        wallet_attestation_hash,
        source_signal_hash,
        market_context_hash,
        strategy_name,
        agent_id,
    })
}

fn map_tool_receipt_to_artifact(
    intent_id: Uuid,
    receipt: &HyperliquidToolReceipt,
) -> Result<ExecutionReceipt, String> {
    let mode = match receipt.mode.as_str() {
        "paper" => ExecutionMode::Paper,
        "live" => ExecutionMode::Live,
        other => return Err(format!("unexpected execution mode in receipt: {}", other)),
    };
    let side = match receipt.side.as_str() {
        "buy" => ExecutionSide::Buy,
        "sell" => ExecutionSide::Sell,
        other => return Err(format!("unexpected execution side in receipt: {}", other)),
    };

    let notional = parse_decimal_arg("receipt.notional", &receipt.notional)?;
    let price_ref = parse_decimal_arg("receipt.price_ref", &receipt.price_ref)?;

    let mut fills = Vec::with_capacity(receipt.simulated_fills.len());
    for fill in &receipt.simulated_fills {
        fills.push(SimulatedFill {
            quantity: parse_decimal_arg("receipt.simulated_fill.quantity", &fill.quantity)?,
            price: parse_decimal_arg("receipt.simulated_fill.price", &fill.price)?,
        });
    }

    Ok(ExecutionReceipt::new(
        intent_id,
        mode,
        receipt.symbol.clone(),
        side,
        notional,
        price_ref,
        fills,
        receipt.decision_hash.clone(),
    ))
}

fn short_hash(value: &str) -> String {
    if value.len() <= 12 {
        return value.to_string();
    }
    format!("{}...{}", &value[..8], &value[value.len() - 4..])
}

#[derive(Debug, Clone)]
enum ConnectorState {
    Healthy,
    Degraded,
    Disabled,
    AuthFailed,
}

impl ConnectorState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Disabled => "disabled",
            Self::AuthFailed => "auth_failed",
        }
    }
}

#[derive(Debug, Clone)]
struct ConnectorProbe {
    state: ConnectorState,
    detail: String,
}

fn effective_hyperliquid_api_base_url(settings: &Settings) -> String {
    if let Some(ref url) = settings.hyperliquid_runtime.api_base_url
        && !url.trim().is_empty()
    {
        return url.trim_end_matches('/').to_string();
    }
    if settings
        .hyperliquid_runtime
        .network
        .eq_ignore_ascii_case("mainnet")
    {
        "https://api.hyperliquid.xyz".to_string()
    } else {
        "https://api.hyperliquid-testnet.xyz".to_string()
    }
}

fn effective_bridge_base_url() -> Option<String> {
    let candidates = [
        std::env::var("ENCLAGENT_BRIDGE_URL").ok(),
        std::env::var("ENCLAGENT_BRIDGE_BASE_URL").ok(),
        std::env::var("HYPERLIQUID_BRIDGE_URL").ok(),
        std::env::var("HYPERLIQUID_BRIDGE_BASE_URL").ok(),
    ];

    for raw in candidates.into_iter().flatten() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if url::Url::parse(trimmed).is_ok() {
            return Some(trimmed.trim_end_matches('/').to_string());
        }
    }
    None
}

fn effective_bridge_api_key() -> Option<String> {
    let candidates = [
        std::env::var("ENCLAGENT_BRIDGE_API_KEY").ok(),
        std::env::var("HYPERLIQUID_BRIDGE_API_KEY").ok(),
        std::env::var("MCP_API_KEY").ok(),
    ];
    for raw in candidates.into_iter().flatten() {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

async fn probe_hyperliquid_native(settings: &Settings) -> ConnectorProbe {
    let base_url = effective_hyperliquid_api_base_url(settings);
    let timeout_ms = settings.hyperliquid_runtime.timeout_ms.clamp(500, 15_000);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return ConnectorProbe {
                state: ConnectorState::Degraded,
                detail: format!("client init failed: {}", error),
            };
        }
    };

    let endpoint = format!("{}/info", base_url);
    let start = std::time::Instant::now();
    let response = client
        .post(&endpoint)
        .json(&serde_json::json!({ "type": "meta" }))
        .send()
        .await;
    let latency_ms = start.elapsed().as_millis();

    match response {
        Ok(resp) if resp.status().is_success() => ConnectorProbe {
            state: ConnectorState::Healthy,
            detail: format!("{} ({} ms)", endpoint, latency_ms),
        },
        Ok(resp) => ConnectorProbe {
            state: ConnectorState::Degraded,
            detail: format!("{} returned HTTP {}", endpoint, resp.status()),
        },
        Err(error) => ConnectorProbe {
            state: ConnectorState::Degraded,
            detail: format!("{} unreachable: {}", endpoint, error),
        },
    }
}

async fn probe_bridge_service() -> ConnectorProbe {
    let Some(base_url) = effective_bridge_base_url() else {
        return ConnectorProbe {
            state: ConnectorState::Disabled,
            detail: "set ENCLAGENT_BRIDGE_URL (or ENCLAGENT_BRIDGE_BASE_URL)".to_string(),
        };
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return ConnectorProbe {
                state: ConnectorState::Degraded,
                detail: format!("client init failed: {}", error),
            };
        }
    };

    let health_url = format!("{}/api/health", base_url);
    let health_response = client.get(&health_url).send().await;
    if let Err(error) = health_response {
        return ConnectorProbe {
            state: ConnectorState::Degraded,
            detail: format!("{} unreachable: {}", health_url, error),
        };
    }

    let key = effective_bridge_api_key();
    let mcp_url = format!("{}/api/mcp", base_url);
    let mut request = client
        .post(&mcp_url)
        .json(&serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }));
    if let Some(ref k) = key {
        request = request
            .header("x-api-key", k)
            .header("authorization", format!("Bearer {}", k));
    }

    match request.send().await {
        Ok(resp) if resp.status().is_success() => ConnectorProbe {
            state: ConnectorState::Healthy,
            detail: format!("{} (MCP authorized)", base_url),
        },
        Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => ConnectorProbe {
            state: ConnectorState::AuthFailed,
            detail: format!("{} rejected API key", mcp_url),
        },
        Ok(resp) => ConnectorProbe {
            state: ConnectorState::Degraded,
            detail: format!("{} returned HTTP {}", mcp_url, resp.status()),
        },
        Err(error) => ConnectorProbe {
            state: ConnectorState::Degraded,
            detail: format!("{} unreachable: {}", mcp_url, error),
        },
    }
}

impl Agent {
    /// Handle job-related intents without turn tracking.
    pub(super) async fn handle_job_or_command(
        &self,
        intent: MessageIntent,
        message: &IncomingMessage,
    ) -> Result<SubmissionResult, Error> {
        // Send thinking status for non-trivial operations
        if let MessageIntent::CreateJob { .. } = &intent {
            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::Thinking("Processing...".into()),
                    &message.metadata,
                )
                .await;
        }

        let response = match intent {
            MessageIntent::CreateJob {
                title,
                description,
                category,
            } => {
                self.handle_create_job(&message.user_id, title, description, category)
                    .await?
            }
            MessageIntent::CheckJobStatus { job_id } => {
                self.handle_check_status(&message.user_id, job_id).await?
            }
            MessageIntent::CancelJob { job_id } => {
                self.handle_cancel_job(&message.user_id, &job_id).await?
            }
            MessageIntent::ListJobs { filter } => {
                self.handle_list_jobs(&message.user_id, filter).await?
            }
            MessageIntent::HelpJob { job_id } => {
                self.handle_help_job(&message.user_id, &job_id).await?
            }
            MessageIntent::Command { command, args } => {
                match self
                    .handle_command(&message.user_id, &command, &args)
                    .await?
                {
                    Some(s) => s,
                    None => return Ok(SubmissionResult::Ok { message: None }), // Shutdown signal
                }
            }
            _ => "Unknown intent".to_string(),
        };
        Ok(SubmissionResult::response(response))
    }

    async fn handle_create_job(
        &self,
        user_id: &str,
        title: String,
        description: String,
        category: Option<String>,
    ) -> Result<String, Error> {
        // Create job context
        let job_id = self
            .context_manager
            .create_job_for_user(user_id, &title, &description)
            .await?;

        // Update category if provided
        if let Some(cat) = category {
            self.context_manager
                .update_context(job_id, |ctx| {
                    ctx.category = Some(cat);
                })
                .await?;
        }

        // Persist new job to database (fire-and-forget)
        if let Some(store) = self.store()
            && let Ok(ctx) = self.context_manager.get_context(job_id).await
        {
            let store = store.clone();
            tokio::spawn(async move {
                if let Err(e) = store.save_job(&ctx).await {
                    tracing::warn!("Failed to persist new job {}: {}", job_id, e);
                }
            });
        }

        // Schedule for execution
        self.scheduler.schedule(job_id).await?;

        Ok(format!(
            "Created job: {}\nID: {}\n\nThe job has been scheduled and is now running.",
            title, job_id
        ))
    }

    async fn handle_check_status(
        &self,
        user_id: &str,
        job_id: Option<String>,
    ) -> Result<String, Error> {
        match job_id {
            Some(id) => {
                let uuid = Uuid::parse_str(&id)
                    .map_err(|_| crate::error::JobError::NotFound { id: Uuid::nil() })?;

                let ctx = self.context_manager.get_context(uuid).await?;
                if ctx.user_id != user_id {
                    return Err(crate::error::JobError::NotFound { id: uuid }.into());
                }

                Ok(format!(
                    "Job: {}\nStatus: {:?}\nCreated: {}\nStarted: {}\nActual cost: {}",
                    ctx.title,
                    ctx.state,
                    ctx.created_at.format("%Y-%m-%d %H:%M:%S"),
                    ctx.started_at
                        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "Not started".to_string()),
                    ctx.actual_cost
                ))
            }
            None => {
                // Show summary of all jobs
                let summary = self.context_manager.summary_for(user_id).await;
                Ok(format!(
                    "Jobs summary:\n  Total: {}\n  In Progress: {}\n  Completed: {}\n  Failed: {}\n  Stuck: {}",
                    summary.total,
                    summary.in_progress,
                    summary.completed,
                    summary.failed,
                    summary.stuck
                ))
            }
        }
    }

    async fn handle_cancel_job(&self, user_id: &str, job_id: &str) -> Result<String, Error> {
        let uuid = Uuid::parse_str(job_id)
            .map_err(|_| crate::error::JobError::NotFound { id: Uuid::nil() })?;

        let ctx = self.context_manager.get_context(uuid).await?;
        if ctx.user_id != user_id {
            return Err(crate::error::JobError::NotFound { id: uuid }.into());
        }

        self.scheduler.stop(uuid).await?;

        Ok(format!("Job {} has been cancelled.", job_id))
    }

    async fn handle_list_jobs(
        &self,
        user_id: &str,
        _filter: Option<String>,
    ) -> Result<String, Error> {
        let jobs = self.context_manager.all_jobs_for(user_id).await;

        if jobs.is_empty() {
            return Ok("No jobs found.".to_string());
        }

        let mut output = String::from("Jobs:\n");
        for job_id in jobs {
            if let Ok(ctx) = self.context_manager.get_context(job_id).await
                && ctx.user_id == user_id
            {
                output.push_str(&format!("  {} - {} ({:?})\n", job_id, ctx.title, ctx.state));
            }
        }

        Ok(output)
    }

    async fn handle_help_job(&self, user_id: &str, job_id: &str) -> Result<String, Error> {
        let uuid = Uuid::parse_str(job_id)
            .map_err(|_| crate::error::JobError::NotFound { id: Uuid::nil() })?;

        let ctx = self.context_manager.get_context(uuid).await?;
        if ctx.user_id != user_id {
            return Err(crate::error::JobError::NotFound { id: uuid }.into());
        }

        if ctx.state == crate::context::JobState::Stuck {
            // Attempt recovery
            self.context_manager
                .update_context(uuid, |ctx| ctx.attempt_recovery())
                .await?
                .map_err(|s| crate::error::JobError::ContextError {
                    id: uuid,
                    reason: s,
                })?;

            // Reschedule
            self.scheduler.schedule(uuid).await?;

            Ok(format!(
                "Job {} was stuck. Attempting recovery (attempt #{}).",
                job_id,
                ctx.repair_attempts + 1
            ))
        } else {
            Ok(format!(
                "Job {} is not stuck (current state: {:?}). No help needed.",
                job_id, ctx.state
            ))
        }
    }

    /// Trigger a manual heartbeat check.
    pub(super) async fn process_heartbeat(&self) -> Result<SubmissionResult, Error> {
        let Some(workspace) = self.workspace() else {
            return Ok(SubmissionResult::error(
                "Heartbeat requires a workspace (database must be connected).",
            ));
        };

        let runner = crate::agent::HeartbeatRunner::new(
            crate::agent::HeartbeatConfig::default(),
            workspace.clone(),
            self.llm().clone(),
        );

        match runner.check_heartbeat().await {
            crate::agent::HeartbeatResult::Ok => Ok(SubmissionResult::ok_with_message(
                "Heartbeat: all clear, nothing needs attention.",
            )),
            crate::agent::HeartbeatResult::NeedsAttention(msg) => Ok(SubmissionResult::response(
                format!("Heartbeat findings:\n\n{}", msg),
            )),
            crate::agent::HeartbeatResult::Skipped => Ok(SubmissionResult::ok_with_message(
                "Heartbeat skipped: no HEARTBEAT.md checklist found in workspace.",
            )),
            crate::agent::HeartbeatResult::Failed(err) => Ok(SubmissionResult::error(format!(
                "Heartbeat failed: {}",
                err
            ))),
        }
    }

    /// Summarize the current thread's conversation.
    pub(super) async fn process_summarize(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let messages = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.messages()
        };

        if messages.is_empty() {
            return Ok(SubmissionResult::ok_with_message(
                "Nothing to summarize (empty thread).",
            ));
        }

        // Build a summary prompt with the conversation
        let mut context = Vec::new();
        context.push(ChatMessage::system(
            "Summarize the conversation so far in 3-5 concise bullet points. \
             Focus on decisions made, actions taken, and key outcomes. \
             Be brief and factual.",
        ));
        // Include the conversation messages (truncate to last 20 to avoid context overflow)
        let start = if messages.len() > 20 {
            messages.len() - 20
        } else {
            0
        };
        context.extend_from_slice(&messages[start..]);
        context.push(ChatMessage::user("Summarize this conversation."));

        let request = crate::llm::CompletionRequest::new(context)
            .with_max_tokens(512)
            .with_temperature(0.3);

        match self.llm().complete(request).await {
            Ok(response) => Ok(SubmissionResult::response(format!(
                "Thread Summary:\n\n{}",
                response.content.trim()
            ))),
            Err(e) => Ok(SubmissionResult::error(format!("Summarize failed: {}", e))),
        }
    }

    /// Suggest next steps based on the current thread.
    pub(super) async fn process_suggest(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let messages = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.messages()
        };

        if messages.is_empty() {
            return Ok(SubmissionResult::ok_with_message(
                "Nothing to suggest from (empty thread).",
            ));
        }

        let mut context = Vec::new();
        context.push(ChatMessage::system(
            "Based on the conversation so far, suggest 2-4 concrete next steps the user could take. \
             Be actionable and specific. Format as a numbered list.",
        ));
        let start = if messages.len() > 20 {
            messages.len() - 20
        } else {
            0
        };
        context.extend_from_slice(&messages[start..]);
        context.push(ChatMessage::user("What should I do next?"));

        let request = crate::llm::CompletionRequest::new(context)
            .with_max_tokens(512)
            .with_temperature(0.5);

        match self.llm().complete(request).await {
            Ok(response) => Ok(SubmissionResult::response(format!(
                "Suggested Next Steps:\n\n{}",
                response.content.trim()
            ))),
            Err(e) => Ok(SubmissionResult::error(format!("Suggest failed: {}", e))),
        }
    }

    async fn load_copytrading_profile_for_user(
        &self,
        user_id: &str,
    ) -> CopyTradingInitializationProfile {
        let settings = self.load_settings_for_user(user_id).await;
        let copy = settings.copytrading;
        CopyTradingInitializationProfile {
            max_allocation_usd: Decimal::from(copy.max_allocation_usd),
            per_trade_notional_cap_usd: Decimal::from(copy.per_trade_notional_cap_usd),
            max_leverage: Decimal::from(copy.max_leverage),
            symbol_allowlist: copy.symbol_allowlist,
            symbol_denylist: copy.symbol_denylist,
            max_slippage_bps: copy.max_slippage_bps,
            information_sharing_scope: parse_information_sharing_scope(
                &copy.information_sharing_scope,
            ),
        }
    }

    async fn load_settings_for_user(&self, user_id: &str) -> Settings {
        if let Some(store) = self.store()
            && let Ok(map) = store.get_all_settings(user_id).await
        {
            return Settings::from_db_map(&map);
        }
        Settings::default()
    }

    async fn render_connector_status_for_user(&self, user_id: &str) -> String {
        let settings = self.load_settings_for_user(user_id).await;
        let native = probe_hyperliquid_native(&settings).await;
        let bridge = probe_bridge_service().await;

        format!(
            "## Connectors\n\n\
             | Connector | State | Detail |\n\
             | --- | --- | --- |\n\
             | Hyperliquid native API | {} | {} |\n\
             | Bridge service | {} | {} |",
            native.state.as_str(),
            native.detail,
            bridge.state.as_str(),
            bridge.detail
        )
    }

    async fn render_positions_status_for_user(&self, user_id: &str) -> Result<String, Error> {
        let settings = self.load_settings_for_user(user_id).await;
        let runtime = &settings.hyperliquid_runtime;
        let copy = &settings.copytrading;
        let connector_status = self.render_connector_status_for_user(user_id).await;

        let (receipt_count, last_receipt_at) = if let Some(store) = self.store() {
            let records = store.list_intent_audit_records(user_id, 50).await?;
            let last = records.first().map(|r| r.created_at);
            (records.len(), last)
        } else {
            (0usize, None)
        };

        let mut lines = vec![
            "## Positions".to_string(),
            String::new(),
            "| Field | Value |".to_string(),
            "| --- | --- |".to_string(),
            format!("| Network | {} |", runtime.network),
            format!(
                "| Execution policy | {} |",
                runtime.paper_live_policy.replace('_', " ")
            ),
            "| Data feed | Pending live position adapter |".to_string(),
            format!("| Mirrored receipts (last 50) | {} |", receipt_count),
            format!(
                "| Last mirrored receipt | {} |",
                last_receipt_at
                    .map(|ts| ts.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "none".to_string())
            ),
            String::new(),
            "### Guardrails".to_string(),
            format!("- Allocation cap: {} USD", copy.max_allocation_usd),
            format!(
                "- Per-trade notional cap: {} USD",
                copy.per_trade_notional_cap_usd
            ),
            format!("- Leverage cap: {}x", copy.max_leverage),
            format!("- Slippage cap: {} bps", copy.max_slippage_bps),
            format!(
                "- Symbol allowlist: {}",
                if copy.symbol_allowlist.is_empty() {
                    "none".to_string()
                } else {
                    copy.symbol_allowlist.join(", ")
                }
            ),
            format!(
                "- Symbol denylist: {}",
                if copy.symbol_denylist.is_empty() {
                    "none".to_string()
                } else {
                    copy.symbol_denylist.join(", ")
                }
            ),
        ];

        if self.store().is_none() {
            lines.push(String::new());
            lines.push("Telemetry note: database store is not connected.".to_string());
        }
        lines.push(String::new());
        lines.push(connector_status);

        Ok(lines.join("\n"))
    }

    async fn render_vault_status_for_user(&self, user_id: &str) -> Result<String, Error> {
        let settings = self.load_settings_for_user(user_id).await;
        let wallet = &settings.wallet_vault_policy;
        let verification = &settings.verification_backend;
        let connector_status = self.render_connector_status_for_user(user_id).await;

        let (receipt_count, last_receipt_at) = if let Some(store) = self.store() {
            let records = store.list_intent_audit_records(user_id, 50).await?;
            let last = records.first().map(|r| r.created_at);
            (records.len(), last)
        } else {
            (0usize, None)
        };

        Ok(format!(
            "## Vault\n\n\
             | Field | Value |\n\
             | --- | --- |\n\
             | Custody mode | {} |\n\
             | Operator wallet | {} |\n\
             | User wallet | {} |\n\
             | Vault address | {} |\n\
             | Kill switch | {} ({}) |\n\
             | Verification backend | {} |\n\
             | Signed fallback receipts | {} |\n\
             | Mirrored receipts (last 50) | {} |\n\
             | Last mirrored receipt | {} |\n\n\
             Telemetry note: live vault telemetry adapter is pending.\n\n\
             {}",
            wallet.custody_mode,
            mask_hex_address(wallet.operator_wallet_address.as_deref()),
            mask_hex_address(wallet.user_wallet_address.as_deref()),
            mask_hex_address(wallet.vault_address.as_deref()),
            if wallet.kill_switch_enabled {
                "enabled"
            } else {
                "disabled"
            },
            wallet.kill_switch_behavior,
            verification.backend,
            if verification.fallback_require_signed_receipts {
                "required"
            } else {
                "optional"
            },
            receipt_count,
            last_receipt_at
                .map(|ts| ts.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "none".to_string()),
            connector_status,
        ))
    }

    async fn render_risk_status_for_user(&self, user_id: &str) -> Result<String, Error> {
        let settings = self.load_settings_for_user(user_id).await;
        let runtime = &settings.hyperliquid_runtime;
        let wallet = &settings.wallet_vault_policy;
        let copy = &settings.copytrading;
        let connector_status = self.render_connector_status_for_user(user_id).await;

        let (mirror_count, mirror_pnl_usd, fee_usd) = if let Some(store) = self.store() {
            let records = store.list_intent_audit_records(user_id, 100).await?;
            let mut pnl_total = Decimal::ZERO;
            let mut fee_total = Decimal::ZERO;
            for record in &records {
                if let Some(ref pnl) = record.mirrored_pnl_usd
                    && let Ok(value) = Decimal::from_str(pnl)
                {
                    pnl_total += value;
                }
                if let Some(ref fee) = record.revenue_share_fee_usd
                    && let Ok(value) = Decimal::from_str(fee)
                {
                    fee_total += value;
                }
            }
            (records.len(), pnl_total.normalize(), fee_total.normalize())
        } else {
            (0usize, Decimal::ZERO, Decimal::ZERO)
        };

        Ok(format!(
            "## Risk\n\n\
             | Control | Value |\n\
             | --- | --- |\n\
             | Network | {} |\n\
             | Paper/live policy | {} |\n\
             | Request timeout | {} ms |\n\
             | Max retries | {} |\n\
             | Retry backoff | {} ms |\n\
             | Max position size | {} USD |\n\
             | Leverage cap | {}x |\n\
             | Kill switch | {} ({}) |\n\
             | Copy allocation cap | {} USD |\n\
             | Copy per-trade cap | {} USD |\n\
             | Copy slippage cap | {} bps |\n\
             | Copy sharing scope | {} |\n\
             | Mirrored executions | {} |\n\
             | Mirrored PnL | {} USD |\n\
             | Revenue-share fees | {} USD |\n\n\
             Telemetry note: live risk feed integration is pending; policy checks are active.\n\n\
             {}",
            runtime.network,
            runtime.paper_live_policy,
            runtime.timeout_ms,
            runtime.max_retries,
            runtime.retry_backoff_ms,
            wallet.max_position_size_usd,
            wallet.leverage_cap,
            if wallet.kill_switch_enabled {
                "enabled"
            } else {
                "disabled"
            },
            wallet.kill_switch_behavior,
            copy.max_allocation_usd,
            copy.per_trade_notional_cap_usd,
            copy.max_slippage_bps,
            copy.information_sharing_scope,
            mirror_count,
            mirror_pnl_usd,
            fee_usd,
            connector_status,
        ))
    }

    async fn summarize_copytrade_audit_for_user(&self, user_id: &str) -> Result<String, Error> {
        let Some(store) = self.store() else {
            return Ok("Copytrade status unavailable: store not configured.".to_string());
        };

        let records = store.list_intent_audit_records(user_id, 100).await?;
        if records.is_empty() {
            return Ok("No mirrored execution receipts recorded yet.".to_string());
        }

        let mut pnl_total = Decimal::ZERO;
        let mut fee_total = Decimal::ZERO;
        let mut attribution_rows = 0usize;
        let mut complete_lineage = 0usize;

        for record in &records {
            if let Some(ref pnl) = record.mirrored_pnl_usd
                && let Ok(value) = Decimal::from_str(pnl)
            {
                pnl_total += value;
            }
            if let Some(ref fee) = record.revenue_share_fee_usd
                && let Ok(value) = Decimal::from_str(fee)
            {
                fee_total += value;
            }
            attribution_rows += record.provider_attributions.len();
            if record.signal_hash.is_some()
                && record.verification_hash.is_some()
                && record.settlement_hash.is_some()
            {
                complete_lineage += 1;
            }
        }

        Ok(format!(
            "Mirrored executions: {}\nComplete proof lineage: {}\nProvider attributions: {}\nMirrored PnL (USD): {}\nRevenue share fees (USD): {}",
            records.len(),
            complete_lineage,
            attribution_rows,
            pnl_total.normalize(),
            fee_total.normalize()
        ))
    }

    async fn execute_vault_strategy_for_user(
        &self,
        user_id: &str,
        args: &[String],
    ) -> Result<SubmissionResult, Error> {
        let request = match parse_vault_run_args(args) {
            Ok(request) => request,
            Err(error) => return Ok(SubmissionResult::error(error)),
        };

        let settings = self.load_settings_for_user(user_id).await;
        if let Some(error) = testnet_guard_error(
            &settings.hyperliquid_runtime.network,
            false,
            "Vault strategy execution",
        ) {
            return Ok(SubmissionResult::error(error));
        }

        let leverage = request.leverage.unwrap_or(Decimal::ONE);
        let max_notional = Decimal::from(settings.wallet_vault_policy.max_position_size_usd);
        if request.notional > max_notional {
            return Ok(SubmissionResult::error(format!(
                "notional_usd {} exceeds wallet_vault_policy.max_position_size_usd {}",
                request.notional.normalize(),
                max_notional.normalize()
            )));
        }
        let max_leverage = Decimal::from(settings.wallet_vault_policy.leverage_cap);
        if leverage > max_leverage {
            return Ok(SubmissionResult::error(format!(
                "leverage {} exceeds wallet_vault_policy.leverage_cap {}",
                leverage.normalize(),
                max_leverage.normalize()
            )));
        }
        if settings.wallet_vault_policy.vault_address.is_none() {
            return Ok(SubmissionResult::error(
                "wallet_vault_policy.vault_address must be configured before /vault run"
                    .to_string(),
            ));
        }

        let copy_profile = self.load_copytrading_profile_for_user(user_id).await;
        let policy_text = format!(
            "Require wallet attestation and signal hash. Max trade notional {}. Max leverage {}x. Max slippage {} bps.",
            copy_profile.per_trade_notional_cap_usd.normalize(),
            copy_profile.max_leverage.normalize(),
            copy_profile.max_slippage_bps
        );
        let compiled_policy = match compile_copytrade_policy(&policy_text, &copy_profile) {
            Ok(compiled) => compiled,
            Err(error) => {
                return Ok(SubmissionResult::error(format!(
                    "failed to compile deterministic policy: {}",
                    error
                )));
            }
        };

        let source_signal_hash = if let Some(hash) = request.source_signal_hash.clone() {
            hash
        } else {
            let seed = serde_json::json!({
                "strategy": request.strategy_name,
                "symbol": request.symbol,
                "side": request.side,
                "notional": request.notional.normalize().to_string(),
                "price_ref": request.price_ref.normalize().to_string(),
                "leverage": leverage.normalize().to_string(),
                "network": settings.hyperliquid_runtime.network,
                "vault_address": settings.wallet_vault_policy.vault_address,
            });
            match deterministic_hash(&seed) {
                Ok(hash) => hash,
                Err(error) => {
                    return Ok(SubmissionResult::error(format!(
                        "failed to compute source signal hash: {}",
                        error
                    )));
                }
            }
        };

        let market_context_hash = if let Some(hash) = request.market_context_hash.clone() {
            hash
        } else {
            let seed = serde_json::json!({
                "symbol": request.symbol,
                "side": request.side,
                "network": settings.hyperliquid_runtime.network,
                "runtime_endpoint": effective_hyperliquid_api_base_url(&settings),
                "generated_at": Utc::now().date_naive().to_string(),
            });
            match deterministic_hash(&seed) {
                Ok(hash) => hash,
                Err(error) => {
                    return Ok(SubmissionResult::error(format!(
                        "failed to compute market context hash: {}",
                        error
                    )));
                }
            }
        };

        let intent_id = Uuid::new_v4();
        let mut strategy = StrategyPayload::new();
        strategy.insert(
            "name".to_string(),
            serde_json::json!(request.strategy_name.clone()),
        );
        strategy.insert(
            "symbol".to_string(),
            serde_json::json!(request.symbol.clone()),
        );
        strategy.insert("side".to_string(), serde_json::json!(request.side.clone()));
        strategy.insert(
            "execution_mode".to_string(),
            serde_json::json!(request.mode.clone()),
        );
        strategy.insert("attested_execution".to_string(), serde_json::json!(true));
        strategy.insert(
            "verification_backend".to_string(),
            serde_json::json!(settings.verification_backend.backend.clone()),
        );

        let mut risk_limits = RiskLimitsPayload::new();
        risk_limits.insert(
            "max_position_size_usd".to_string(),
            serde_json::json!(settings.wallet_vault_policy.max_position_size_usd),
        );
        risk_limits.insert(
            "max_leverage".to_string(),
            serde_json::json!(settings.wallet_vault_policy.leverage_cap),
        );
        risk_limits.insert(
            "kill_switch_enabled".to_string(),
            serde_json::json!(settings.wallet_vault_policy.kill_switch_enabled),
        );
        risk_limits.insert(
            "kill_switch_behavior".to_string(),
            serde_json::json!(settings.wallet_vault_policy.kill_switch_behavior.clone()),
        );

        let intent_envelope = IntentEnvelope {
            intent_id,
            agent_id: request
                .agent_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "agent-main".to_string()),
            user_id: user_id.to_string(),
            strategy,
            risk_limits: risk_limits.clone(),
            market_context_hash: market_context_hash.clone(),
            created_at: Utc::now(),
        };
        if let Err(error) = intent_envelope.validate() {
            return Ok(SubmissionResult::error(format!(
                "intent validation failed: {}",
                error
            )));
        }

        let mut tool_context =
            JobContext::with_user(user_id, "vault_strategy_run", request.strategy_name.clone());
        tool_context.metadata = serde_json::json!({
            "hyperliquid_runtime": {
                "paper_live_policy": settings.hyperliquid_runtime.paper_live_policy,
            }
        });

        let mut profile_payload = BTreeMap::new();
        profile_payload.insert(
            "max_allocation_usd".to_string(),
            serde_json::json!(copy_profile.max_allocation_usd.normalize().to_string()),
        );
        profile_payload.insert(
            "per_trade_notional_cap_usd".to_string(),
            serde_json::json!(
                copy_profile
                    .per_trade_notional_cap_usd
                    .normalize()
                    .to_string()
            ),
        );
        profile_payload.insert(
            "max_leverage".to_string(),
            serde_json::json!(copy_profile.max_leverage.normalize().to_string()),
        );
        profile_payload.insert(
            "symbol_allowlist".to_string(),
            serde_json::json!(copy_profile.symbol_allowlist),
        );
        profile_payload.insert(
            "symbol_denylist".to_string(),
            serde_json::json!(copy_profile.symbol_denylist),
        );
        profile_payload.insert(
            "max_slippage_bps".to_string(),
            serde_json::json!(copy_profile.max_slippage_bps),
        );
        profile_payload.insert(
            "information_sharing_scope".to_string(),
            serde_json::json!(match copy_profile.information_sharing_scope {
                InformationSharingScope::None => "none",
                InformationSharingScope::SignalsOnly => "signals_only",
                InformationSharingScope::SignalsAndExecution => "signals_and_execution",
                InformationSharingScope::FullAudit => "full_audit",
            }),
        );

        let tool_params = serde_json::json!({
            "intent_id": intent_id.to_string(),
            "mode": request.mode,
            "live_policy_gate": request.live_policy_gate,
            "paper_live_policy": settings.hyperliquid_runtime.paper_live_policy,
            "trading_endpoint": effective_hyperliquid_api_base_url(&settings),
            "verification_endpoint": settings.verification_backend.eigencloud_endpoint,
            "symbol": request.symbol,
            "side": request.side,
            "notional": request.notional.normalize().to_string(),
            "price_ref": request.price_ref.normalize().to_string(),
            "leverage": leverage.normalize().to_string(),
            "market_context_hash": market_context_hash,
            "risk_limits": risk_limits,
            "source_signal_hash": source_signal_hash,
            "wallet_attestation_hash": request.wallet_attestation_hash,
            "natural_language_policy": policy_text,
            "copytrading_profile": profile_payload,
        });

        let Some(tool) = self.tools().get("hyperliquid_execute").await else {
            return Ok(SubmissionResult::error(
                "hyperliquid_execute tool is not registered".to_string(),
            ));
        };

        let tool_output = match tool.execute(tool_params, &tool_context).await {
            Ok(output) => output,
            Err(error) => {
                return Ok(SubmissionResult::error(format!(
                    "vault strategy execution failed: {}",
                    error
                )));
            }
        };

        let tool_receipt: HyperliquidToolReceipt =
            match serde_json::from_value(tool_output.result.clone()) {
                Ok(receipt) => receipt,
                Err(error) => {
                    return Ok(SubmissionResult::error(format!(
                        "failed to decode execution receipt: {}",
                        error
                    )));
                }
            };
        if !is_lower_hex_64(&tool_receipt.decision_hash) {
            return Ok(SubmissionResult::error(
                "execution receipt decision_hash is invalid".to_string(),
            ));
        }

        let execution_receipt = match map_tool_receipt_to_artifact(intent_id, &tool_receipt) {
            Ok(receipt) => receipt,
            Err(error) => return Ok(SubmissionResult::error(error)),
        };
        if let Err(error) = execution_receipt.validate() {
            return Ok(SubmissionResult::error(format!(
                "execution receipt validation failed: {}",
                error
            )));
        }

        let source_signal_hash = tool_receipt.source_signal_hash.clone().unwrap_or_default();
        if source_signal_hash.is_empty() || !is_lower_hex_64(&source_signal_hash) {
            return Ok(SubmissionResult::error(
                "source signal hash missing or invalid in execution receipt".to_string(),
            ));
        }

        let profile_hash = match deterministic_hash(&copy_profile) {
            Ok(hash) => hash,
            Err(error) => {
                return Ok(SubmissionResult::error(format!(
                    "failed to hash copytrading profile: {}",
                    error
                )));
            }
        };

        let execution_intent = A2aExecutionIntentContract {
            execution_intent_id: Uuid::new_v4(),
            intent_id,
            source_signal_hash: source_signal_hash.clone(),
            policy_hash: tool_receipt
                .policy_hash
                .clone()
                .unwrap_or_else(|| compiled_policy.policy_hash.clone()),
            profile_hash,
            wallet_attestation_hash: tool_receipt
                .wallet_attestation_hash
                .clone()
                .unwrap_or_default(),
            expected_notional: request.notional,
            expected_leverage: leverage,
            created_at: Utc::now(),
        };
        if let Err(error) = execution_intent.validate() {
            return Ok(SubmissionResult::error(format!(
                "execution intent contract validation failed: {}",
                error
            )));
        }

        let proof_lineage_hash = match build_copytrade_proof_lineage_hash(
            &source_signal_hash,
            &execution_intent,
            &execution_receipt,
            None,
            None,
        ) {
            Ok(hash) => hash,
            Err(error) => {
                return Ok(SubmissionResult::error(format!(
                    "failed to build proof lineage hash: {}",
                    error
                )));
            }
        };

        let verification_backend = if settings
            .verification_backend
            .backend
            .eq_ignore_ascii_case("eigencloud_primary")
        {
            VerificationBackend::EigencloudPrimary
        } else {
            VerificationBackend::SignedFallback
        };
        let verification_provider =
            if verification_backend == VerificationBackend::EigencloudPrimary {
                VerificationJobProvider::EigenCloud
            } else {
                VerificationJobProvider::EigenCompute
            };

        let auth_scheme = if settings
            .verification_backend
            .eigencloud_auth_scheme
            .eq_ignore_ascii_case("api_key")
        {
            "api_key".to_string()
        } else {
            "bearer".to_string()
        };
        let credentials = VerificationJobCredentialRef {
            provider: verification_provider,
            secret_name: "eigencloud_auth_token".to_string(),
            auth_scheme,
        };
        let verification_endpoint =
            if verification_backend == VerificationBackend::EigencloudPrimary {
                settings
                    .verification_backend
                    .eigencloud_endpoint
                    .clone()
                    .unwrap_or_else(|| "https://verify-sepolia.eigencloud.xyz".to_string())
            } else {
                settings
                    .verification_backend
                    .fallback_chain_path
                    .as_ref()
                    .map(|path| format!("file://{}", path.display()))
                    .unwrap_or_else(|| "fallback://receipt-chain".to_string())
            };
        let verification_job = match create_eigen_verification_job(
            verification_provider,
            verification_endpoint.clone(),
            proof_lineage_hash.clone(),
            credentials,
        ) {
            Ok(job) => job,
            Err(error) => {
                return Ok(SubmissionResult::error(format!(
                    "failed to create verification job: {}",
                    error
                )));
            }
        };

        let verification_record = VerificationRecord::new(
            execution_receipt.receipt_id,
            verification_backend,
            format!("{}#job={}", verification_endpoint, verification_job.job_id),
            VerificationStatus::Pending,
        );

        let mut audit_record = match IntentAuditRecord::from_artifacts(
            request.agent_id,
            user_id.to_string(),
            &intent_envelope,
            &execution_receipt,
            Some(&verification_record),
        ) {
            Ok(record) => record,
            Err(error) => {
                return Ok(SubmissionResult::error(format!(
                    "failed to build audit record: {}",
                    error
                )));
            }
        };

        audit_record = match audit_record.with_copytrade_lineage(
            Some(source_signal_hash.clone()),
            None,
            Vec::new(),
        ) {
            Ok(record) => record,
            Err(error) => {
                return Ok(SubmissionResult::error(format!(
                    "failed to attach proof lineage: {}",
                    error
                )));
            }
        };

        let Some(store) = self.store() else {
            return Ok(SubmissionResult::error(
                "intent audit store is not configured".to_string(),
            ));
        };
        if let Err(error) = store.persist_intent_audit_record(&audit_record).await {
            return Ok(SubmissionResult::error(format!(
                "failed to persist audit record: {}",
                error
            )));
        }

        let mut verification_workspace_path: Option<String> = None;
        if let Some(workspace) = self.workspace() {
            if let Err(error) = workspace.persist_audit_record(&audit_record).await {
                return Ok(SubmissionResult::error(format!(
                    "failed to persist workspace audit record: {}",
                    error
                )));
            }
            let verification_doc_path =
                format!("audits/verifications/{}.json", verification_job.job_id);
            let verification_doc = serde_json::json!({
                "intent_id": intent_envelope.intent_id,
                "receipt_id": execution_receipt.receipt_id,
                "tool_receipt_id": tool_receipt.receipt_id,
                "verification_record_id": verification_record.verification_id,
                "verification_backend": settings.verification_backend.backend,
                "verification_endpoint": verification_endpoint,
                "verification_job": verification_job,
                "proof_lineage_hash": proof_lineage_hash,
                "source_signal_hash": source_signal_hash,
                "created_at": Utc::now(),
            });
            let serialized = serde_json::to_string_pretty(&verification_doc)
                .unwrap_or_else(|_| "{}".to_string());
            if let Err(error) = workspace.write(&verification_doc_path, &serialized).await {
                return Ok(SubmissionResult::error(format!(
                    "failed to persist verification workspace doc: {}",
                    error
                )));
            }
            verification_workspace_path = Some(verification_doc_path);
        }

        let response = serde_json::json!({
            "status": "queued",
            "network": settings.hyperliquid_runtime.network,
            "intent_id": intent_envelope.intent_id,
            "tool_receipt_id": tool_receipt.receipt_id,
            "receipt_id": execution_receipt.receipt_id,
            "decision_hash": tool_receipt.decision_hash,
            "policy_hash": tool_receipt.policy_hash,
            "source_signal_hash": source_signal_hash,
            "proof_lineage_hash": proof_lineage_hash,
            "verification_record_id": verification_record.verification_id,
            "verification_status": verification_record.status.as_str(),
            "verification_job_id": verification_job.job_id,
            "verification_workspace_path": verification_workspace_path,
            "audit_workspace_path": audit_record.workspace_document_path(),
            "chain_hash": audit_record.chain_hash,
        });

        Ok(SubmissionResult::response(format!(
            "Attested vault strategy run accepted.\n```json\n{}\n```",
            render_json_block(&response)
        )))
    }

    async fn verify_receipt_for_user(
        &self,
        user_id: &str,
        lookup: &str,
    ) -> Result<SubmissionResult, Error> {
        let Some(store) = self.store() else {
            return Ok(SubmissionResult::error(
                "Receipt verification is unavailable: store is not configured.".to_string(),
            ));
        };

        let records = store.list_intent_audit_records(user_id, 200).await?;
        if records.is_empty() {
            return Ok(SubmissionResult::response(
                "No receipt records found for this user.".to_string(),
            ));
        }

        let record = if lookup.eq_ignore_ascii_case("latest") {
            records.first()
        } else if let Ok(uuid) = Uuid::parse_str(lookup) {
            records.iter().find(|record| {
                record.receipt_id == uuid
                    || record.intent_id == uuid
                    || record.verification_id == Some(uuid)
            })
        } else {
            records
                .iter()
                .find(|record| record.chain_hash.eq_ignore_ascii_case(lookup))
        };

        let Some(record) = record else {
            return Ok(SubmissionResult::error(format!(
                "No receipt record matched '{}'. Try /verify latest or /receipts all.",
                lookup
            )));
        };

        let verification_status = record
            .verification_status
            .map(|status| status.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let payload = serde_json::json!({
            "intent_id": record.intent_id,
            "receipt_id": record.receipt_id,
            "verification_id": record.verification_id,
            "verification_status": verification_status,
            "signal_hash": record.signal_hash,
            "intent_hash": record.intent_hash,
            "receipt_hash": record.receipt_hash,
            "verification_hash": record.verification_hash,
            "settlement_hash": record.settlement_hash,
            "chain_hash": record.chain_hash,
            "workspace_path": record.workspace_path,
            "created_at": record.created_at,
        });

        Ok(SubmissionResult::response(format!(
            "Verification record:\n```json\n{}\n```",
            render_json_block(&payload)
        )))
    }

    async fn list_receipts_for_user(
        &self,
        user_id: &str,
        selector: &str,
    ) -> Result<SubmissionResult, Error> {
        let Some(store) = self.store() else {
            return Ok(SubmissionResult::error(
                "Receipt listing is unavailable: store is not configured.".to_string(),
            ));
        };

        let records = store.list_intent_audit_records(user_id, 200).await?;
        if records.is_empty() {
            return Ok(SubmissionResult::response(
                "No receipt records found for this user.".to_string(),
            ));
        }

        let filtered: Vec<&IntentAuditRecord> = if selector.eq_ignore_ascii_case("latest") {
            records.first().into_iter().collect()
        } else if selector.eq_ignore_ascii_case("all") || selector == "*" {
            records.iter().collect()
        } else if let Ok(agent_id) = Uuid::parse_str(selector) {
            records
                .iter()
                .filter(|record| record.agent_id == Some(agent_id))
                .collect()
        } else {
            return Ok(SubmissionResult::error(
                "Usage: /receipts <agent-id|all|latest>".to_string(),
            ));
        };

        if filtered.is_empty() {
            return Ok(SubmissionResult::response(format!(
                "No receipts matched selector '{}'.",
                selector
            )));
        }

        let mut lines = Vec::new();
        lines.push("| intent_id | receipt_id | verification | chain |".to_string());
        lines.push("| --- | --- | --- | --- |".to_string());

        for record in filtered.iter().take(25) {
            let verification_status = record
                .verification_status
                .map(|status| status.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            lines.push(format!(
                "| {} | {} | {} | {} |",
                record.intent_id,
                record.receipt_id,
                verification_status,
                short_hash(&record.chain_hash)
            ));
        }

        Ok(SubmissionResult::response(format!(
            "Receipts ({} shown):\n{}",
            lines.len().saturating_sub(2),
            lines.join("\n")
        )))
    }

    async fn persist_settings_for_user(
        &self,
        user_id: &str,
        settings: &Settings,
    ) -> Result<(), Error> {
        let Some(store) = self.store() else {
            return Err(Error::Config(ConfigError::InvalidValue {
                key: "settings.store".to_string(),
                message: "Settings store is not configured.".to_string(),
            }));
        };

        let map = settings.to_db_map();
        store.set_all_settings(user_id, &map).await?;
        Ok(())
    }

    async fn bridge_request_json(
        &self,
        method: reqwest::Method,
        path: &str,
        payload: Value,
    ) -> Result<Value, String> {
        let base_url = effective_bridge_base_url().ok_or_else(|| {
            "Bridge service is not configured. Set ENCLAGENT_BRIDGE_URL (or ENCLAGENT_BRIDGE_BASE_URL)."
                .to_string()
        })?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| format!("Bridge HTTP client init failed: {}", error))?;

        let url = format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let mut request = client.request(method, &url).json(&payload);
        if let Some(key) = effective_bridge_api_key() {
            request = request
                .header("x-api-key", &key)
                .header("authorization", format!("Bearer {}", key));
        }

        let response = request
            .send()
            .await
            .map_err(|error| format!("Bridge request failed: {}", error))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|error| format!("Bridge response read failed: {}", error))?;
        let parsed_json = serde_json::from_slice::<Value>(&body).ok();
        let body_text = String::from_utf8_lossy(&body).trim().to_string();

        if !status.is_success() {
            let detail = parsed_json
                .as_ref()
                .and_then(|value| value.get("error"))
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
                .or_else(|| {
                    if body_text.is_empty() {
                        None
                    } else {
                        Some(body_text.clone())
                    }
                })
                .unwrap_or_else(|| "Unknown bridge error".to_string());
            return Err(format!(
                "Bridge request to {} returned HTTP {}: {}",
                path,
                status.as_u16(),
                detail
            ));
        }

        if let Some(json) = parsed_json {
            return Ok(json);
        }

        Ok(serde_json::json!({
            "raw": body_text,
            "status": status.as_u16(),
        }))
    }

    async fn bridge_post_json(&self, path: &str, payload: Value) -> Result<Value, String> {
        self.bridge_request_json(reqwest::Method::POST, path, payload)
            .await
    }

    async fn bridge_patch_json(&self, path: &str, payload: Value) -> Result<Value, String> {
        self.bridge_request_json(reqwest::Method::PATCH, path, payload)
            .await
    }

    async fn render_funding_status_for_user(&self, user_id: &str) -> Result<String, Error> {
        let settings = self.load_settings_for_user(user_id).await;
        let runtime = &settings.hyperliquid_runtime;
        let wallet = &settings.wallet_vault_policy;
        let connector_status = self.render_connector_status_for_user(user_id).await;
        let bridge_status = match self
            .bridge_post_json("/api/fund", serde_json::json!({ "action": "status" }))
            .await
        {
            Ok(response) => render_json_block(&response),
            Err(error) => format!("bridge status unavailable: {}", error),
        };

        Ok(format!(
            "## Funding\n\n\
             | Field | Value |\n\
             | --- | --- |\n\
             | Runtime network | {} |\n\
             | Operator wallet | {} |\n\
             | User wallet | {} |\n\
             | Bridge status | `{}` |\n\n\
             Bridge `/api/fund` status:\n\
             ```json\n\
             {}\n\
             ```\n\n\
             Commands:\n\
             - `/funding provision <agent-id> <usd> [--no-auto-activate]`\n\
             - `/funding fund <agent-id> <usd> [--no-auto-activate]`\n\
             - `/funding agent-balance <agent-id> [--include-pnl]`\n\
             - `/funding operator-balance`\n\
             - `{}  # allow mainnet override for mutating actions`\n\n\
             {}",
            runtime.network,
            mask_hex_address(wallet.operator_wallet_address.as_deref()),
            mask_hex_address(wallet.user_wallet_address.as_deref()),
            if bridge_status.starts_with("bridge status unavailable") {
                "degraded"
            } else {
                "healthy"
            },
            bridge_status,
            MAINNET_OVERRIDE_FLAG,
            connector_status
        ))
    }

    async fn handle_funding_command(
        &self,
        user_id: &str,
        args: &[String],
    ) -> Result<SubmissionResult, Error> {
        let allow_mainnet = has_flag(args, MAINNET_OVERRIDE_FLAG);
        let without_mainnet = strip_flag(args, MAINNET_OVERRIDE_FLAG);
        if without_mainnet.is_empty() || without_mainnet[0].eq_ignore_ascii_case("status") {
            return Ok(SubmissionResult::response(
                self.render_funding_status_for_user(user_id).await?,
            ));
        }

        let subcommand = without_mainnet[0].to_ascii_lowercase();
        let settings = self.load_settings_for_user(user_id).await;

        match subcommand.as_str() {
            "provision" | "fund" => {
                if let Some(error) = testnet_guard_error(
                    &settings.hyperliquid_runtime.network,
                    allow_mainnet,
                    "Funding mutation",
                ) {
                    return Ok(SubmissionResult::error(error));
                }

                let clean_args = strip_flag(&without_mainnet, AUTO_ACTIVATE_DISABLE_FLAG);
                if clean_args.len() < 3 {
                    return Ok(SubmissionResult::error(
                        "Usage: /funding <provision|fund> <agent-id> <usd> [--no-auto-activate] [--allow-mainnet]",
                    ));
                }

                let agent_id = clean_args[1].trim();
                if agent_id.is_empty() {
                    return Ok(SubmissionResult::error("agent-id cannot be empty."));
                }
                let amount = match parse_positive_usd_amount(&clean_args[2]) {
                    Ok(value) => value,
                    Err(error) => return Ok(SubmissionResult::error(error)),
                };
                let auto_activate = !has_flag(args, AUTO_ACTIVATE_DISABLE_FLAG);

                let payload = serde_json::json!({
                    "action": subcommand,
                    "agentId": agent_id,
                    "amount": amount,
                    "autoActivate": auto_activate,
                });
                let response = match self.bridge_post_json("/api/fund", payload).await {
                    Ok(value) => value,
                    Err(error) => return Ok(SubmissionResult::error(error)),
                };

                Ok(SubmissionResult::response(format!(
                    "Funding action `{}` completed.\n```json\n{}\n```",
                    subcommand,
                    render_json_block(&response)
                )))
            }
            "agent-balance" => {
                let include_pnl = has_flag(&without_mainnet, INCLUDE_PNL_FLAG);
                let clean_args = strip_flag(&without_mainnet, INCLUDE_PNL_FLAG);
                if clean_args.len() < 2 {
                    return Ok(SubmissionResult::error(
                        "Usage: /funding agent-balance <agent-id> [--include-pnl]",
                    ));
                }
                let agent_id = clean_args[1].trim();
                if agent_id.is_empty() {
                    return Ok(SubmissionResult::error("agent-id cannot be empty."));
                }

                let payload = serde_json::json!({
                    "action": "agent-balance",
                    "agentId": agent_id,
                    "includePnl": include_pnl,
                });
                let response = match self.bridge_post_json("/api/fund", payload).await {
                    Ok(value) => value,
                    Err(error) => return Ok(SubmissionResult::error(error)),
                };

                Ok(SubmissionResult::response(format!(
                    "Agent balance response:\n```json\n{}\n```",
                    render_json_block(&response)
                )))
            }
            "operator-balance" => {
                let response = match self
                    .bridge_post_json(
                        "/api/fund",
                        serde_json::json!({
                            "action": "operator-balance",
                        }),
                    )
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(SubmissionResult::error(error)),
                };

                Ok(SubmissionResult::response(format!(
                    "Operator balance response:\n```json\n{}\n```",
                    render_json_block(&response)
                )))
            }
            _ => Ok(SubmissionResult::error(
                "Usage: /funding [status|provision|fund|agent-balance|operator-balance] ...",
            )),
        }
    }

    async fn handle_vault_command(
        &self,
        user_id: &str,
        args: &[String],
    ) -> Result<SubmissionResult, Error> {
        let allow_mainnet = has_flag(args, MAINNET_OVERRIDE_FLAG);
        let without_mainnet = strip_flag(args, MAINNET_OVERRIDE_FLAG);
        if without_mainnet.is_empty() || without_mainnet[0].eq_ignore_ascii_case("status") {
            return Ok(SubmissionResult::response(
                self.render_vault_status_for_user(user_id).await?,
            ));
        }

        let subcommand = without_mainnet[0].to_ascii_lowercase();
        let settings = self.load_settings_for_user(user_id).await;

        match subcommand.as_str() {
            "run" | "execute" => {
                self.execute_vault_strategy_for_user(user_id, &without_mainnet)
                    .await
            }
            "create" | "open" => {
                if let Some(error) = testnet_guard_error(
                    &settings.hyperliquid_runtime.network,
                    allow_mainnet,
                    "Vault creation",
                ) {
                    return Ok(SubmissionResult::error(error));
                }
                if without_mainnet.len() < 2 {
                    return Ok(SubmissionResult::error(
                        "Usage: /vault create <agent-id> [--allow-mainnet]",
                    ));
                }
                let agent_id = without_mainnet[1].trim();
                if agent_id.is_empty() {
                    return Ok(SubmissionResult::error("agent-id cannot be empty."));
                }

                let response = match self
                    .bridge_patch_json(
                        &format!("/api/agents/{}", agent_id),
                        serde_json::json!({
                            "vaultSocial": {
                                "isOpenVault": true,
                                "agentPostsTrades": true,
                                "allowDiscussion": true,
                                "agentRespondsToQuestions": true
                            }
                        }),
                    )
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(SubmissionResult::error(error)),
                };

                Ok(SubmissionResult::response(format!(
                    "Vault create/open action completed for agent `{}`.\n```json\n{}\n```",
                    agent_id,
                    render_json_block(&response)
                )))
            }
            "deposit" | "withdraw" => {
                if let Some(error) = testnet_guard_error(
                    &settings.hyperliquid_runtime.network,
                    allow_mainnet,
                    "Vault fund mutation",
                ) {
                    return Ok(SubmissionResult::error(error));
                }
                if without_mainnet.len() < 2 {
                    return Ok(SubmissionResult::error(
                        "Usage: /vault <deposit|withdraw> <usd> [vault-address] [--allow-mainnet]",
                    ));
                }

                let amount = match parse_positive_usd_amount(&without_mainnet[1]) {
                    Ok(value) => value,
                    Err(error) => return Ok(SubmissionResult::error(error)),
                };
                let vault_address = if without_mainnet.len() >= 3 {
                    without_mainnet[2].trim().to_string()
                } else {
                    settings
                        .wallet_vault_policy
                        .vault_address
                        .clone()
                        .unwrap_or_default()
                };
                if vault_address.is_empty() {
                    return Ok(SubmissionResult::error(
                        "Vault address is required. Provide it directly or configure wallet_vault_policy.vault_address.",
                    ));
                }
                if !is_hex_wallet_address(&vault_address) {
                    return Ok(SubmissionResult::error(
                        "Vault address must be a 0x-prefixed 40-hex address.",
                    ));
                }

                let response = match self
                    .bridge_post_json(
                        "/api/fund",
                        serde_json::json!({
                            "action": subcommand,
                            "vaultAddress": vault_address,
                            "amount": amount,
                        }),
                    )
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(SubmissionResult::error(error)),
                };

                Ok(SubmissionResult::response(format!(
                    "Vault `{}` action completed.\n```json\n{}\n```",
                    subcommand,
                    render_json_block(&response)
                )))
            }
            "set-address" => {
                if without_mainnet.len() < 2 {
                    return Ok(SubmissionResult::error("Usage: /vault set-address <0x...>"));
                }
                let address = without_mainnet[1].trim();
                if !is_hex_wallet_address(address) {
                    return Ok(SubmissionResult::error(
                        "Vault address must be a 0x-prefixed 40-hex address.",
                    ));
                }

                let mut updated = settings;
                updated
                    .set("wallet_vault_policy.vault_address", address)
                    .map_err(|message| {
                        Error::Config(ConfigError::InvalidValue {
                            key: "wallet_vault_policy.vault_address".to_string(),
                            message,
                        })
                    })?;
                self.persist_settings_for_user(user_id, &updated).await?;

                Ok(SubmissionResult::response(format!(
                    "Vault address updated: {}",
                    mask_hex_address(Some(address))
                )))
            }
            _ => Ok(SubmissionResult::error(
                "Usage: /vault [status|run|create|deposit|withdraw|set-address] ...",
            )),
        }
    }

    /// Handle system commands that bypass thread-state checks entirely.
    pub(super) async fn handle_system_command(
        &self,
        user_id: &str,
        command: &str,
        args: &[String],
    ) -> Result<SubmissionResult, Error> {
        let canonical_command = canonicalize_system_command_name(command);

        match canonical_command.as_str() {
            "help" => Ok(SubmissionResult::response(concat!(
                "System:\n",
                "  /help             Show this help\n",
                "  /model [name]     Show or switch the active model\n",
                "  /version          Show version info\n",
                "  /tools            List available tools\n",
                "  /debug            Toggle debug mode\n",
                "  /ping             Connectivity check\n",
                "\n",
                "Jobs:\n",
                "  /job <desc>       Create a new job\n",
                "  /status [id]      Check job status\n",
                "  /cancel <id>      Cancel a job\n",
                "  /list             List all jobs\n",
                "\n",
                "Session:\n",
                "  /undo             Undo last turn\n",
                "  /redo             Redo undone turn\n",
                "  /compact          Compress context window\n",
                "  /clear            Clear current thread\n",
                "  /interrupt        Stop current operation\n",
                "  /new              New conversation thread\n",
                "  /thread <id>      Switch to thread\n",
                "  /resume <id>      Resume from checkpoint\n",
                "\n",
                "Agent:\n",
                "  /heartbeat        Run heartbeat check\n",
                "  /summarize        Summarize current thread\n",
                "  /suggest          Suggest next steps\n",
                "\n",
                "Enclagent:\n",
                "  /positions        Show positions workflow\n",
                "  /exposure         Show exposure workflow\n",
                "  /funding ...      Funding actions (status/provision/fund/balance)\n",
                "  /vault ...        Vault actions (status/run/create/deposit/withdraw)\n",
                "  /risk             Show risk workflow\n",
                "  /pause-agent      Pause trading workflow\n",
                "  /resume-agent     Resume trading workflow\n",
                "  /verify <id>      Show verification/audit record (id or latest)\n",
                "  /receipts <id>    List receipts (agent-id, all, or latest)\n",
                "  /copy-policy ...  Compile deterministic copytrading checks\n",
                "  /copy-status      Show mirrored PnL/fee attribution summary\n",
                "  /connectors       Show Bridge + Hyperliquid connector health\n",
                "\n",
                "  /quit             Exit",
            ))),

            "ping" => Ok(SubmissionResult::response("pong!")),

            "version" => Ok(SubmissionResult::response(format!(
                "{} v{}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION")
            ))),

            "tools" => {
                let tools = self.tools().list().await;
                Ok(SubmissionResult::response(format!(
                    "Available tools: {}",
                    tools.join(", ")
                )))
            }

            "debug" => {
                // Debug toggle is handled client-side in the REPL.
                // For non-REPL channels, just acknowledge.
                Ok(SubmissionResult::ok_with_message(
                    "Debug toggle is handled by your client.",
                ))
            }

            "model" => {
                let current = self.llm().active_model_name();

                if args.is_empty() {
                    // Show current model and list available models
                    let mut out = format!("Active model: {}\n", current);
                    match self.llm().list_models().await {
                        Ok(models) if !models.is_empty() => {
                            out.push_str("\nAvailable models:\n");
                            for m in &models {
                                let marker = if *m == current { " (active)" } else { "" };
                                out.push_str(&format!("  {}{}\n", m, marker));
                            }
                            out.push_str("\nUse /model <name> to switch.");
                        }
                        Ok(_) => {
                            out.push_str(
                                "\nCould not fetch model list. Use /model <name> to switch.",
                            );
                        }
                        Err(e) => {
                            out.push_str(&format!(
                                "\nCould not fetch models: {}. Use /model <name> to switch.",
                                e
                            ));
                        }
                    }
                    Ok(SubmissionResult::response(out))
                } else {
                    let requested = &args[0];

                    // Validate the model exists
                    match self.llm().list_models().await {
                        Ok(models) if !models.is_empty() => {
                            if !models.iter().any(|m| m == requested) {
                                return Ok(SubmissionResult::error(format!(
                                    "Unknown model: {}. Available models:\n  {}",
                                    requested,
                                    models.join("\n  ")
                                )));
                            }
                        }
                        Ok(_) => {
                            // Empty model list, can't validate but try anyway
                        }
                        Err(e) => {
                            tracing::warn!("Could not fetch model list for validation: {}", e);
                        }
                    }

                    match self.llm().set_model(requested) {
                        Ok(()) => Ok(SubmissionResult::response(format!(
                            "Switched model to: {}",
                            requested
                        ))),
                        Err(e) => Ok(SubmissionResult::error(format!(
                            "Failed to switch model: {}",
                            e
                        ))),
                    }
                }
            }

            "positions" => Ok(SubmissionResult::response(
                self.render_positions_status_for_user(user_id).await?,
            )),

            "exposure" => Ok(SubmissionResult::response(
                "Exposure workflow is available. Live exposure aggregation is pending.",
            )),

            "funding" => self.handle_funding_command(user_id, args).await,

            "vault" => self.handle_vault_command(user_id, args).await,

            "risk" => Ok(SubmissionResult::response(
                self.render_risk_status_for_user(user_id).await?,
            )),

            "pause-agent" => Ok(SubmissionResult::ok_with_message(
                "Pause workflow accepted. Runtime pause controls are not wired yet.",
            )),

            "resume-agent" => Ok(SubmissionResult::ok_with_message(
                "Resume workflow accepted. Runtime resume controls are not wired yet.",
            )),

            "verify" => {
                let lookup = args.first().map(|value| value.as_str()).unwrap_or("latest");
                self.verify_receipt_for_user(user_id, lookup).await
            }

            "receipts" => {
                let selector = args.first().map(|value| value.as_str()).unwrap_or("latest");
                self.list_receipts_for_user(user_id, selector).await
            }

            "copy-policy" => {
                if args.is_empty() {
                    return Ok(SubmissionResult::error(
                        "Usage: /copy-policy <natural-language policy text>",
                    ));
                }

                let policy = args.join(" ");
                let profile = self.load_copytrading_profile_for_user(user_id).await;
                let compiled = compile_copytrade_policy(&policy, &profile).map_err(|e| {
                    Error::Config(ConfigError::InvalidValue {
                        key: "copytrading.policy".to_string(),
                        message: e.to_string(),
                    })
                })?;
                let rendered =
                    serde_json::to_string_pretty(&compiled).unwrap_or_else(|_| "{}".to_string());
                Ok(SubmissionResult::response(format!(
                    "Compiled copytrading policy:\n{}",
                    rendered
                )))
            }

            "copy-status" => Ok(SubmissionResult::response(
                self.summarize_copytrade_audit_for_user(user_id).await?,
            )),

            "connectors" => Ok(SubmissionResult::response(
                self.render_connector_status_for_user(user_id).await,
            )),

            _ => Ok(SubmissionResult::error(unknown_command_message(command))),
        }
    }

    /// Handle legacy command routing from the Router (job commands that go through
    /// process_user_input -> router -> handle_job_or_command -> here).
    pub(super) async fn handle_command(
        &self,
        user_id: &str,
        command: &str,
        args: &[String],
    ) -> Result<Option<String>, Error> {
        // System commands are now handled directly via Submission::SystemCommand,
        // but the router may still send us unknown /commands.
        match self.handle_system_command(user_id, command, args).await? {
            SubmissionResult::Response { content } => Ok(Some(content)),
            SubmissionResult::Ok { message } => Ok(message),
            SubmissionResult::Error { message } => Ok(Some(format!("Error: {}", message))),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canonicalize_system_command_name, command_suggestions, is_hex_wallet_address,
        is_lower_hex_64, levenshtein_distance, mask_hex_address, parse_positive_usd_amount,
        parse_vault_run_args, testnet_guard_error, unknown_command_message,
    };

    #[test]
    fn test_command_alias_canonicalization() {
        assert_eq!(canonicalize_system_command_name("/pos"), "positions");
        assert_eq!(
            canonicalize_system_command_name("pause_agent"),
            "pause-agent"
        );
        assert_eq!(
            canonicalize_system_command_name("/verify-receipt"),
            "verify"
        );
        assert_eq!(canonicalize_system_command_name("receipt"), "receipts");
    }

    #[test]
    fn test_levenshtein_distance_basic() {
        assert_eq!(levenshtein_distance("positions", "positions"), 0);
        assert_eq!(levenshtein_distance("positons", "positions"), 1);
        assert_eq!(levenshtein_distance("risk", "vault"), 5);
    }

    #[test]
    fn test_typo_suggestions_for_ws2_commands() {
        let suggestions = command_suggestions("positons");
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0], "positions");

        let suggestions = command_suggestions("pauze-agent");
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0], "pause-agent");
    }

    #[test]
    fn test_unknown_command_message_includes_help_and_candidates() {
        let message = unknown_command_message("receitps");
        assert!(message.contains("Unknown command: receitps."));
        assert!(message.contains("/receipts"));
        assert!(message.contains("Try /help"));

        let message = unknown_command_message("zzzzzzzzzz");
        assert!(message.contains("Try /help"));
    }

    #[test]
    fn test_mask_hex_address() {
        assert_eq!(mask_hex_address(None), "not configured");
        assert_eq!(
            mask_hex_address(Some("0x1234567890abcdef1234567890abcdef12345678")),
            "0x1234...5678"
        );
        assert_eq!(mask_hex_address(Some("abc")), "abc");
    }

    #[test]
    fn test_parse_positive_usd_amount() {
        assert_eq!(parse_positive_usd_amount("10").unwrap(), "10");
        assert_eq!(parse_positive_usd_amount("1.234500").unwrap(), "1.2345");
        assert!(parse_positive_usd_amount("0").is_err());
        assert!(parse_positive_usd_amount("-1").is_err());
        assert!(parse_positive_usd_amount("1.1234567").is_err());
    }

    #[test]
    fn test_is_hex_wallet_address() {
        assert!(is_hex_wallet_address(
            "0x1234567890abcdef1234567890abcdef12345678"
        ));
        assert!(is_hex_wallet_address(
            "0x1234567890ABCDEF1234567890abcdef12345678"
        ));
        assert!(!is_hex_wallet_address("0x1234"));
        assert!(!is_hex_wallet_address(
            "1234567890abcdef1234567890abcdef12345678"
        ));
    }

    #[test]
    fn test_testnet_guard_error() {
        assert!(testnet_guard_error("testnet", false, "op").is_none());
        assert!(testnet_guard_error("mainnet", true, "op").is_none());

        let err = testnet_guard_error("mainnet", false, "op").unwrap();
        assert!(err.contains("op is blocked"));
        assert!(err.contains("--allow-mainnet"));
    }

    #[test]
    fn test_is_lower_hex_64() {
        assert!(is_lower_hex_64(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_lower_hex_64(
            "0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_lower_hex_64("abcd"));
    }

    #[test]
    fn test_parse_vault_run_args_with_required_fields() {
        let args = vec![
            "run".to_string(),
            "BTC-USD".to_string(),
            "buy".to_string(),
            "250".to_string(),
            "50000".to_string(),
            "--attestation".to_string(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
        ];

        let parsed = parse_vault_run_args(&args).unwrap();
        assert_eq!(parsed.symbol, "BTC-USD");
        assert_eq!(parsed.side, "buy");
        assert_eq!(parsed.notional.normalize().to_string(), "250");
        assert_eq!(parsed.price_ref.normalize().to_string(), "50000");
        assert_eq!(parsed.mode, "paper");
        assert!(!parsed.live_policy_gate);
    }

    #[test]
    fn test_parse_vault_run_args_requires_attestation() {
        let args = vec![
            "run".to_string(),
            "BTC-USD".to_string(),
            "buy".to_string(),
            "250".to_string(),
            "50000".to_string(),
        ];

        let err = parse_vault_run_args(&args).unwrap_err();
        assert!(err.contains("attested execution requires --attestation"));
    }
}
