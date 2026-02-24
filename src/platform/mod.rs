//! Platform primitives for module governance and org tenancy.
//!
//! This layer is intentionally lightweight and runtime-native:
//! - Curated module catalog (Core-8)
//! - Module state defaults/merge helpers
//! - Org workspace + membership role helpers

use std::collections::{BTreeSet, HashMap};

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Capability descriptor exposed by a module manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleCapability {
    pub key: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
}

/// Module manifest for the curated platform catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleManifest {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub enabled_by_default: bool,
    pub optional_addon: bool,
    pub capabilities: Vec<ModuleCapability>,
}

/// Runtime module state for a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleState {
    pub module_id: String,
    pub enabled: bool,
    pub status: String,
    pub updated_at: String,
    #[serde(default)]
    pub config: serde_json::Value,
}

/// Organization workspace descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgWorkspace {
    pub id: String,
    pub name: String,
    pub enclave_id: String,
    pub plan: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Organization membership record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgMembership {
    pub member_id: String,
    pub role: String,
    pub status: String,
    pub invited_at: String,
    pub updated_at: String,
}

/// Inference-routing decision emitted by the module-aware router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRouteDecision {
    pub layer: String,
    pub module_id: String,
    pub confidence: f64,
    pub rationale: String,
}

/// Result of intent routing after module-state enforcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRouteResolution {
    pub requested_module_id: String,
    pub decision: InferenceRouteDecision,
    pub allowed: bool,
    pub reason: String,
}

/// Result of capability-based module policy enforcement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityGuardResolution {
    pub allowed: bool,
    pub required_capabilities: Vec<String>,
    pub blocked_capabilities: Vec<String>,
    pub reason: String,
}

/// Settings key used to persist module state.
pub const PLATFORM_MODULE_STATE_KEY: &str = "platform.modules.state";

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn capability(key: &str, description: &str, required: bool) -> ModuleCapability {
    ModuleCapability {
        key: key.to_string(),
        description: description.to_string(),
        required,
    }
}

fn module_manifest(
    id: &str,
    name: &str,
    category: &str,
    description: &str,
    enabled_by_default: bool,
    optional_addon: bool,
    capabilities: Vec<ModuleCapability>,
) -> ModuleManifest {
    ModuleManifest {
        id: id.to_string(),
        name: name.to_string(),
        category: category.to_string(),
        description: description.to_string(),
        enabled_by_default,
        optional_addon,
        capabilities,
    }
}

/// Curated Core-8 module catalog for v1 stable.
pub fn curated_module_catalog() -> Vec<ModuleManifest> {
    vec![
        module_manifest(
            "general",
            "General Assistant",
            "core",
            "Cross-domain baseline assistant for everyday tasks.",
            true,
            false,
            vec![
                capability("chat", "General conversational interface", true),
                capability("memory", "Workspace memory read/write", true),
                capability(
                    "verification_lineage",
                    "Intent/receipt verification capture",
                    true,
                ),
            ],
        ),
        module_manifest(
            "developer",
            "Developer Workflows",
            "core",
            "Coding, repository workflows, and technical documentation support.",
            true,
            false,
            vec![
                capability("code_generation", "Generate and refactor source code", true),
                capability("repo_ops", "Repository analysis and maintenance", true),
                capability("tooling", "Build/test-oriented task planning", true),
            ],
        ),
        module_manifest(
            "creative",
            "Creative Studio",
            "core",
            "Content, narrative, and creative ideation workflows.",
            true,
            false,
            vec![
                capability(
                    "content_ideation",
                    "Concept and creative brief generation",
                    true,
                ),
                capability("voice_tone", "Audience and style adaptation", false),
            ],
        ),
        module_manifest(
            "research",
            "Research Analyst",
            "core",
            "Structured synthesis, evidence framing, and research workflows.",
            true,
            false,
            vec![
                capability(
                    "structured_synthesis",
                    "Convert inputs into structured summaries",
                    true,
                ),
                capability("evidence_mapping", "Track assumptions and confidence", true),
            ],
        ),
        module_manifest(
            "business_ops",
            "Business Operations",
            "core",
            "Planning, docs, sheets/calendar, and operating rhythm workflows.",
            true,
            false,
            vec![
                capability("planning_ops", "Plans, reports, and checklists", true),
                capability("workspace_docs", "Document-centric operations", true),
            ],
        ),
        module_manifest(
            "communications",
            "Communications",
            "core",
            "Stakeholder updates, messages, and communication artifacts.",
            true,
            false,
            vec![
                capability(
                    "draft_messages",
                    "Draft internal/external communications",
                    true,
                ),
                capability(
                    "audience_adaptation",
                    "Tone/format adaptation by audience",
                    false,
                ),
            ],
        ),
        module_manifest(
            "hyperliquid_addon",
            "Hyperliquid Addon",
            "addon",
            "Optional trading and execution capabilities for Hyperliquid workflows.",
            false,
            true,
            vec![
                capability(
                    "hyperliquid_execute",
                    "Hyperliquid execution interface",
                    true,
                ),
                capability("risk_controls", "Trading risk control envelope", true),
            ],
        ),
        module_manifest(
            "eigenda_addon",
            "EigenDA Addon",
            "addon",
            "Optional data-availability commitment layer for verifiable artifacts.",
            false,
            true,
            vec![
                capability("artifact_commitment", "Commit artifacts to DA layer", true),
                capability("da_pointer", "Return DA pointer metadata for audits", true),
            ],
        ),
    ]
}

/// Default module state vector in catalog order.
pub fn default_module_states() -> Vec<ModuleState> {
    let now = now_rfc3339();
    curated_module_catalog()
        .into_iter()
        .map(|manifest| ModuleState {
            module_id: manifest.id.clone(),
            enabled: manifest.enabled_by_default,
            status: if manifest.enabled_by_default {
                "enabled".to_string()
            } else {
                "disabled".to_string()
            },
            updated_at: now.clone(),
            config: serde_json::json!({}),
        })
        .collect()
}

/// Merge persisted module state into the curated catalog and return normalized state.
///
/// Unknown module IDs in persisted data are discarded; missing catalog modules are
/// populated from defaults.
pub fn merge_module_states(persisted: Vec<ModuleState>) -> Vec<ModuleState> {
    let mut persisted_map: HashMap<String, ModuleState> = HashMap::new();
    for item in persisted {
        persisted_map.insert(item.module_id.clone(), item);
    }

    let now = now_rfc3339();
    curated_module_catalog()
        .into_iter()
        .map(|manifest| {
            if let Some(item) = persisted_map.remove(&manifest.id) {
                ModuleState {
                    module_id: manifest.id,
                    enabled: item.enabled,
                    status: if item.enabled {
                        "enabled".to_string()
                    } else {
                        "disabled".to_string()
                    },
                    updated_at: item.updated_at,
                    config: item.config,
                }
            } else {
                ModuleState {
                    module_id: manifest.id,
                    enabled: manifest.enabled_by_default,
                    status: if manifest.enabled_by_default {
                        "enabled".to_string()
                    } else {
                        "disabled".to_string()
                    },
                    updated_at: now.clone(),
                    config: serde_json::json!({}),
                }
            }
        })
        .collect()
}

/// Return true if the module ID is part of the curated catalog.
pub fn module_exists(module_id: &str) -> bool {
    curated_module_catalog().iter().any(|m| m.id == module_id)
}

/// Lookup a module manifest by ID.
pub fn module_manifest_by_id(module_id: &str) -> Option<ModuleManifest> {
    curated_module_catalog()
        .into_iter()
        .find(|m| m.id == module_id)
}

/// Return true if a module is an optional addon module.
pub fn module_is_optional_addon(module_id: &str) -> bool {
    module_manifest_by_id(module_id)
        .map(|manifest| manifest.optional_addon)
        .unwrap_or(false)
}

/// Return true if the given module is enabled in runtime state.
pub fn module_is_enabled(states: &[ModuleState], module_id: &str) -> bool {
    states
        .iter()
        .find(|state| state.module_id == module_id)
        .map(|state| state.enabled)
        .unwrap_or(false)
}

/// Return capability keys for a module ID.
pub fn module_capability_keys(module_id: &str) -> Vec<String> {
    module_manifest_by_id(module_id)
        .map(|manifest| {
            manifest
                .capabilities
                .into_iter()
                .map(|capability| capability.key)
                .collect()
        })
        .unwrap_or_default()
}

/// Return capability requirements for a command.
pub fn command_required_capabilities(command: &str) -> &'static [&'static str] {
    const HYPERLIQUID_CAPS: &[&str] = &["hyperliquid_execute"];

    let normalized = command
        .trim()
        .trim_start_matches('/')
        .replace('_', "-")
        .to_ascii_lowercase();
    match normalized.as_str() {
        "positions" | "position" | "pos" | "exposure" | "expo" | "funding" | "fund" | "funds"
        | "vault" | "risk" | "pause-agent" | "pauseagent" | "agent-pause" | "resume-agent"
        | "resumeagent" | "agent-resume" | "copy-policy" | "copy-status" | "connectors" => {
            HYPERLIQUID_CAPS
        }
        _ => &[],
    }
}

/// Return capability requirements for a tool.
pub fn tool_required_capabilities(tool_name: &str) -> &'static [&'static str] {
    const HYPERLIQUID_CAPS: &[&str] = &["hyperliquid_execute"];
    const EIGENDA_CAPS: &[&str] = &["artifact_commitment"];

    let normalized = tool_name.trim().to_ascii_lowercase();
    if normalized == "hyperliquid_execute" || normalized.starts_with("hyperliquid_") {
        return HYPERLIQUID_CAPS;
    }
    if normalized.starts_with("eigenda_") {
        return EIGENDA_CAPS;
    }
    &[]
}

fn module_ids_for_capability(capability: &str) -> Vec<String> {
    curated_module_catalog()
        .into_iter()
        .filter(|module| module.capabilities.iter().any(|cap| cap.key == capability))
        .map(|module| module.id)
        .collect()
}

/// Resolve capability requirements against current module state.
///
/// This is the execution-layer policy gate for commands and tools. Every
/// required capability must be backed by at least one enabled module.
pub fn resolve_capability_guard(
    required_capabilities: &[&str],
    states: &[ModuleState],
) -> CapabilityGuardResolution {
    let required: Vec<String> = required_capabilities
        .iter()
        .map(|cap| cap.trim())
        .filter(|cap| !cap.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|cap| cap.to_string())
        .collect();

    if required.is_empty() {
        return CapabilityGuardResolution {
            allowed: true,
            required_capabilities: Vec::new(),
            blocked_capabilities: Vec::new(),
            reason: "No module capability requirements.".to_string(),
        };
    }

    let mut blocked_capabilities = Vec::new();
    let mut reason_segments = Vec::new();
    for capability in &required {
        let module_ids = module_ids_for_capability(capability);
        if module_ids.is_empty() {
            blocked_capabilities.push(capability.clone());
            reason_segments.push(format!(
                "Capability '{}' is not mapped to any module manifest.",
                capability
            ));
            continue;
        }

        let allowed = module_ids
            .iter()
            .any(|module_id| module_is_enabled(states, module_id));
        if !allowed {
            blocked_capabilities.push(capability.clone());
            reason_segments.push(format!(
                "Capability '{}' requires enabled module(s): {}.",
                capability,
                module_ids.join(", ")
            ));
        }
    }

    if blocked_capabilities.is_empty() {
        return CapabilityGuardResolution {
            allowed: true,
            required_capabilities: required,
            blocked_capabilities,
            reason: "All required module capabilities are enabled.".to_string(),
        };
    }

    CapabilityGuardResolution {
        allowed: false,
        required_capabilities: required,
        blocked_capabilities,
        reason: reason_segments.join(" "),
    }
}

fn contains_any_lower(haystack: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| haystack.contains(pattern))
}

/// Layer-2 intent/domain router that maps user input into a module decision.
pub fn infer_route_decision(input: &str) -> InferenceRouteDecision {
    let lower = input.to_ascii_lowercase();

    let (module_id, confidence, rationale) = if contains_any_lower(
        &lower,
        &[
            "hyperliquid",
            "/vault",
            "/funding",
            "/positions",
            "/risk",
            "/copy-policy",
            "/copy-status",
            "copytrade",
            "copy trade",
            "perpetual",
            "order book",
            "leverage",
            "liquidation",
            "funding rate",
        ],
    ) {
        (
            "hyperliquid_addon",
            0.93,
            "Matched trading or Hyperliquid intent markers.",
        )
    } else if contains_any_lower(
        &lower,
        &[
            "eigenda",
            "data availability",
            "da commitment",
            "blob commitment",
            "availability layer",
        ],
    ) {
        (
            "eigenda_addon",
            0.91,
            "Matched data-availability commitment intent markers.",
        )
    } else if contains_any_lower(
        &lower,
        &[
            "code",
            "repo",
            "pull request",
            "debug",
            "refactor",
            "compile",
            "clippy",
            "rust",
            "python",
            "typescript",
            "unit test",
            "integration test",
            "stack trace",
            "api",
        ],
    ) {
        (
            "developer",
            0.82,
            "Matched software development workflow markers.",
        )
    } else if contains_any_lower(
        &lower,
        &[
            "story",
            "poem",
            "script",
            "creative",
            "branding",
            "tagline",
            "design concept",
            "ad copy",
            "moodboard",
        ],
    ) {
        (
            "creative",
            0.8,
            "Matched creative ideation or content markers.",
        )
    } else if contains_any_lower(
        &lower,
        &[
            "research",
            "analyze",
            "analysis",
            "evidence",
            "sources",
            "benchmark",
            "compare",
            "whitepaper",
            "summarize",
        ],
    ) {
        (
            "research",
            0.81,
            "Matched synthesis, evidence, or research markers.",
        )
    } else if contains_any_lower(
        &lower,
        &[
            "roadmap",
            "okr",
            "kpi",
            "sprint",
            "backlog",
            "runbook",
            "operating plan",
            "project plan",
            "process",
        ],
    ) {
        (
            "business_ops",
            0.79,
            "Matched planning and business-operations markers.",
        )
    } else if contains_any_lower(
        &lower,
        &[
            "email",
            "message",
            "announcement",
            "status update",
            "stakeholder update",
            "memo",
            "press release",
            "reply",
            "draft",
        ],
    ) {
        (
            "communications",
            0.78,
            "Matched communication drafting markers.",
        )
    } else {
        (
            "general",
            0.55,
            "No domain-specific markers matched; using general baseline.",
        )
    };

    InferenceRouteDecision {
        layer: "layer2_intent_domain_router".to_string(),
        module_id: module_id.to_string(),
        confidence,
        rationale: rationale.to_string(),
    }
}

/// Resolve an intent route against module state, enforcing disabled-module policy.
///
/// Policy:
/// - If the requested module is enabled, allow it.
/// - If a disabled module is an addon, block.
/// - If a disabled module is core and `general` is enabled, fallback to `general`.
/// - Otherwise block.
pub fn resolve_inference_route(input: &str, states: &[ModuleState]) -> InferenceRouteResolution {
    let mut decision = infer_route_decision(input);
    let requested_module_id = decision.module_id.clone();

    if module_is_enabled(states, &requested_module_id) {
        return InferenceRouteResolution {
            requested_module_id,
            decision,
            allowed: true,
            reason: "Requested module is enabled.".to_string(),
        };
    }

    if module_is_optional_addon(&requested_module_id) {
        return InferenceRouteResolution {
            requested_module_id: requested_module_id.clone(),
            decision,
            allowed: false,
            reason: format!("Optional addon '{requested_module_id}' is disabled."),
        };
    }

    if module_is_enabled(states, "general") {
        let prior = decision.rationale.clone();
        decision.module_id = "general".to_string();
        decision.confidence = (decision.confidence * 0.5).max(0.35);
        decision.rationale =
            format!("{prior} Requested module disabled; falling back to general module.");
        return InferenceRouteResolution {
            requested_module_id,
            decision,
            allowed: true,
            reason: "Requested module disabled; general fallback applied.".to_string(),
        };
    }

    InferenceRouteResolution {
        requested_module_id,
        decision,
        allowed: false,
        reason: "Requested module disabled and general fallback is unavailable.".to_string(),
    }
}

/// Build default org workspace for a user.
pub fn default_org_workspace(user_id: &str) -> OrgWorkspace {
    let now = now_rfc3339();
    let slug = slugify(user_id);
    OrgWorkspace {
        id: format!("org_{slug}"),
        name: "Primary Workspace".to_string(),
        enclave_id: format!("enclave_{slug}"),
        plan: "closed_beta".to_string(),
        created_at: now.clone(),
        updated_at: now,
    }
}

/// Build default org memberships for a user (self as owner).
pub fn default_org_memberships(user_id: &str) -> Vec<OrgMembership> {
    let now = now_rfc3339();
    vec![OrgMembership {
        member_id: user_id.to_string(),
        role: "owner".to_string(),
        status: "active".to_string(),
        invited_at: now.clone(),
        updated_at: now,
    }]
}

/// Normalize and validate role strings.
pub fn normalize_org_role(role: &str) -> Option<String> {
    match role.trim().to_ascii_lowercase().as_str() {
        "owner" => Some("owner".to_string()),
        "admin" => Some("admin".to_string()),
        "member" => Some("member".to_string()),
        _ => None,
    }
}

/// Role check for org-management operations.
pub fn can_manage_org(role: &str) -> bool {
    matches!(role, "owner" | "admin")
}

/// Role check for module-management operations.
pub fn can_manage_modules(role: &str) -> bool {
    matches!(role, "owner" | "admin")
}

fn slugify(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_catalog_contains_core_8() {
        let catalog = curated_module_catalog();
        assert_eq!(catalog.len(), 8);
        assert!(catalog.iter().any(|m| m.id == "general"));
        assert!(catalog.iter().any(|m| m.id == "hyperliquid_addon"));
        assert!(catalog.iter().any(|m| m.id == "eigenda_addon"));
    }

    #[test]
    fn defaults_disable_addons() {
        let states = default_module_states();
        let hl = states
            .iter()
            .find(|m| m.module_id == "hyperliquid_addon")
            .expect("hyperliquid addon state");
        let eigenda = states
            .iter()
            .find(|m| m.module_id == "eigenda_addon")
            .expect("eigenda addon state");
        assert!(!hl.enabled);
        assert!(!eigenda.enabled);
    }

    #[test]
    fn role_normalization_accepts_known_roles() {
        assert_eq!(normalize_org_role("owner").as_deref(), Some("owner"));
        assert_eq!(normalize_org_role("admin").as_deref(), Some("admin"));
        assert_eq!(normalize_org_role("member").as_deref(), Some("member"));
        assert!(normalize_org_role("unknown").is_none());
    }

    #[test]
    fn route_infers_hyperliquid_addon_from_trading_intent() {
        let decision = infer_route_decision("run /vault strategy with leverage 3");
        assert_eq!(decision.module_id, "hyperliquid_addon");
        assert!(decision.confidence >= 0.9);
    }

    #[test]
    fn resolve_route_blocks_disabled_addon() {
        let states = default_module_states();
        let resolved = resolve_inference_route("check hyperliquid funding rate", &states);
        assert_eq!(resolved.requested_module_id, "hyperliquid_addon");
        assert!(!resolved.allowed);
        assert!(resolved.reason.contains("disabled"));
    }

    #[test]
    fn resolve_route_falls_back_to_general_when_core_module_disabled() {
        let mut states = default_module_states();
        if let Some(dev) = states.iter_mut().find(|s| s.module_id == "developer") {
            dev.enabled = false;
            dev.status = "disabled".to_string();
        }
        let resolved = resolve_inference_route("debug this rust compile error", &states);
        assert!(resolved.allowed);
        assert_eq!(resolved.requested_module_id, "developer");
        assert_eq!(resolved.decision.module_id, "general");
    }

    #[test]
    fn command_requirements_cover_trading_commands() {
        assert_eq!(
            command_required_capabilities("vault"),
            &["hyperliquid_execute"]
        );
        assert_eq!(
            command_required_capabilities("/copy-policy"),
            &["hyperliquid_execute"]
        );
        assert!(command_required_capabilities("help").is_empty());
    }

    #[test]
    fn tool_requirements_detect_addon_scopes() {
        assert_eq!(
            tool_required_capabilities("hyperliquid_execute"),
            &["hyperliquid_execute"]
        );
        assert_eq!(
            tool_required_capabilities("eigenda_commit"),
            &["artifact_commitment"]
        );
        assert!(tool_required_capabilities("json").is_empty());
    }

    #[test]
    fn capability_guard_blocks_disabled_addon_capability() {
        let states = default_module_states();
        let guard = resolve_capability_guard(&["hyperliquid_execute"], &states);
        assert!(!guard.allowed);
        assert_eq!(guard.blocked_capabilities, vec!["hyperliquid_execute"]);
        assert!(guard.reason.contains("requires enabled module(s)"));
        assert!(guard.reason.contains("hyperliquid_addon"));
    }

    #[test]
    fn capability_guard_allows_enabled_addon_capability() {
        let mut states = default_module_states();
        if let Some(module) = states
            .iter_mut()
            .find(|module| module.module_id == "hyperliquid_addon")
        {
            module.enabled = true;
            module.status = "enabled".to_string();
        }

        let guard = resolve_capability_guard(&["hyperliquid_execute"], &states);
        assert!(guard.allowed);
        assert!(guard.blocked_capabilities.is_empty());
    }
}
