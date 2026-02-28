import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { usePrivy, useLogin, useWallets, useCreateWallet } from "@privy-io/react-auth";
import type { Bootstrap } from "./App";
import { api } from "./api";

const STEPS = [
  "identity",
  "objective",
  "config",
  "decision",
  "signature",
  "provisioning",
] as const;
const MAX_CHAT_MESSAGES = 160;

type StepId = (typeof STEPS)[number];
type StepStatus = "pending" | "active" | "done" | "error";
type Phase =
  | "await_identity"
  | "await_objective"
  | "planning"
  | "await_launch_confirmation"
  | "launching"
  | "provisioning"
  | "ready";

interface StepState {
  status: StepStatus;
  detail: string;
}

interface Message {
  role: "user" | "assistant" | "system" | "error";
  text: string;
}

interface Config {
  profile_name?: string;
  profile_domain?: string;
  custody_mode?: string;
  verification_backend?: string;
  gateway_auth_key?: string;
  [k: string]: unknown;
}

interface Session {
  session_id?: string;
  status?: string;
  provisioning_source?: string;
  runtime_state?: string;
  instance_url?: string;
  verify_url?: string;
  error?: string;
  detail?: string;
}

interface WalletProvider {
  request: (args: { method: string; params?: unknown[] }) => Promise<unknown>;
}

interface PrivyWalletLike {
  address?: string;
  walletClientType?: string;
  wallet_client_type?: string;
  chainType?: string;
  chain_type?: string;
  getEthereumProvider?: () => Promise<unknown>;
}

function normalizeWallet(value: string): string {
  const w = String(value || "").trim();
  return /^0x[a-fA-F0-9]{40}$/.test(w) ? w.toLowerCase() : "";
}

function isWalletProvider(value: unknown): value is WalletProvider {
  return !!value && typeof value === "object" && typeof (value as WalletProvider).request === "function";
}

function normalizeChainId(value: unknown): string {
  if (typeof value === "number" && Number.isFinite(value) && value > 0) {
    return "0x" + value.toString(16);
  }
  if (typeof value !== "string") return "";
  const raw = value.trim();
  if (!raw) return "";
  if (/^0x[0-9a-fA-F]+$/.test(raw)) return "0x" + parseInt(raw, 16).toString(16);
  if (/^\d+$/.test(raw)) return "0x" + parseInt(raw, 10).toString(16);
  return "";
}

function parseChainIdNumber(value: string): number | null {
  const normalized = normalizeChainId(value);
  if (!normalized) return null;
  const parsed = parseInt(normalized, 16);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

function isWalletAlreadyExistsError(value: unknown): boolean {
  const message = String((value as { message?: string })?.message || value || "").toLowerCase();
  return message.includes("already") && message.includes("wallet");
}

function getEthereumEmbeddedAddress(user: unknown): string {
  const u = user as { linkedAccounts?: Array<{ type?: string; walletClientType?: string; chainType?: string; address?: string }> } | null;
  if (!u?.linkedAccounts) return "";
  for (const acc of u.linkedAccounts) {
    if (
      acc?.type === "wallet" &&
      (acc.walletClientType === "privy" || (acc as { wallet_client_type?: string }).wallet_client_type === "privy") &&
      (acc.chainType === "ethereum" || (acc as { chain_type?: string }).chain_type === "ethereum") &&
      typeof acc.address === "string"
    ) {
      const addr = String(acc.address).trim();
      if (/^0x[a-fA-F0-9]{40}$/.test(addr)) return addr;
    }
  }
  return "";
}

function extractWalletFromUser(user: unknown): string {
  const u = user as { linkedAccounts?: Array<{ address?: string }>; linked_accounts?: Array<{ address?: string }>; accounts?: Array<{ address?: string }>; wallet?: { address?: string }; wallet_address?: string } | null;
  if (!u) return "";
  const embedded = getEthereumEmbeddedAddress(u);
  if (embedded) return embedded;
  const linked = u.linkedAccounts || u.linked_accounts || [];
  for (const acc of linked) {
    if (acc?.address && /^0x[a-fA-F0-9]{40}$/.test(String(acc.address))) return String(acc.address).trim();
  }
  if (u.wallet?.address && /^0x[a-fA-F0-9]{40}$/.test(String(u.wallet.address))) return String(u.wallet.address).trim();
  if (u.wallet_address && /^0x[a-fA-F0-9]{40}$/.test(String(u.wallet_address))) return String(u.wallet_address).trim();
  return "";
}

function extractPrivyUserId(user: unknown, fallback: string): string {
  const u = user as { id?: string; user_id?: string; did?: string } | null;
  if (u?.id) return String(u.id).trim();
  if (u?.user_id) return String(u.user_id).trim();
  if (u?.did) return String(u.did).trim();
  return fallback ? `wallet:${normalizeWallet(fallback)}` : "";
}

function maskKey(v: string): string {
  if (!v || v.length <= 8) return v || "-";
  return v.slice(0, 4) + "..." + v.slice(-4);
}

function generateGatewayAuthKey(): string {
  const alphabet = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  let out = "";
  for (let i = 0; i < bytes.length; i++) out += alphabet[bytes[i]! % alphabet.length];
  return out;
}

function renderEnvironment(): string {
  const host = String(window.location.hostname || "").toLowerCase();
  if (!host || host === "localhost" || host === "127.0.0.1") return "Local";
  if (/stage|staging|-stg/.test(host)) return "Staging";
  return "Production";
}

export default function Launchpad({ bootstrap }: { bootstrap: Bootstrap }) {
  const appId = String(bootstrap.privy_app_id || "").trim();
  const hasPrivy = !!appId;

  const privy = usePrivy();
  const { wallets, ready: walletsReady } = useWallets();
  const user = privy?.user ?? null;
  const ready = !!privy?.ready;
  const authenticated = !!user;

  const ethereumEmbeddedAddress = useMemo(() => (user ? getEthereumEmbeddedAddress(user) : ""), [user]);

  const resolveWalletContext = useCallback(
    async (preferredWalletAddress: string) => {
      if (!walletsReady || !Array.isArray(wallets) || wallets.length === 0) return null;
      const preferred = normalizeWallet(preferredWalletAddress);
      const candidates = (wallets as PrivyWalletLike[]).filter((wallet) => {
        const chainType = String(wallet.chainType || wallet.chain_type || "").trim().toLowerCase();
        if (chainType && chainType !== "ethereum") return false;
        const client = String(wallet.walletClientType || wallet.wallet_client_type || "").trim().toLowerCase();
        return !client || client === "privy";
      });
      const ordered = [...candidates];
      if (preferred) {
        ordered.sort((a, b) => {
          const aMatch = normalizeWallet(String(a.address || "")) === preferred;
          const bMatch = normalizeWallet(String(b.address || "")) === preferred;
          if (aMatch === bMatch) return 0;
          return aMatch ? -1 : 1;
        });
      }
      for (const wallet of ordered) {
        if (typeof wallet.getEthereumProvider !== "function") continue;
        try {
          const providerCandidate = await wallet.getEthereumProvider();
          if (!isWalletProvider(providerCandidate)) continue;
          const providerChain = await providerCandidate
            .request({ method: "eth_chainId" })
            .catch(() => "");
          return {
            provider: providerCandidate,
            walletAddress:
              normalizeWallet(String(wallet.address || "")) || preferred,
            chainId: normalizeChainId(providerChain),
          };
        } catch (_) {
          continue;
        }
      }
      return null;
    },
    [wallets, walletsReady]
  );

  const { login } = useLogin({
    onComplete: async (payload) => {
      const user = payload?.user || null;
      const payloadWallet = extractWalletFromUser(user) || (payload as { walletAddress?: string }).walletAddress || "";
      const wallet = normalizeWallet(payloadWallet);
      const privyUserId = extractPrivyUserId(user, wallet);
      const identityToken = (payload as { identityToken?: string }).identityToken || "";
      const accessToken = (payload as { accessToken?: string }).accessToken || "";
      const payloadChainId = normalizeChainId((payload as { chainId?: string }).chainId || "");
      const payloadProvider = isWalletProvider((payload as { ethereumProvider?: unknown }).ethereumProvider)
        ? (payload as { ethereumProvider?: WalletProvider }).ethereumProvider
        : null;
      const walletContext = payloadProvider
        ? {
            provider: payloadProvider,
            walletAddress: wallet,
            chainId: payloadChainId,
          }
        : await resolveWalletContext(wallet);
      onPrivyLoginComplete({
        walletAddress: wallet || walletContext?.walletAddress || "",
        privyUserId,
        identityToken,
        accessToken,
        chainId: payloadChainId || walletContext?.chainId || "",
        ethereumProvider: walletContext?.provider || payloadProvider || null,
      });
    },
    onError: (err: unknown) => {
      const msg = String((err as { message?: string })?.message ?? err);
      if (msg.includes("exited_auth_flow") || msg.includes("user_cancelled")) return;
      addMessage("error", "Privy login failed: " + msg);
    },
  });

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

  const walletAddress = ethereumEmbeddedAddress || extractWalletFromUser(user) || state.walletAddress;

  const [steps, setSteps] = useState<Record<StepId, StepState>>(() => ({
    identity: { status: "active", detail: "Waiting for Privy login." },
    objective: { status: "pending", detail: "Waiting for your goal." },
    config: { status: "pending", detail: "Not generated yet." },
    decision: { status: "pending", detail: "No decision yet." },
    signature: { status: "pending", detail: "Awaiting challenge issue." },
    provisioning: { status: "pending", detail: "No instance yet." },
  }));

  const [messages, setMessages] = useState<Message[]>([]);
  const [composerEnabled, setComposerEnabled] = useState(false);
  const [chatAction, setChatAction] = useState<{ label: string; onClick: () => void } | null>(null);
  const [inputValue, setInputValue] = useState("");
  const [session, setSession] = useState<Session>({});
  const [configSummary, setConfigSummary] = useState<Config>({});
  const welcomeShownRef = useRef(false);
  const handledIdentityKeyRef = useRef("");
  const walletProvisionKeyRef = useRef("");
  const walletProvisionInFlightRef = useRef(false);
  const sessionIdRef = useRef("");
  const latestSessionStatusRef = useRef("");
  const pollingTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const redirectAttemptedRef = useRef(false);

  const addMessage = useCallback((role: Message["role"], text: string) => {
    setMessages((m) => {
      const next = [...m, { role, text }];
      return next.length > MAX_CHAT_MESSAGES ? next.slice(next.length - MAX_CHAT_MESSAGES) : next;
    });
  }, []);

  const addMessageIfNew = useCallback((role: Message["role"], text: string) => {
    setMessages((m) => {
      const last = m[m.length - 1];
      if (last && last.role === role && last.text === text) return m;
      const next = [...m, { role, text }];
      return next.length > MAX_CHAT_MESSAGES ? next.slice(next.length - MAX_CHAT_MESSAGES) : next;
    });
  }, []);

  const setStepState = useCallback((step: StepId, status: StepStatus, detail: string) => {
    setSteps((s) => ({ ...s, [step]: { status, detail } }));
  }, []);

  const { createWallet } = useCreateWallet({
    onSuccess: () => {
      addMessageIfNew("system", "Embedded wallet provisioned. Finalizing identity.");
      setStepState("identity", "active", "Embedded wallet provisioned. Finalizing identity.");
    },
    onError: (error) => {
      if (isWalletAlreadyExistsError(error)) return;
      addMessageIfNew("error", "Embedded wallet provisioning failed: " + String(error || "unknown_error"));
    },
  });

  const provisionEmbeddedWallet = useCallback(async () => {
    if (walletProvisionInFlightRef.current) return;
    walletProvisionInFlightRef.current = true;
    setStepState("identity", "active", "Provisioning Privy embedded wallet...");
    addMessageIfNew("system", "No wallet detected yet. Provisioning embedded wallet via Privy.");
    try {
      await createWallet();
    } catch (error) {
      if (!isWalletAlreadyExistsError(error)) {
        addMessageIfNew("error", "Embedded wallet provisioning failed: " + String((error as { message?: string })?.message || error));
      }
    } finally {
      walletProvisionInFlightRef.current = false;
    }
  }, [addMessageIfNew, createWallet, setStepState]);

  useEffect(() => {
    sessionIdRef.current = state.sessionId;
  }, [state.sessionId]);

  useEffect(() => {
    latestSessionStatusRef.current = state.latestSessionStatus;
  }, [state.latestSessionStatus]);

  useEffect(() => {
    redirectAttemptedRef.current = state.redirectAttempted;
  }, [state.redirectAttempted]);

  const clearPolling = useCallback(() => {
    if (pollingTimerRef.current) {
      clearInterval(pollingTimerRef.current);
      pollingTimerRef.current = null;
    }
    setState((prev) => (prev.pollingTimer ? { ...prev, pollingTimer: null } : prev));
  }, []);

  const pollSessionStatus = useCallback(async () => {
    const sessionId = sessionIdRef.current;
    if (!sessionId) return;
    const s = await api.getSession(sessionId);
    const status = String(s.status || "");
    setSession(s);
    if (status && status !== latestSessionStatusRef.current) {
      latestSessionStatusRef.current = status;
      setState((prev) =>
        prev.latestSessionStatus === status
          ? prev
          : { ...prev, latestSessionStatus: status }
      );
      addMessage("system", `Session status: ${status} - ${s.detail || ""}`);
    }
    if (status === "ready") {
      clearPolling();
      setState((prev) => ({ ...prev, phase: "ready" }));
      setStepState("provisioning", "done", "Instance ready.");
      addMessage("assistant", "Your enclave is ready. Use the session links to open the runtime and verification surface.");
      const targetUrl = s.instance_url || s.verify_url || "";
      if (targetUrl && !redirectAttemptedRef.current) {
        redirectAttemptedRef.current = true;
        setState((prev) => (prev.redirectAttempted ? prev : { ...prev, redirectAttempted: true }));
        addMessage("system", "Session ready. Redirecting to runtime...");
        if (window.parent !== window) {
          window.parent.postMessage({ source: "enclagent:launchpad", type: "session_ready_redirect", url: targetUrl, session_id: s.session_id || "" }, window.location.origin);
        }
        setTimeout(() => window.location.assign(targetUrl), 200);
      }
      setComposerEnabled(false);
      setChatAction(null);
    }
    if (["failed", "error", "verification_failed"].includes(status)) {
      clearPolling();
      setState((prev) => ({ ...prev, phase: "await_launch_confirmation" }));
      setStepState("provisioning", "error", "Provisioning failed.");
      addMessage("error", "Provisioning failed: " + (s.error || s.detail || "Unknown"));
      setComposerEnabled(true);
      setChatAction({ label: "Retry Launch", onClick: () => beginLaunchSequence() });
    }
  }, [addMessage, clearPolling, setStepState]);

  const startPolling = useCallback(() => {
    clearPolling();
    const interval = Math.max(1200, bootstrap.poll_interval_ms || 1500);
    const timer = setInterval(() => {
      void pollSessionStatus();
    }, interval);
    pollingTimerRef.current = timer;
    setState((prev) => ({ ...prev, pollingTimer: timer }));
    void pollSessionStatus();
  }, [bootstrap.poll_interval_ms, clearPolling, pollSessionStatus]);

  useEffect(() => {
    return () => clearPolling();
  }, [clearPolling]);

  const onPrivyLoginComplete = useCallback(
    (payload: {
      walletAddress?: string;
      privyUserId?: string;
      identityToken?: string;
      accessToken?: string;
      chainId?: string;
      ethereumProvider?: unknown;
    }) => {
      const wallet = payload.walletAddress ? normalizeWallet(payload.walletAddress) : "";
      const provider = isWalletProvider(payload.ethereumProvider) ? payload.ethereumProvider : null;
      const privyUserId = payload.privyUserId || (wallet ? `wallet:${wallet}` : "");
      const identityKey = `${privyUserId}|${wallet}`;
      const isDuplicateIdentity =
        identityKey !== "|" && handledIdentityKeyRef.current === identityKey;
      setState((prev) => ({
        ...prev,
        privyUserId,
        privyIdentityToken: payload.identityToken || "",
        privyAccessToken: payload.accessToken || "",
        walletAddress: wallet || prev.walletAddress,
        chainId: payload.chainId || prev.chainId,
        ethereumProvider: provider ?? prev.ethereumProvider,
      }));
      if (isDuplicateIdentity) {
        return;
      }
      if (identityKey !== "|") {
        handledIdentityKeyRef.current = identityKey;
      }
      if (!wallet) {
        const needsProvisioning = !wallet || !ethereumEmbeddedAddress;
        setStepState(
          "identity",
          "active",
          needsProvisioning
            ? "Privy authenticated. Provisioning embedded wallet."
            : "Privy authenticated. Resolving wallet signer."
        );
        setComposerEnabled(false);
        setChatAction(null);
        addMessageIfNew(
          "assistant",
          needsProvisioning
            ? "Privy login complete. Provisioning your embedded wallet."
            : "Privy login complete. Finalizing wallet signer."
        );
        return;
      }
      if (!provider) {
        setStepState("identity", "done", "Privy identity connected. Wallet signer pending.");
        setChatAction(null);
        setStepState("objective", "active", "Tell me what you want this agent to do.");
        setState((prev) => ({ ...prev, phase: "await_objective" }));
        setComposerEnabled(true);
        addMessageIfNew(
          "assistant",
          "Identity confirmed. Tell me what you want the agent to do. Signer finalization will continue in the background."
        );
        return;
      }
      setStepState("identity", "done", "Privy identity connected.");
      setChatAction(null);
      setStepState("objective", "active", "Tell me what you want this agent to do.");
      setState((prev) => ({ ...prev, phase: "await_objective" }));
      setComposerEnabled(true);
      addMessageIfNew("assistant", "Identity confirmed. Tell me what you want the agent to do.");
    },
    [addMessageIfNew, ethereumEmbeddedAddress, setStepState]
  );

  useEffect(() => {
    if (!hasPrivy) return;
    if (!ready || !authenticated || !user) return;
    const wallet = normalizeWallet(extractWalletFromUser(user) || ethereumEmbeddedAddress);
    const privyUserId = extractPrivyUserId(user, wallet);
    const privyAny = privy as { getIdentityToken?: () => Promise<string>; getAccessToken?: () => Promise<string> } | null;
    const getIdentityToken = privyAny?.getIdentityToken;
    const getAccessToken = privyAny?.getAccessToken;
    (async () => {
      let identityToken = "";
      let accessToken = "";
      if (typeof getIdentityToken === "function") identityToken = (await getIdentityToken()) || "";
      if (typeof getAccessToken === "function") accessToken = (await getAccessToken()) || "";
      const walletContext = await resolveWalletContext(wallet);
      onPrivyLoginComplete({
        walletAddress: wallet || walletContext?.walletAddress || "",
        privyUserId,
        identityToken,
        accessToken,
        chainId: walletContext?.chainId || "",
        ethereumProvider: walletContext?.provider || null,
      });
    })();
  }, [
    hasPrivy,
    ready,
    authenticated,
    user,
    ethereumEmbeddedAddress,
    privy,
    onPrivyLoginComplete,
    resolveWalletContext,
  ]);

  useEffect(() => {
    if (!ready || !authenticated || !walletAddress) return;
    if (isWalletProvider(state.ethereumProvider)) return;
    let cancelled = false;
    (async () => {
      const walletContext = await resolveWalletContext(walletAddress);
      if (cancelled || !walletContext?.provider) return;
      setState((prev) => ({
        ...prev,
        ethereumProvider: walletContext.provider,
        chainId: prev.chainId || walletContext.chainId || "",
      }));
      setStepState("identity", "done", "Privy identity connected.");
      addMessageIfNew("system", "Wallet signer connected.");
    })();
    return () => {
      cancelled = true;
    };
  }, [
    ready,
    authenticated,
    walletAddress,
    state.ethereumProvider,
    resolveWalletContext,
    setStepState,
    addMessageIfNew,
  ]);

  useEffect(() => {
    if (!ready || !authenticated || !user) return;
    if (normalizeWallet(ethereumEmbeddedAddress)) return;
    const provisionKey = extractPrivyUserId(user, "") || String((user as { id?: string }).id || "").trim();
    if (!provisionKey || walletProvisionKeyRef.current === provisionKey) return;
    walletProvisionKeyRef.current = provisionKey;
    const timer = setTimeout(() => {
      void provisionEmbeddedWallet();
    }, 700);
    return () => clearTimeout(timer);
  }, [ready, authenticated, user, ethereumEmbeddedAddress, provisionEmbeddedWallet]);

  useEffect(() => {
    if (welcomeShownRef.current) return;
    welcomeShownRef.current = true;
    addMessageIfNew("assistant", "Welcome. First step: sign up or log in with Privy and connect your wallet.");
  }, [addMessageIfNew]);

  const handleLogout = useCallback(async () => {
    clearPolling();
    if (privy?.logout) await privy.logout();
    setState({
      phase: "await_identity",
      walletAddress: "",
      chainId: "",
      sessionId: "",
      challengeMessage: "",
      objective: "",
      config: null,
      decision: null,
      gatewayAuthKey: "",
      privyUserId: "",
      privyIdentityToken: "",
      privyAccessToken: "",
      ethereumProvider: null,
      pollingTimer: null,
      latestSessionStatus: "",
      redirectAttempted: false,
    });
    setSteps({
      identity: { status: "active", detail: "Waiting for Privy login." },
      objective: { status: "pending", detail: "Waiting for your goal." },
      config: { status: "pending", detail: "Not generated yet." },
      decision: { status: "pending", detail: "No decision yet." },
      signature: { status: "pending", detail: "Awaiting challenge issue." },
      provisioning: { status: "pending", detail: "No instance yet." },
    });
    setConfigSummary({});
    setSession({});
    setComposerEnabled(false);
    setChatAction(null);
    handledIdentityKeyRef.current = "";
    welcomeShownRef.current = false;
    walletProvisionKeyRef.current = "";
    walletProvisionInFlightRef.current = false;
    sessionIdRef.current = "";
    latestSessionStatusRef.current = "";
    redirectAttemptedRef.current = false;
    addMessage("system", "Logged out. Sign in again to continue.");
  }, [privy, addMessage, clearPolling]);

  const handleUserInput = async (message: string) => {
    addMessage("user", message);
    if (state.phase === "await_identity") {
      addMessage("assistant", "Complete Privy login first.");
      return;
    }
    if (state.phase === "await_objective") {
      await handleObjective(message);
      return;
    }
    if (state.phase === "await_launch_confirmation") {
      if (/^(continue|launch|proceed|yes|y|confirm)$/i.test(message.trim())) {
        await beginLaunchSequence();
      } else {
        addMessage("assistant", "Type continue when ready.");
      }
      return;
    }
    if (state.phase === "provisioning" || state.phase === "ready") {
      addMessage("assistant", "Provisioning in progress or session ready.");
    }
  };

  const handleObjective = async (message: string) => {
    setState((prev) => ({ ...prev, objective: message, phase: "planning" }));
    setStepState("objective", "done", "Objective captured.");
    setStepState("config", "active", "Generating configuration draft...");
    setComposerEnabled(false);
    setChatAction(null);
    try {
      const gatewayAuthKey = generateGatewayAuthKey();
      setState((prev) => ({ ...prev, gatewayAuthKey }));
      const suggestion = await api.suggestConfig({
        wallet_address: state.walletAddress,
        intent: message,
        gateway_auth_key: gatewayAuthKey,
      });
      const config = normalizeConfig(suggestion?.config || {});
      setState((prev) => ({ ...prev, config }));
      setConfigSummary(config);
      setStepState("config", "done", "Config drafted.");
      const decision = deriveDecision(message, config);
      setState((prev) => ({ ...prev, decision }));
      setStepState("decision", "done", decision.title);
      setStepState("signature", "active", "Pending challenge and signature.");
      addMessage("assistant", `Configuration ready. Runtime decision: ${decision.title}. ${decision.reason} Reply continue to issue your challenge and sign.`);
      setState((prev) => ({ ...prev, phase: "await_launch_confirmation" }));
      setComposerEnabled(true);
      setChatAction({ label: "Continue to Signature", onClick: () => beginLaunchSequence() });
    } catch (err) {
      setStepState("config", "error", "Config draft failed.");
      setState((prev) => ({ ...prev, phase: "await_objective" }));
      setComposerEnabled(true);
      addMessage("error", "Failed to draft configuration: " + String((err as Error)?.message || err));
    }
  };

  const normalizeConfig = (c: Record<string, unknown>): Config => {
    const out = { ...c } as Config;
    out.profile_name = (out.profile_name as string) || "ironclaw_profile_" + Date.now();
    out.profile_domain = (out.profile_domain as string) || "general";
    out.custody_mode = (out.custody_mode as string) || "user_wallet";
    out.verification_backend = (out.verification_backend as string) || "eigencloud_primary";
    out.gateway_auth_key = (out.gateway_auth_key as string) || state.gatewayAuthKey;
    out.accept_terms = true;
    out.user_wallet_address = (out.user_wallet_address as string) || state.walletAddress;
    out.symbol_allowlist = (Array.isArray(out.symbol_allowlist) || out.symbol_allowlist) ? out.symbol_allowlist : ["BTC", "ETH"];
    out.enable_memory = out.enable_memory ?? true;
    return out;
  };

  const deriveDecision = (objective: string, config: Config) => {
    const text = objective.toLowerCase();
    const live = /(live|execution|execute|trade|production|autonomous|deploy|24\/7)/.test(text);
    const dedicated = live || String(config.paper_live_policy || "").toLowerCase() === "live_allowed";
    if (dedicated) {
      return { mode: "dedicated", title: "Dedicated Enclaved IronClaw Instance", reason: "Objective indicates continuous or execution-sensitive behavior." };
    }
    return { mode: "shared", title: "Shared Runtime First", reason: "Objective indicates research/planning posture." };
  };

  const beginLaunchSequence = async () => {
    if (!state.config || !state.walletAddress) {
      addMessage("error", "Cannot launch: missing config or wallet.");
      return;
    }
    let signerProvider = isWalletProvider(state.ethereumProvider)
      ? state.ethereumProvider
      : null;
    let chainId = state.chainId;
    if (!signerProvider) {
      const walletContext = await resolveWalletContext(state.walletAddress);
      if (!walletContext?.provider) {
        addMessage(
          "error",
          "Wallet signer unavailable. Open Privy wallet once, then retry launch."
        );
        return;
      }
      signerProvider = walletContext.provider;
      chainId = chainId || walletContext.chainId || "";
      setState((prev) => ({
        ...prev,
        ethereumProvider: walletContext.provider,
        chainId: prev.chainId || walletContext.chainId || "",
      }));
      addMessageIfNew("system", "Wallet signer finalized. Proceeding to challenge.");
    }
    latestSessionStatusRef.current = "";
    redirectAttemptedRef.current = false;
    setState((prev) => ({ ...prev, phase: "launching", redirectAttempted: false }));
    setComposerEnabled(false);
    setChatAction(null);
    setStepState("signature", "active", "Issuing challenge...");
    setStepState("provisioning", "active", "Waiting for verification...");
    try {
      const chainIdNum = parseChainIdNumber(chainId);
      const challenge = await api.challenge({
        wallet_address: state.walletAddress,
        privy_user_id: state.privyUserId || null,
        chain_id: chainIdNum,
      });
      const sessionId = String(challenge.session_id || "");
      const challengeMessage = String(challenge.message || "");
      sessionIdRef.current = sessionId;
      setState((prev) => ({ ...prev, sessionId, challengeMessage }));
      setSession({ session_id: sessionId, status: "challenge_issued", provisioning_source: "pending", runtime_state: "pending" });
      await ensureOnboardingReady(sessionId, state.config, state.objective);
      const signature = await signMessage(challengeMessage, signerProvider);
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
      setState((prev) => ({ ...prev, phase: "provisioning" }));
      startPolling();
    } catch (err) {
      setStepState("signature", "error", "Challenge/signature failed.");
      setStepState("provisioning", "error", "Provisioning did not start.");
      setState((prev) => ({ ...prev, phase: "await_launch_confirmation" }));
      setComposerEnabled(true);
      setChatAction({ label: "Retry Signature", onClick: () => beginLaunchSequence() });
      addMessage("error", "Launch failed: " + String((err as Error)?.message || err));
    }
  };

  const ensureOnboardingReady = async (sessionId: string, config: Config, objective: string) => {
    let onboardingState = await api.getOnboardingState(sessionId);
    const objectiveText = objective.trim() || `Launch profile ${config.profile_name || "frontdoor_profile"} with deterministic verification.`;
    const assignments = `profile_name=${String(config.profile_name || "frontdoor_profile").replace(/[\n\r,;=]/g, "_")}, gateway_auth_key=__from_config__, accept_terms=true`;
    if (!onboardingState.objective) {
      onboardingState = await api.postOnboardingChat(sessionId, objectiveText);
    }
    if (Array.isArray(onboardingState.missing_fields) && onboardingState.missing_fields.length > 0) {
      onboardingState = await api.postOnboardingChat(sessionId, assignments);
    }
    if (onboardingState.current_step !== "ready_to_sign" && !onboardingState.completed) {
      onboardingState = await api.postOnboardingChat(sessionId, "confirm plan");
    }
    if (Array.isArray(onboardingState.missing_fields) && onboardingState.missing_fields.length > 0) {
      onboardingState = await api.postOnboardingChat(sessionId, assignments);
      onboardingState = await api.postOnboardingChat(sessionId, "confirm plan");
    }
    if (Array.isArray(onboardingState.missing_fields) && onboardingState.missing_fields.length > 0) {
      throw new Error("Onboarding required variables unresolved: " + onboardingState.missing_fields.join(", "));
    }
    if (onboardingState.current_step !== "ready_to_sign" && !onboardingState.completed) {
      onboardingState = await api.postOnboardingChat(sessionId, "confirm sign");
    }
    if (onboardingState.current_step !== "ready_to_sign" && !onboardingState.completed) {
      throw new Error("Onboarding did not reach ready_to_sign state.");
    }
  };

  const signMessage = async (
    message: string,
    providerOverride?: WalletProvider | null
  ): Promise<string> => {
    const wallet = normalizeWallet(state.walletAddress);
    if (!wallet) throw new Error("Wallet address unavailable.");
    const provider = providerOverride ?? state.ethereumProvider;
    if (!isWalletProvider(provider))
      throw new Error("Wallet provider unavailable.");
    const hexMsg = "0x" + Array.from(new TextEncoder().encode(message)).map((b) => b.toString(16).padStart(2, "0")).join("");
    const attempts: [string, string][] = [
      [hexMsg, wallet],
      [message, wallet],
      [wallet, hexMsg],
      [wallet, message],
    ];
    for (const params of attempts) {
      try {
        const sig = await provider.request({ method: "personal_sign", params });
        if (typeof sig === "string" && sig) return sig;
      } catch (_) {}
    }
    throw new Error("Wallet signature failed.");
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const v = inputValue.trim();
    if (!v) return;
    setInputValue("");
    handleUserInput(v);
  };

  const showIdentityButton = ready && (!authenticated || !walletAddress);

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
              <div key={i} className={`lp-msg ${m.role}`}>
                {m.text}
              </div>
            ))}
          </div>
          <div className="lp-chat-actions">
            {showIdentityButton && (
              <button type="button" className="lp-action-btn" onClick={() => login()}>
                Sign Up / Connect Wallet
              </button>
            )}
            {authenticated && (
              <button type="button" className="lp-action-btn" onClick={handleLogout}>
                Logout
              </button>
            )}
            {chatAction && (
              <button type="button" className="lp-action-btn" onClick={chatAction.onClick}>
                {chatAction.label}
              </button>
            )}
          </div>
          <form className="lp-chat-compose" onSubmit={handleSubmit}>
            <textarea
              rows={1}
              placeholder="Describe what you want this agent to do..."
              disabled={!composerEnabled}
              value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
            />
            <button type="submit" disabled={!composerEnabled}>
              Send
            </button>
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
                    <p className="lp-step-title">
                      {step === "identity" && "1. Privy Signup"}
                      {step === "objective" && "2. Objective"}
                      {step === "config" && "3. Config Draft"}
                      {step === "decision" && "4. Runtime Decision"}
                      {step === "signature" && "5. Signature"}
                      {step === "provisioning" && "6. Provisioning"}
                    </p>
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
              <div className="lp-kv-row"><span>Status</span><strong>{session.status || "-"}</strong></div>
              <div className="lp-kv-row"><span>Provisioning Source</span><strong>{session.provisioning_source || "-"}</strong></div>
              <div className="lp-kv-row"><span>Runtime</span><strong>{session.runtime_state || "-"}</strong></div>
            </div>
            <div className="lp-links">
              {session.instance_url && (
                <a href={session.instance_url} target="_blank" rel="noopener noreferrer">Open Runtime</a>
              )}
              {session.verify_url && (
                <a href={session.verify_url} target="_blank" rel="noopener noreferrer">Open Verify</a>
              )}
            </div>
          </section>
        </aside>
      </section>
    </main>
    </>
  );
}
