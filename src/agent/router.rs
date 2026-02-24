//! Message routing to appropriate handlers.
//!
//! The router handles explicit commands (starting with `/`).
//! Natural language intent classification is handled by `IntentClassifier`
//! which uses LLM + tools instead of brittle pattern matching.

use std::str::FromStr;

use regex::Regex;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::agent::intent::{
    ArtifactValidationError, CopyTradingInitializationProfile, InformationSharingScope,
    deterministic_hash,
};
use crate::channels::IncomingMessage;

/// Intent extracted from a message.
#[derive(Debug, Clone)]
pub enum MessageIntent {
    /// Create a new job.
    CreateJob {
        title: String,
        description: String,
        category: Option<String>,
    },
    /// Check status of a job.
    CheckJobStatus { job_id: Option<String> },
    /// Cancel a job.
    CancelJob { job_id: String },
    /// List jobs.
    ListJobs { filter: Option<String> },
    /// Help with a stuck job.
    HelpJob { job_id: String },
    /// General conversation/question.
    Chat { content: String },
    /// System command.
    Command { command: String, args: Vec<String> },
    /// Unknown intent.
    Unknown,
}

/// Deterministic runtime checks compiled from natural-language copytrading policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledCopyTradePolicy {
    pub normalized_policy: String,
    pub requires_wallet_attestation: bool,
    pub requires_signal_hash: bool,
    pub enforce_symbol_allowlist: bool,
    pub enforce_symbol_denylist: bool,
    pub max_notional_usd: Decimal,
    pub max_leverage: Decimal,
    pub max_slippage_bps: u32,
    pub allow_information_sharing: bool,
    pub policy_hash: String,
}

#[derive(Debug, Clone, Serialize)]
struct CompiledPolicySeed<'a> {
    normalized_policy: &'a str,
    requires_wallet_attestation: bool,
    requires_signal_hash: bool,
    enforce_symbol_allowlist: bool,
    enforce_symbol_denylist: bool,
    max_notional_usd: &'a Decimal,
    max_leverage: &'a Decimal,
    max_slippage_bps: u32,
    allow_information_sharing: bool,
}

fn normalize_policy_text(policy: &str) -> String {
    policy
        .split_whitespace()
        .map(|chunk| chunk.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_decimal(pattern: &str, text: &str) -> Option<Decimal> {
    let regex = Regex::new(pattern).ok()?;
    let captures = regex.captures(text)?;
    let raw = captures.get(1)?.as_str();
    Decimal::from_str(raw).ok()
}

fn extract_u32(pattern: &str, text: &str) -> Option<u32> {
    let regex = Regex::new(pattern).ok()?;
    let captures = regex.captures(text)?;
    captures.get(1)?.as_str().parse::<u32>().ok()
}

/// Compile natural-language copytrading controls into deterministic runtime checks.
pub fn compile_copytrade_policy(
    natural_language_policy: &str,
    profile: &CopyTradingInitializationProfile,
) -> Result<CompiledCopyTradePolicy, ArtifactValidationError> {
    profile.validate()?;
    if natural_language_policy.trim().is_empty() {
        return Err(ArtifactValidationError::EmptyField(
            "natural_language_policy",
        ));
    }

    let normalized = normalize_policy_text(natural_language_policy);

    let requires_wallet_attestation =
        !normalized.contains("skip attestation") && !normalized.contains("without attestation");
    let requires_signal_hash =
        !normalized.contains("unsigned signal") && !normalized.contains("without signal hash");
    let enforce_symbol_allowlist = !normalized.contains("allow any symbol");
    let enforce_symbol_denylist = !normalized.contains("ignore denylist");

    let max_notional_override = extract_decimal(
        r"(?:max|cap)\s+(?:trade\s+)?notional\s+\$?([0-9]+(?:\.[0-9]+)?)",
        &normalized,
    );
    let max_leverage_override =
        extract_decimal(r"(?:max\s+)?leverage\s+([0-9]+(?:\.[0-9]+)?)x", &normalized);
    let max_slippage_override =
        extract_u32(r"(?:max\s+)?slippage\s+([0-9]{1,4})\s*bps", &normalized);

    let max_notional_usd = max_notional_override
        .unwrap_or(profile.per_trade_notional_cap_usd)
        .min(profile.per_trade_notional_cap_usd);
    let max_leverage = max_leverage_override
        .unwrap_or(profile.max_leverage)
        .min(profile.max_leverage);
    let max_slippage_bps = max_slippage_override
        .unwrap_or(profile.max_slippage_bps)
        .min(profile.max_slippage_bps);

    let profile_allows_sharing = !matches!(
        profile.information_sharing_scope,
        InformationSharingScope::None
    );
    let allow_information_sharing =
        profile_allows_sharing && !normalized.contains("disable sharing");

    let seed = CompiledPolicySeed {
        normalized_policy: &normalized,
        requires_wallet_attestation,
        requires_signal_hash,
        enforce_symbol_allowlist,
        enforce_symbol_denylist,
        max_notional_usd: &max_notional_usd,
        max_leverage: &max_leverage,
        max_slippage_bps,
        allow_information_sharing,
    };
    let policy_hash = deterministic_hash(&seed)
        .map_err(|_| ArtifactValidationError::InvalidValue("natural_language_policy"))?;

    Ok(CompiledCopyTradePolicy {
        normalized_policy: normalized,
        requires_wallet_attestation,
        requires_signal_hash,
        enforce_symbol_allowlist,
        enforce_symbol_denylist,
        max_notional_usd,
        max_leverage,
        max_slippage_bps,
        allow_information_sharing,
        policy_hash,
    })
}

impl MessageIntent {
    /// Stable status label for WS-2 runtime status payloads.
    pub fn status_label(&self) -> String {
        match self {
            Self::CreateJob { .. } => "job.create".to_string(),
            Self::CheckJobStatus { .. } => "job.status".to_string(),
            Self::CancelJob { .. } => "job.cancel".to_string(),
            Self::ListJobs { .. } => "job.list".to_string(),
            Self::HelpJob { .. } => "job.help".to_string(),
            Self::Chat { .. } => "chat.message".to_string(),
            Self::Command { command, .. } => format!("command.{}", command),
            Self::Unknown => "intent.unknown".to_string(),
        }
    }

    /// Detect live-trading command intents without explicit operator gate.
    pub fn missing_live_policy_gate(&self) -> bool {
        match self {
            Self::Command { command, args } if command == "trade" || command == "execute" => {
                let requests_live = args
                    .iter()
                    .any(|arg| arg.eq_ignore_ascii_case("live") || arg == "--mode=live");
                let has_gate = args.iter().any(|arg| {
                    arg == "--live-policy-gate"
                        || arg == "--live-policy-gate=true"
                        || arg == "--confirm-live"
                });
                requests_live && !has_gate
            }
            _ => false,
        }
    }
}

/// Routes messages to appropriate handlers based on explicit commands.
///
/// For natural language messages, use `IntentClassifier` instead.
pub struct Router {
    /// Command prefix (e.g., "/" or "!")
    command_prefix: String,
}

impl Router {
    /// Create a new router.
    pub fn new() -> Self {
        Self {
            command_prefix: "/".to_string(),
        }
    }

    /// Set the command prefix.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.command_prefix = prefix.into();
        self
    }

    /// Check if a message is an explicit command.
    pub fn is_command(&self, message: &IncomingMessage) -> bool {
        message.content.trim().starts_with(&self.command_prefix)
    }

    /// Route an explicit command to determine its intent.
    ///
    /// Returns `None` if the message is not a command.
    /// For non-commands, use `IntentClassifier::classify()` instead.
    pub fn route_command(&self, message: &IncomingMessage) -> Option<MessageIntent> {
        let content = message.content.trim();

        if content.starts_with(&self.command_prefix) {
            Some(self.parse_command(content))
        } else {
            None
        }
    }

    fn parse_command(&self, content: &str) -> MessageIntent {
        let without_prefix = content
            .strip_prefix(&self.command_prefix)
            .unwrap_or(content);
        let parts: Vec<&str> = without_prefix.split_whitespace().collect();

        match parts.first().map(|s| s.to_lowercase()).as_deref() {
            Some("job") | Some("create") => {
                let rest = parts[1..].join(" ");
                MessageIntent::CreateJob {
                    title: rest.clone(),
                    description: rest,
                    category: None,
                }
            }
            Some("status") => {
                let job_id = parts.get(1).map(|s| s.to_string());
                MessageIntent::CheckJobStatus { job_id }
            }
            Some("cancel") => {
                if let Some(job_id) = parts.get(1) {
                    MessageIntent::CancelJob {
                        job_id: job_id.to_string(),
                    }
                } else {
                    MessageIntent::Unknown
                }
            }
            Some("list") | Some("jobs") => {
                let filter = parts.get(1).map(|s| s.to_string());
                MessageIntent::ListJobs { filter }
            }
            Some("help") => {
                if let Some(job_id) = parts.get(1) {
                    MessageIntent::HelpJob {
                        job_id: job_id.to_string(),
                    }
                } else {
                    MessageIntent::Command {
                        command: "help".to_string(),
                        args: vec![],
                    }
                }
            }
            Some(cmd) => MessageIntent::Command {
                command: cmd.to_string(),
                args: parts[1..].iter().map(|s| s.to_string()).collect(),
            },
            None => MessageIntent::Unknown,
        }
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_command_routing() {
        let router = Router::new();

        let msg = IncomingMessage::new("test", "user", "/status abc-123");
        let intent = router.route_command(&msg);

        assert!(matches!(intent, Some(MessageIntent::CheckJobStatus { .. })));
    }

    #[test]
    fn test_is_command() {
        let router = Router::new();

        let cmd_msg = IncomingMessage::new("test", "user", "/status");
        assert!(router.is_command(&cmd_msg));

        let chat_msg = IncomingMessage::new("test", "user", "Hello there");
        assert!(!router.is_command(&chat_msg));
    }

    #[test]
    fn test_non_command_returns_none() {
        let router = Router::new();

        // Natural language messages return None - they should use IntentClassifier
        let msg = IncomingMessage::new("test", "user", "Can you create a website for me?");
        assert!(router.route_command(&msg).is_none());

        let msg2 = IncomingMessage::new("test", "user", "Hello, how are you?");
        assert!(router.route_command(&msg2).is_none());
    }

    #[test]
    fn test_command_create_job() {
        let router = Router::new();

        let msg = IncomingMessage::new("test", "user", "/job build a website");
        let intent = router.route_command(&msg);

        match intent {
            Some(MessageIntent::CreateJob { title, .. }) => {
                assert_eq!(title, "build a website");
            }
            _ => panic!("Expected CreateJob intent"),
        }
    }

    #[test]
    fn test_command_list_jobs() {
        let router = Router::new();

        let msg = IncomingMessage::new("test", "user", "/list active");
        let intent = router.route_command(&msg);

        match intent {
            Some(MessageIntent::ListJobs { filter }) => {
                assert_eq!(filter, Some("active".to_string()));
            }
            _ => panic!("Expected ListJobs intent"),
        }
    }

    #[test]
    fn test_status_label() {
        let intent = MessageIntent::CheckJobStatus {
            job_id: Some("abc-123".to_string()),
        };
        assert_eq!(intent.status_label(), "job.status");

        let cmd = MessageIntent::Command {
            command: "tools".to_string(),
            args: vec![],
        };
        assert_eq!(cmd.status_label(), "command.tools");
    }

    #[test]
    fn compile_copytrade_policy_is_deterministic() {
        let profile = CopyTradingInitializationProfile {
            max_allocation_usd: dec!(10000),
            per_trade_notional_cap_usd: dec!(750),
            max_leverage: dec!(3),
            symbol_allowlist: vec!["BTC-USD".to_string()],
            symbol_denylist: vec!["DOGE-USD".to_string()],
            max_slippage_bps: 25,
            information_sharing_scope: InformationSharingScope::SignalsAndExecution,
        };

        let first = compile_copytrade_policy(
            "Only approved providers. max notional 500. max slippage 10 bps. max leverage 2x.",
            &profile,
        )
        .unwrap();
        let second = compile_copytrade_policy(
            "Only approved providers. max notional 500. max slippage 10 bps. max leverage 2x.",
            &profile,
        )
        .unwrap();

        assert_eq!(first.policy_hash, second.policy_hash);
        assert_eq!(first.max_notional_usd, dec!(500));
        assert_eq!(first.max_slippage_bps, 10);
        assert_eq!(first.max_leverage, dec!(2));
    }

    #[test]
    fn test_missing_live_policy_gate_detection() {
        let intent = MessageIntent::Command {
            command: "trade".to_string(),
            args: vec!["live".to_string()],
        };
        assert!(intent.missing_live_policy_gate());

        let gated = MessageIntent::Command {
            command: "trade".to_string(),
            args: vec!["live".to_string(), "--live-policy-gate=true".to_string()],
        };
        assert!(!gated.missing_live_policy_gate());
    }
}
