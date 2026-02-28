import React, { useCallback, useEffect, useMemo, useState } from "react";
import type { Bootstrap } from "./App";
import { api } from "./api";

const STEPS = ["identity", "objective", "config", "decision", "signature", "provisioning"] as const;
type StepId = (typeof STEPS)[number];
type StepStatus = "pending" | "active" | "done" | "error";
type Phase = "await_identity" | "await_objective" | "planning" | "await_launch_confirmation" | "launching" | "provisioning" | "ready";

interface Config {
  profile_name?: string;
  profile_domain?: string;
  custody_mode?: string;
  verification_backend?: string;
  gateway_auth_key?: string;
  [k: string]: unknown;
}

function normalizeWallet(v: string): string {
  const w = String(v || "").trim();
  return /^0x[a-fA-F0-9]{40}$/.test(w) ? w.toLowerCase() : "";
}

function maskKey(v: string): string {
  if (!v || v.length <= 8) return v || "-";
  return v.slice(0, 4) + "..." + v.slice(-4);
}

function generateGatewayAuthKey(): string {
  const alphabet = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes).map((b) => alphabet[b! % alphabet.length]).join("");
}

function renderEnvironment(): string {
  const host = String(window.location.hostname || "").toLowerCase();
  if (!host || host === "localhost" || host === "127.0.0.1") return "Local";
  if (/stage|staging|-stg/.test(host)) return "Staging";
  return "Production";
}

function sanitizeRuntimeUrl(value: unknown): string {
  const raw = String(value || "").trim();
  if (!raw) return "";
  try {
    const url = new URL(raw, window.location.origin);
    if (url.protocol !== "http:" && url.protocol !== "https:") return "";
    if (url.origin === window.location.origin) {
      const currentPath = window.location.pathname.replace(/\/+$/, "") || "/";
      const targetPath = url.pathname.replace(/\/+$/, "") || "/";
      if (targetPath === currentPath || targetPath === "/frontdoor") {
        return "";
      }
    }
    return url.toString();
  } catch (_) {
    return "";
  }
}

function statusTone(value: unknown): "positive" | "warning" | "negative" | "neutral" {
  const normalized = String(value || "").trim().toLowerCase();
  if (!normalized) return "neutral";
  if (
    normalized.includes("ready") ||
    normalized.includes("running") ||
    normalized.includes("active") ||
    normalized.includes("healthy")
  ) {
    return "positive";
  }
  if (
    normalized.includes("error") ||
    normalized.includes("failed") ||
    normalized.includes("denied") ||
    normalized.includes("cancel")
  ) {
    return "negative";
  }
  if (
    normalized.includes("launch") ||
    normalized.includes("provision") ||
    normalized.includes("pending") ||
    normalized.includes("starting")
  ) {
    return "warning";
  }
  return "neutral";
}

export default function LaunchpadWalletOnly({ bootstrap }: { bootstrap: Bootstrap }) {
  const [state, setState] = useState({
    phase: "await_identity" as Phase,
    walletAddress: "",
    chainId: "",
    sessionId: "",
    challengeMessage: "",
    objective: "",
    config: null as Config | null,
    decision: null as { mode: string; title: string; reason: string } | null,
    gatewayAuthKey: "",
    privyUserId: "",
    privyIdentityToken: "",
    privyAccessToken: "",
    ethereumProvider: null as unknown,
    pollingTimer: null as ReturnType<typeof setInterval> | null,
    latestSessionStatus: "",
    redirectAttempted: false,
  });
  const [steps, setSteps] = useState<Record<StepId, { status: StepStatus; detail: string }>>(() => ({
    identity: { status: "active", detail: "Waiting for wallet connection." },
    objective: { status: "pending", detail: "Waiting for your goal." },
    config: { status: "pending", detail: "Not generated yet." },
    decision: { status: "pending", detail: "No decision yet." },
    signature: { status: "pending", detail: "Awaiting challenge issue." },
    provisioning: { status: "pending", detail: "No instance yet." },
  }));
  const [messages, setMessages] = useState<Array<{ role: string; text: string }>>([]);
  const [composerEnabled, setComposerEnabled] = useState(false);
  const [chatAction, setChatAction] = useState<{ label: string; onClick: () => void } | null>(null);
  const [inputValue, setInputValue] = useState("");
  const [session, setSession] = useState<{ session_id?: string; status?: string; provisioning_source?: string; runtime_state?: string; instance_url?: string; app_url?: string; verify_url?: string }>({});
  const [configSummary, setConfigSummary] = useState<Config>({});

  const addMessage = useCallback((role: string, text: string) => {
    setMessages((m) => [...m, { role, text }]);
  }, []);

  const setStepState = useCallback((step: StepId, status: StepStatus, detail: string) => {
    setSteps((s) => ({ ...s, [step]: { status, detail } }));
  }, []);

  const connectWallet = useCallback(async () => {
    const eth = (window as { ethereum?: { request: (a: { method: string; params?: unknown[] }) => Promise<unknown> } }).ethereum;
    if (!eth) {
      addMessage("error", "No wallet provider detected.");
      return;
    }
    const accounts = (await eth.request({ method: "eth_requestAccounts" })) as string[];
    if (!accounts?.[0]) {
      addMessage("error", "No account returned.");
      return;
    }
    const addr = normalizeWallet(accounts[0]!);
    if (!addr) {
      addMessage("error", "Invalid address.");
      return;
    }
    const chainIdHex = (await eth.request({ method: "eth_chainId" })) as string;
    setState((p) => ({
      ...p,
      walletAddress: addr,
      chainId: chainIdHex ? "0x" + parseInt(chainIdHex, 16).toString(16) : "",
      privyUserId: `wallet:${addr}`,
      ethereumProvider: eth,
    }));
    setStepState("identity", "done", "Wallet connected.");
    setStepState("objective", "active", "Tell me what you want this agent to do.");
    setState((p) => ({ ...p, phase: "await_objective" }));
    setComposerEnabled(true);
    setChatAction(null);
    addMessage("assistant", "Identity confirmed. Tell me what you want the agent to do.");
  }, [addMessage, setStepState]);

  useEffect(() => {
    addMessage("assistant", "Welcome. Wallet-only mode. Click Connect wallet below to continue.");
    setChatAction({ label: "Connect Wallet", onClick: connectWallet });
  }, []);

  const pollSessionStatus = useCallback(async () => {
    if (!state.sessionId) return;
    const s = await api.getSession(state.sessionId);
    setSession(s);
    if (s.status !== state.latestSessionStatus) {
      setState((p) => ({ ...p, latestSessionStatus: s.status || "" }));
      addMessage("system", `Session status: ${s.status} - ${s.detail || ""}`);
    }
    if (s.status === "ready") {
      if (state.pollingTimer) clearInterval(state.pollingTimer);
      setState((p) => ({ ...p, phase: "ready", pollingTimer: null }));
      setStepState("provisioning", "done", "Instance ready.");
      addMessage("assistant", "Your enclave is ready.");
      const targetUrl = s.instance_url || s.app_url || s.verify_url || "";
      if (targetUrl && !state.redirectAttempted) {
        setState((p) => ({ ...p, redirectAttempted: true }));
        addMessage("system", "Session ready. Redirecting...");
        if (window.parent !== window) {
          window.parent.postMessage({ source: "enclagent:launchpad", type: "session_ready_redirect", url: targetUrl, session_id: s.session_id || "" }, window.location.origin);
        }
        setTimeout(() => window.location.assign(targetUrl), 200);
      }
      setComposerEnabled(false);
      setChatAction(null);
    }
    if (["failed", "error", "verification_failed"].includes(s.status || "")) {
      if (state.pollingTimer) clearInterval(state.pollingTimer);
      setState((p) => ({ ...p, phase: "await_launch_confirmation", pollingTimer: null }));
      setStepState("provisioning", "error", "Provisioning failed.");
      addMessage("error", "Provisioning failed: " + (s.error || s.detail || "Unknown"));
      setComposerEnabled(true);
      setChatAction({ label: "Retry Launch", onClick: () => beginLaunchSequence() });
    }
  }, [state.sessionId, state.latestSessionStatus, state.pollingTimer, state.redirectAttempted, addMessage, setStepState]);

  const startPolling = useCallback(() => {
    const interval = Math.max(1200, bootstrap.poll_interval_ms || 1500);
    const timer = setInterval(pollSessionStatus, interval);
    setState((p) => ({ ...p, pollingTimer: timer }));
    pollSessionStatus();
  }, [bootstrap.poll_interval_ms, pollSessionStatus]);

  const normalizeConfig = (c: Record<string, unknown>): Config => {
    const out = { ...c } as Config;
    out.profile_name = (out.profile_name as string) || "ironclaw_profile_" + Date.now();
    out.profile_domain = (out.profile_domain as string) || "general";
    out.custody_mode = (out.custody_mode as string) || "user_wallet";
    out.verification_backend = (out.verification_backend as string) || "eigencloud_primary";
    out.gateway_auth_key = (out.gateway_auth_key as string) || state.gatewayAuthKey;
    out.accept_terms = true;
    out.user_wallet_address = (out.user_wallet_address as string) || state.walletAddress;
    out.symbol_allowlist = Array.isArray(out.symbol_allowlist) ? out.symbol_allowlist : ["BTC", "ETH"];
    out.enable_memory = out.enable_memory ?? true;
    return out;
  };

  const deriveDecision = (objective: string, config: Config) => {
    const text = objective.toLowerCase();
    const live = /(live|execution|execute|trade|production|autonomous|deploy|24\/7)/.test(text);
    const dedicated = live || String(config.paper_live_policy || "").toLowerCase() === "live_allowed";
    if (dedicated) return { mode: "dedicated", title: "Dedicated Enclaved IronClaw Instance", reason: "Objective indicates continuous or execution-sensitive behavior." };
    return { mode: "shared", title: "Shared Runtime First", reason: "Objective indicates research/planning posture." };
  };

  const ensureOnboardingReady = async (sessionId: string, config: Config, objective: string) => {
    let st = await api.getOnboardingState(sessionId);
    const objectiveText = objective.trim() || `Launch profile ${config.profile_name || "frontdoor_profile"} with deterministic verification.`;
    const assignments = `profile_name=${String(config.profile_name || "frontdoor_profile").replace(/[\n\r,;=]/g, "_")}, gateway_auth_key=__from_config__, accept_terms=true`;
    if (!st.objective) st = await api.postOnboardingChat(sessionId, objectiveText) as { objective?: string; missing_fields?: string[]; current_step?: string; completed?: boolean };
    if (Array.isArray(st.missing_fields) && st.missing_fields.length > 0) st = await api.postOnboardingChat(sessionId, assignments) as typeof st;
    if (st.current_step !== "ready_to_sign" && !st.completed) st = await api.postOnboardingChat(sessionId, "confirm plan") as typeof st;
    if (Array.isArray(st.missing_fields) && st.missing_fields.length > 0) {
      st = await api.postOnboardingChat(sessionId, assignments) as typeof st;
      st = await api.postOnboardingChat(sessionId, "confirm plan") as typeof st;
    }
    if (Array.isArray(st.missing_fields) && st.missing_fields.length > 0) throw new Error("Onboarding required variables unresolved: " + st.missing_fields.join(", "));
    if (st.current_step !== "ready_to_sign" && !st.completed) st = await api.postOnboardingChat(sessionId, "confirm sign") as typeof st;
    if (st.current_step !== "ready_to_sign" && !st.completed) throw new Error("Onboarding did not reach ready_to_sign state.");
  };

  const signMessage = async (message: string): Promise<string> => {
    const provider = state.ethereumProvider as { request: (a: { method: string; params: unknown[] }) => Promise<string> } | null;
    if (!provider?.request) throw new Error("Wallet provider unavailable.");
    const hexMsg = "0x" + Array.from(new TextEncoder().encode(message)).map((b) => b.toString(16).padStart(2, "0")).join("");
    const wallet = normalizeWallet(state.walletAddress);
    for (const params of [[hexMsg, wallet], [message, wallet], [wallet, hexMsg], [wallet, message]]) {
      try {
        const sig = await provider.request({ method: "personal_sign", params });
        if (sig) return sig;
      } catch (_) {}
    }
    throw new Error("Wallet signature failed.");
  };

  const beginLaunchSequence = async () => {
    if (!state.config || !state.walletAddress) {
      addMessage("error", "Cannot launch: missing config or wallet.");
      return;
    }
    setState((p) => ({ ...p, phase: "launching", redirectAttempted: false }));
    setComposerEnabled(false);
    setChatAction(null);
    setStepState("signature", "active", "Issuing challenge...");
    setStepState("provisioning", "active", "Waiting for verification...");
    try {
      const chainIdNum = state.chainId ? (state.chainId.startsWith("0x") ? parseInt(state.chainId, 16) : parseInt(state.chainId, 10)) : null;
      const challenge = await api.challenge({ wallet_address: state.walletAddress, privy_user_id: state.privyUserId || null, chain_id: chainIdNum });
      const sessionId = String(challenge.session_id || "");
      const challengeMessage = String(challenge.message || "");
      setState((p) => ({ ...p, sessionId, challengeMessage }));
      setSession({ session_id: sessionId, status: "challenge_issued", provisioning_source: "pending", runtime_state: "pending" });
      await ensureOnboardingReady(sessionId, state.config, state.objective);
      const signature = await signMessage(challengeMessage);
      await api.verify({
        session_id: sessionId,
        wallet_address: state.walletAddress,
        privy_user_id: state.privyUserId || null,
        privy_identity_token: state.privyIdentityToken || null,
        privy_access_token: state.privyAccessToken || null,
        message: challengeMessage,
        signature,
        config: state.config,
      });
      setStepState("signature", "done", "Signature accepted.");
      setStepState("provisioning", "active", "Provisioning started. Polling...");
      addMessage("assistant", "Signature verified. Provisioning has started.");
      setState((p) => ({ ...p, phase: "provisioning" }));
      startPolling();
    } catch (err) {
      setStepState("signature", "error", "Challenge/signature failed.");
      setStepState("provisioning", "error", "Provisioning did not start.");
      setState((p) => ({ ...p, phase: "await_launch_confirmation" }));
      setComposerEnabled(true);
      setChatAction({ label: "Retry Signature", onClick: () => beginLaunchSequence() });
      addMessage("error", "Launch failed: " + String((err as Error)?.message || err));
    }
  };

  const handleObjective = async (message: string) => {
    setState((p) => ({ ...p, objective: message, phase: "planning" }));
    setStepState("objective", "done", "Objective captured.");
    setStepState("config", "active", "Generating configuration draft...");
    setComposerEnabled(false);
    setChatAction(null);
    try {
      const gatewayAuthKey = generateGatewayAuthKey();
      setState((p) => ({ ...p, gatewayAuthKey }));
      const suggestion = await api.suggestConfig({ wallet_address: state.walletAddress, intent: message, gateway_auth_key: gatewayAuthKey });
      const config = normalizeConfig(suggestion?.config || {});
      setState((p) => ({ ...p, config }));
      setConfigSummary(config);
      setStepState("config", "done", "Config drafted.");
      const decision = deriveDecision(message, config);
      setState((p) => ({ ...p, decision }));
      setStepState("decision", "done", decision.title);
      setStepState("signature", "active", "Pending challenge and signature.");
      addMessage("assistant", `Configuration ready. Runtime decision: ${decision.title}. ${decision.reason} Reply continue to issue your challenge and sign.`);
      setState((p) => ({ ...p, phase: "await_launch_confirmation" }));
      setComposerEnabled(true);
      setChatAction({ label: "Continue to Signature", onClick: () => beginLaunchSequence() });
    } catch (err) {
      setStepState("config", "error", "Config draft failed.");
      setState((p) => ({ ...p, phase: "await_objective" }));
      setComposerEnabled(true);
      addMessage("error", "Failed to draft configuration: " + String((err as Error)?.message || err));
    }
  };

  const handleUserInput = async (message: string) => {
    addMessage("user", message);
    if (state.phase === "await_identity") {
      addMessage("assistant", "Complete wallet connection first.");
      return;
    }
    if (state.phase === "await_objective") {
      await handleObjective(message);
      return;
    }
    if (state.phase === "await_launch_confirmation") {
      if (/^(continue|launch|proceed|yes|y|confirm)$/i.test(message.trim())) await beginLaunchSequence();
      else addMessage("assistant", "Type continue when ready.");
    }
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const v = inputValue.trim();
    if (!v) return;
    setInputValue("");
    handleUserInput(v);
  };

  const safeInstanceUrl = useMemo(
    () => sanitizeRuntimeUrl(session.instance_url || session.app_url),
    [session.instance_url, session.app_url]
  );
  const safeVerifyUrl = useMemo(
    () => sanitizeRuntimeUrl(session.verify_url),
    [session.verify_url]
  );
  const sessionTone = statusTone(session.status || state.latestSessionStatus);
  const runtimeTone = statusTone(session.runtime_state);
  const gatewayLinks = useMemo(
    () =>
      [
        {
          key: "runtime",
          title: "Runtime Gateway",
          detail: "Primary endpoint for live agent runtime traffic.",
          url: safeInstanceUrl,
        },
        {
          key: "verify",
          title: "Verification Gateway",
          detail: "Audit and verification surface for receipts and proofs.",
          url: safeVerifyUrl,
        },
      ].filter((gateway) => !!gateway.url),
    [safeInstanceUrl, safeVerifyUrl]
  );

  return (
    <>
      <div className="lp-bg lp-bg-a" />
      <div className="lp-bg lp-bg-b" />
      <main className="lp-shell">
        <header className="lp-header">
          <div>
            <p className="lp-eyebrow">Enclagent Gateway Launchpad</p>
            <h1>Enclagent Setup</h1>
          </div>
          <div className="lp-environment">{renderEnvironment()}</div>
        </header>
        <section className="lp-grid">
          <article className="lp-chat-card">
            <div className="lp-chat-head">
              <h2>Launchpad Chat</h2>
              <p>Identity, objective, policy, signature, provisioning.</p>
            </div>
            <div className="lp-chat-stream">
              {messages.map((m, i) => (
                <div key={i} className={`lp-msg ${m.role}`}>{m.text}</div>
              ))}
            </div>
            <div className="lp-chat-actions">
              {state.phase === "await_identity" && (
                <button type="button" className="lp-action-btn" onClick={connectWallet}>
                  Connect Wallet
                </button>
              )}
              {chatAction && (
                <button type="button" className="lp-action-btn" onClick={chatAction.onClick}>
                  {chatAction.label}
                </button>
              )}
            </div>
            <form className="lp-chat-compose" onSubmit={handleSubmit}>
              <textarea rows={1} placeholder="Describe what you want this agent to do..." disabled={!composerEnabled} value={inputValue} onChange={(e) => setInputValue(e.target.value)} />
              <button type="submit" disabled={!composerEnabled}>Send</button>
            </form>
          </article>
          <aside className="lp-side-card">
            <section className="lp-side-block">
              <h3>Step Progress</h3>
              <div className="lp-steps">
                {STEPS.map((step) => (
                  <div key={step} className={`lp-step ${steps[step]?.status === "active" ? "is-active" : ""} ${steps[step]?.status === "done" ? "is-done" : ""} ${steps[step]?.status === "error" ? "is-error" : ""}`} data-step={step}>
                    <span className="lp-step-dot" />
                    <div>
                      <p className="lp-step-title">{step === "identity" && "1. Wallet"} {step === "objective" && "2. Objective"} {step === "config" && "3. Config Draft"} {step === "decision" && "4. Runtime Decision"} {step === "signature" && "5. Signature"} {step === "provisioning" && "6. Provisioning"}</p>
                      <p className="lp-step-desc">{steps[step]?.detail || "-"}</p>
                    </div>
                  </div>
                ))}
              </div>
            </section>
            <section className="lp-side-block">
              <h3>Configuration Summary</h3>
              <div className="lp-kv">
                <div className="lp-kv-row"><span>Profile</span><strong>{configSummary.profile_name || "Pending"}</strong></div>
                <div className="lp-kv-row"><span>Domain</span><strong>{configSummary.profile_domain || "Pending"}</strong></div>
                <div className="lp-kv-row"><span>Custody</span><strong>{configSummary.custody_mode || "Pending"}</strong></div>
                <div className="lp-kv-row"><span>Verification</span><strong>{configSummary.verification_backend || "Pending"}</strong></div>
                <div className="lp-kv-row"><span>Gateway Auth Key</span><strong>{maskKey(configSummary.gateway_auth_key || "")}</strong></div>
              </div>
            </section>
            <section className="lp-side-block">
              <h3>Session Status</h3>
              <div className="lp-kv">
                <div className="lp-kv-row"><span>Session</span><strong>{session.session_id || "-"}</strong></div>
                <div className="lp-kv-row">
                  <span>Status</span>
                  <strong className={`lp-status-pill is-${sessionTone}`}>{session.status || state.latestSessionStatus || "-"}</strong>
                </div>
                <div className="lp-kv-row"><span>Provisioning Source</span><strong>{session.provisioning_source || "-"}</strong></div>
                <div className="lp-kv-row">
                  <span>Runtime</span>
                  <strong className={`lp-status-pill is-${runtimeTone}`}>{session.runtime_state || "-"}</strong>
                </div>
              </div>
              <div className="lp-gateway-grid">
                {gatewayLinks.length > 0 ? (
                  gatewayLinks.map((gateway) => (
                    <article key={gateway.key} className="lp-gateway-card">
                      <div className="lp-gateway-card-head">
                        <span className="lp-gateway-tag">{gateway.key === "runtime" ? "Runtime" : "Verify"}</span>
                        <span className="lp-gateway-health">Online</span>
                      </div>
                      <p className="lp-gateway-title">{gateway.title}</p>
                      <p className="lp-gateway-detail">{gateway.detail}</p>
                      <p className="lp-gateway-url">{gateway.url}</p>
                      <a
                        href={gateway.url}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="lp-gateway-open"
                      >
                        Open Gateway
                      </a>
                    </article>
                  ))
                ) : (
                  <div className="lp-gateway-empty">
                    Gateway endpoints appear here after provisioning reaches ready state.
                  </div>
                )}
              </div>
            </section>
          </aside>
        </section>
      </main>
    </>
  );
}
