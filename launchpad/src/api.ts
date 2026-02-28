export async function fetchJson<T = unknown>(
  path: string,
  options?: { method?: string; body?: unknown }
): Promise<T> {
  const opts = options || {};
  const headers: Record<string, string> = {};
  let body: string | undefined;
  if (opts.body && typeof opts.body === "object") {
    headers["Content-Type"] = "application/json";
    body = JSON.stringify(opts.body);
  }
  const res = await fetch(path, { method: opts.method || "GET", headers, body });
  const data = await res.json().catch(() => null);
  if (!res.ok) {
    const msg = (data?.error || data?.message || "Request failed") + " (" + res.status + ")";
    throw new Error(msg);
  }
  return data as T;
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
      verify_url?: string;
      error?: string;
      detail?: string;
    }>("/api/frontdoor/session/" + encodeURIComponent(sessionId));
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
};
