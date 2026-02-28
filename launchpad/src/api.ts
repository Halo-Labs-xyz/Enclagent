export async function fetchJson<T = unknown>(
  path: string,
  options?: { method?: string; body?: unknown; timeoutMs?: number }
): Promise<T> {
  const opts = options || {};
  const headers: Record<string, string> = {};
  let body: string | undefined;
  if (opts.body && typeof opts.body === "object") {
    headers["Content-Type"] = "application/json";
    body = JSON.stringify(opts.body);
  }
  const controller = new AbortController();
  let timeoutId: ReturnType<typeof setTimeout> | null = null;
  if ((opts.timeoutMs || 0) > 0) {
    timeoutId = setTimeout(() => controller.abort(), opts.timeoutMs);
  }
  let res: Response;
  try {
    res = await fetch(path, {
      method: opts.method || "GET",
      headers,
      body,
      signal: controller.signal,
    });
  } catch (error) {
    if ((error as { name?: string })?.name === "AbortError" && (opts.timeoutMs || 0) > 0) {
      throw new Error(`Request timed out (${opts.timeoutMs}ms)`);
    }
    throw error;
  } finally {
    if (timeoutId) clearTimeout(timeoutId);
  }
  const data = await res.json().catch(() => null);
  if (!res.ok) {
    const msg = (data?.error || data?.message || "Request failed") + " (" + res.status + ")";
    throw new Error(msg);
  }
  return data as T;
}

export interface FrontdoorTimelineEvent {
  seq_id: number;
  event_type: string;
  status: string;
  detail: string;
  actor: string;
  created_at: string;
}

interface FrontdoorTimelineResponse {
  session_id: string;
  events: FrontdoorTimelineEvent[];
}

export interface LaunchpadBlueprint {
  summary: string;
  mermaid: string;
  identityMarkdown: string;
  missionMarkdown: string;
  model: string;
  anthropicCompatibleSelected: boolean;
}

interface OpenAiModelRecord {
  id?: string;
}

interface OpenAiModelsResponse {
  data?: OpenAiModelRecord[];
}

interface OpenAiChoiceMessage {
  content?: string;
}

interface OpenAiChoice {
  message?: OpenAiChoiceMessage;
}

interface OpenAiChatResponse {
  model?: string;
  choices?: OpenAiChoice[];
}

interface LaunchpadBlueprintRequest {
  objective: string;
  profile_name: string;
  profile_domain: string;
  custody_mode: string;
  verification_backend: string;
  decision_title: string;
  decision_reason: string;
}

const MODEL_LIST_CACHE_TTL_MS = 5 * 60 * 1000;
const MODELS_TIMEOUT_MS = 1200;
const BLUEPRINT_TIMEOUT_MS = 4800;
const MAX_MERMAID_BODY_LINES = 18;

let cachedModelList: { models: string[]; expiresAt: number } | null = null;

function fallbackBlueprint(
  objective: string,
  profileName: string,
  profileDomain: string
): Omit<LaunchpadBlueprint, "model" | "anthropicCompatibleSelected"> {
  const safeObjective = objective.trim() || "Deliver deterministic, verifiable outcomes.";
  const safeProfileName = profileName || "enclagent_profile";
  const safeDomain = profileDomain || "general";
  return {
    summary: `Objective mapped to a deterministic setup plan for ${safeProfileName} (${safeDomain}).`,
    mermaid: `graph TD
  A[User Objective] --> B[Policy Synthesis]
  B --> C[Config Validation]
  C --> D[Identity and Signature]
  D --> E[Provisioning]
  E --> F[Runtime Verification]
  B --> G[Mission and Personality Seeding]`,
    identityMarkdown: `# IDENTITY

## Name
${safeProfileName}

## Personality
- Precise, deterministic, and audit-focused
- Security-first with explicit policy gates
- Clear error handling and operational discipline

## Operating Style
- Prefer verifiable outputs over speculative behavior
- Preserve signer and policy constraints at all times`,
    missionMarkdown: `# MISSION

## Core Mission
${safeObjective}

## Success Criteria
- Deterministic policy execution
- Verifiable receipts and evidence links
- Safe fallback behavior under degraded dependencies

## Constraints
- No live execution without explicit signer authorization
- Preserve privacy and module governance boundaries`,
  };
}

function sanitizeMermaidGraph(input: string): string {
  const raw = String(input || "").replace(/```mermaid|```/gi, "").trim();
  if (!raw) {
    return `graph TD
  A[User Objective] --> B[Policy Synthesis]
  B --> C[Config Validation]
  C --> D[Identity and Signature]
  D --> E[Provisioning]
  E --> F[Runtime Verification]
  B --> G[Mission and Personality Seeding]`;
  }
  if (/^(graph|flowchart)\s+TD\b/i.test(raw)) {
    const lines = raw
      .split(/\r?\n/)
      .map((line) => line.trimEnd())
      .filter((line) => line.trim().length > 0);
    const header = lines[0] || "graph TD";
    const body = lines.slice(1, 1 + MAX_MERMAID_BODY_LINES);
    if (body.length === 0) {
      return `graph TD\n  A[Launch Objective] --> B[Policy Synthesis]`;
    }
    return [header, ...body].join("\n");
  }
  if (/^(graph|flowchart)\s+/i.test(raw)) {
    return `graph TD\n  A[Launch Objective] --> B[Policy Synthesis]`;
  }
  return `graph TD\n  A[Launch Objective] --> B[${raw.replace(/\n+/g, " ").slice(0, 120)}]`;
}

function sanitizeMarkdownSeed(input: string, fallbackTitle: string, fallbackBody: string): string {
  const raw = String(input || "").trim();
  if (!raw) return `# ${fallbackTitle}\n\n${fallbackBody}`;
  if (/^#\s+/m.test(raw)) return raw;
  return `# ${fallbackTitle}\n\n${raw}`;
}

function parseFirstJsonObject(text: string): Record<string, unknown> | null {
  const raw = String(text || "").trim();
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
  } catch (_) {
    // continue
  }
  const fence = raw.match(/```json\s*([\s\S]*?)```/i);
  if (fence?.[1]) {
    try {
      const parsed = JSON.parse(fence[1].trim());
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return parsed as Record<string, unknown>;
      }
    } catch (_) {
      // continue
    }
  }
  const start = raw.indexOf("{");
  const end = raw.lastIndexOf("}");
  if (start >= 0 && end > start) {
    const slice = raw.slice(start, end + 1);
    try {
      const parsed = JSON.parse(slice);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return parsed as Record<string, unknown>;
      }
    } catch (_) {
      return null;
    }
  }
  return null;
}

function isAnthropicCompatibleModel(modelId: string): boolean {
  const id = modelId.toLowerCase();
  return (
    id.includes("claude") ||
    id.includes("anthropic") ||
    id.includes("minimax") ||
    id.includes("m2.5")
  );
}

function modelPreferenceScore(modelId: string): number {
  const id = modelId.toLowerCase();
  if (id.includes("minimax") && id.includes("m2.5")) return 100;
  if (id.includes("minimax")) return 95;
  if (id.includes("claude")) return 90;
  if (id.includes("anthropic")) return 80;
  if (id.includes("gpt")) return 20;
  return 10;
}

function pickPreferredModel(models: string[]): string | null {
  const clean = models.map((m) => String(m || "").trim()).filter(Boolean);
  if (clean.length === 0) return null;
  const sorted = [...clean].sort((a, b) => modelPreferenceScore(b) - modelPreferenceScore(a));
  return sorted[0] || null;
}

async function listGatewayModels(): Promise<string[]> {
  const now = Date.now();
  if (cachedModelList && cachedModelList.expiresAt > now) {
    return cachedModelList.models;
  }
  try {
    const payload = await fetchJson<OpenAiModelsResponse>("/v1/models", {
      timeoutMs: MODELS_TIMEOUT_MS,
    });
    const ids = (payload?.data || [])
      .map((entry) => String(entry?.id || "").trim())
      .filter(Boolean);
    if (ids.length > 0) {
      cachedModelList = {
        models: ids,
        expiresAt: now + MODEL_LIST_CACHE_TTL_MS,
      };
    }
    return ids;
  } catch (_) {
    if (cachedModelList?.models?.length) {
      return cachedModelList.models;
    }
    return [];
  }
}

function blueprintPrompt(params: LaunchpadBlueprintRequest): string {
  return [
    "Create a launchpad setup blueprint for an Enclagent frontdoor provisioning flow.",
    "Return strict JSON with keys: summary, mermaid, identity_markdown, mission_markdown.",
    "The mermaid value must be a valid `graph TD` flow with at least 6 nodes.",
    "The identity_markdown and mission_markdown should be high-quality seed docs.",
    `objective: ${params.objective}`,
    `profile_name: ${params.profile_name}`,
    `profile_domain: ${params.profile_domain}`,
    `custody_mode: ${params.custody_mode}`,
    `verification_backend: ${params.verification_backend}`,
    `runtime_decision_title: ${params.decision_title}`,
    `runtime_decision_reason: ${params.decision_reason}`,
  ].join("\n");
}

async function requestBlueprintFromModel(
  model: string,
  params: LaunchpadBlueprintRequest
): Promise<LaunchpadBlueprint | null> {
  const system =
    "You generate launchpad setup blueprints. Output valid JSON only with keys summary, mermaid, identity_markdown, mission_markdown.";
  const response = await fetchJson<OpenAiChatResponse>("/v1/chat/completions", {
    method: "POST",
    timeoutMs: BLUEPRINT_TIMEOUT_MS,
    body: {
      model,
      temperature: 0.25,
      max_tokens: 700,
      messages: [
        { role: "system", content: system },
        { role: "user", content: blueprintPrompt(params) },
      ],
    },
  });
  const content = String(response?.choices?.[0]?.message?.content || "").trim();
  const parsed = parseFirstJsonObject(content);
  if (!parsed) return null;

  const fallback = fallbackBlueprint(params.objective, params.profile_name, params.profile_domain);
  const summary = String(parsed.summary || fallback.summary).trim() || fallback.summary;
  const mermaid = sanitizeMermaidGraph(String(parsed.mermaid || fallback.mermaid));
  const identityMarkdown = sanitizeMarkdownSeed(
    String(parsed.identity_markdown || fallback.identityMarkdown),
    "IDENTITY",
    "Deterministic, security-first, verifiable agent profile."
  );
  const missionMarkdown = sanitizeMarkdownSeed(
    String(parsed.mission_markdown || fallback.missionMarkdown),
    "MISSION",
    params.objective || "Deliver deterministic outcomes."
  );
  const resolvedModel = String(response?.model || model || "unknown-model");
  return {
    summary,
    mermaid,
    identityMarkdown,
    missionMarkdown,
    model: resolvedModel,
    anthropicCompatibleSelected: isAnthropicCompatibleModel(resolvedModel),
  };
}

function parseActiveModelFromError(error: unknown): string | null {
  const message = String((error as { message?: string })?.message || error || "");
  const match = message.match(/active model is '([^']+)'/i);
  return match?.[1] ? String(match[1]).trim() : null;
}

export const api = {
  async suggestConfig(params: { wallet_address: string; intent: string; gateway_auth_key: string }) {
    return fetchJson<{ config?: Record<string, unknown>; assumptions?: string[]; warnings?: string[] }>(
      "/api/frontdoor/suggest-config",
      { method: "POST", body: params }
    );
  },
  async challenge(params: { wallet_address: string; privy_user_id: string | null; chain_id: number | null }) {
    return fetchJson<{ session_id?: string; message?: string }>("/api/frontdoor/challenge", {
      method: "POST",
      body: params,
    });
  },
  async verify(params: {
    session_id: string;
    wallet_address: string;
    privy_user_id: string | null;
    privy_identity_token: string | null;
    privy_access_token: string | null;
    message: string;
    signature: string;
    config: Record<string, unknown>;
  }) {
    return fetchJson("/api/frontdoor/verify", { method: "POST", body: params });
  },
  async getSession(sessionId: string) {
    return fetchJson<{
      session_id?: string;
      status?: string;
      provisioning_source?: string;
      runtime_state?: string;
      instance_url?: string;
      app_url?: string;
      verify_url?: string;
      error?: string;
      detail?: string;
    }>("/api/frontdoor/session/" + encodeURIComponent(sessionId));
  },
  async getSessionTimeline(sessionId: string) {
    return fetchJson<FrontdoorTimelineResponse>(
      "/api/frontdoor/session/" + encodeURIComponent(sessionId) + "/timeline"
    );
  },
  async getOnboardingState(sessionId: string) {
    return fetchJson<{ objective?: string; missing_fields?: string[]; current_step?: string; completed?: boolean }>(
      "/api/frontdoor/onboarding/state?session_id=" + encodeURIComponent(sessionId)
    );
  },
  async postOnboardingChat(sessionId: string, message: string) {
    const res = await fetchJson<{ state?: Record<string, unknown> }>("/api/frontdoor/onboarding/chat", {
      method: "POST",
      body: { session_id: sessionId, message },
    });
    return res?.state || {};
  },
  async generateLaunchpadBlueprint(params: {
    objective: string;
    profile_name: string;
    profile_domain: string;
    custody_mode: string;
    verification_backend: string;
    decision_title: string;
    decision_reason: string;
  }): Promise<LaunchpadBlueprint> {
    const fallback = fallbackBlueprint(params.objective, params.profile_name, params.profile_domain);
    const models = await listGatewayModels();
    const preferred = pickPreferredModel(models) || "";
    if (!preferred) {
      return {
        ...fallback,
        model: "fallback:no-llm-model",
        anthropicCompatibleSelected: false,
      };
    }
    try {
      const blueprint = await requestBlueprintFromModel(preferred, params);
      if (blueprint) return blueprint;
    } catch (error) {
      const active = parseActiveModelFromError(error);
      if (active && active !== preferred) {
        try {
          const blueprint = await requestBlueprintFromModel(active, params);
          if (blueprint) return blueprint;
        } catch (_) {
          // deterministic fallback below
        }
      }
    }
    return {
      ...fallback,
      model: "fallback:deterministic-launchpad",
      anthropicCompatibleSelected: false,
    };
  },
};
