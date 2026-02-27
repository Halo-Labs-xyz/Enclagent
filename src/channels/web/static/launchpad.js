const PRIVY_SDK_IMPORT_URL = "https://cdn.jsdelivr.net/npm/@privy-io/js-sdk-core@0.60.3/+esm";
const PRIVY_REACT_AUTH_IMPORT_URL = "https://cdn.jsdelivr.net/npm/@privy-io/react-auth@3.15.0/+esm";
const REACT_IMPORT_URL = "https://cdn.jsdelivr.net/npm/react@19.2.4/+esm";
const REACT_DOM_CLIENT_IMPORT_URL = "https://cdn.jsdelivr.net/npm/react-dom@19.2.4/client/+esm";

const steps = [
  "identity",
  "objective",
  "config",
  "decision",
  "signature",
  "provisioning",
];

const state = {
  phase: "await_identity",
  bootstrap: null,
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
  privyClient: null,
  privyWallet: null,
  ethereumProvider: null,
  walletProviderSource: "",
  pollingTimer: null,
  latestSessionStatus: "",
};

const el = {
  environmentPill: document.getElementById("environment-pill"),
  chatStream: document.getElementById("chat-stream"),
  chatActions: document.getElementById("chat-actions"),
  chatForm: document.getElementById("chat-form"),
  chatInput: document.getElementById("chat-input"),
  chatSend: document.getElementById("chat-send"),
  connectWalletBtn: document.getElementById("connect-wallet-btn"),
  identityMeta: document.getElementById("identity-meta"),
  configSummary: document.getElementById("config-summary"),
  sessionSummary: document.getElementById("session-summary"),
  sessionLinks: document.getElementById("session-links"),
  privyRoot: document.getElementById("privy-connect-root"),
};

window.__enclagentPrivyLoginComplete = enclagentPrivyLoginComplete;
window.__enclagentPrivyLogout = handlePrivyLogout;

window.addEventListener("error", (event) => {
  const filename = String(event && event.filename ? event.filename : "");
  const message = String(event && event.message ? event.message : "");
  if (
    filename.includes("evmAsk.js") ||
    message.includes("Cannot destructure property 'keccak_256'")
  ) {
    event.preventDefault();
  }
});

if (!handlePrivyOauthPopupCallbackWindow()) {
  document.addEventListener("DOMContentLoaded", () => {
    bindEvents();
    initialize().catch((err) => {
      addMessage(
        "error",
        "Failed to initialize launchpad: " +
          String(err && err.message ? err.message : err)
      );
    });
  });
}

function bindEvents() {
  el.chatForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const value = String(el.chatInput.value || "").trim();
    if (!value) return;
    el.chatInput.value = "";
    resizeComposer();
    await handleUserInput(value);
  });

  el.chatInput.addEventListener("input", resizeComposer);

  el.connectWalletBtn.addEventListener("click", async () => {
    await withBusyButton(el.connectWalletBtn, async () => {
      await connectWalletIdentity(await maybeGetPrivyClient());
    });
  });
}

async function initialize() {
  renderEnvironment();
  setStepState("identity", "active", "Waiting for Privy login.");
  setComposerEnabled(false);

  addMessage(
    "assistant",
    "Welcome. First step: sign up or log in with Privy and connect your wallet. I will not provision anything until identity and configuration are complete."
  );

  const bootstrap = await fetchJson("/api/frontdoor/bootstrap");
  state.bootstrap = bootstrap;

  if (!bootstrap.enabled) {
    addMessage(
      "error",
      "Launchpad is disabled for this deployment. Enable GATEWAY_FRONTDOOR_ENABLED=true."
    );
    return;
  }

  if (bootstrap.require_privy && !String(bootstrap.privy_app_id || "").trim()) {
    addMessage(
      "error",
      "Privy is required but no Privy App ID is configured."
    );
    return;
  }

  mountPrivyEntryControls();
}

function renderEnvironment() {
  const host = String(window.location.hostname || "").toLowerCase();
  if (!host || host === "localhost" || host === "127.0.0.1") {
    el.environmentPill.textContent = "Local";
    return;
  }
  if (host.includes("stage") || host.includes("staging") || host.includes("-stg")) {
    el.environmentPill.textContent = "Staging";
    return;
  }
  el.environmentPill.textContent = "Production";
}

function mountPrivyEntryControls() {
  if (!el.privyRoot) {
    el.connectWalletBtn.classList.remove("hidden");
    return;
  }

  const appId = String(
    (state.bootstrap && state.bootstrap.privy_app_id) || ""
  ).trim();
  if (!appId) {
    el.connectWalletBtn.classList.remove("hidden");
    el.identityMeta.textContent = "Privy App ID not configured; wallet-only mode enabled.";
    return;
  }

  el.connectWalletBtn.classList.add("hidden");
  el.privyRoot.innerHTML = "<div class=\"privy-hint\">Loading Privy login...</div>";

  mountPrivyReactConnectButton(appId).catch((err) => {
    addMessage(
      "system",
      "Privy UI unavailable in this browser. Fallback wallet connect enabled."
    );
    addMessage("error", errorMessage(err));
    el.privyRoot.innerHTML = "";
    el.connectWalletBtn.classList.remove("hidden");
    el.identityMeta.textContent = "Privy failed to load. Use Connect wallet below.";
  });
}

async function mountPrivyReactConnectButton(appId) {
  const [ReactMod, ReactDomMod, PrivyAuthMod] = await Promise.all([
    import(REACT_IMPORT_URL),
    import(REACT_DOM_CLIENT_IMPORT_URL),
    import(PRIVY_REACT_AUTH_IMPORT_URL),
  ]);

  const React = ReactMod.default || ReactMod;
  const ReactDom = ReactDomMod.default || ReactDomMod;
  const createRoot = ReactDom.createRoot;
  const createElement = React.createElement;

  const PrivyProvider =
    PrivyAuthMod.PrivyProvider ||
    (PrivyAuthMod.default && PrivyAuthMod.default.PrivyProvider);
  const useLogin =
    PrivyAuthMod.useLogin ||
    (PrivyAuthMod.default && PrivyAuthMod.default.useLogin);
  const usePrivy =
    PrivyAuthMod.usePrivy ||
    (PrivyAuthMod.default && PrivyAuthMod.default.usePrivy);
  const useWallets =
    PrivyAuthMod.useWallets ||
    (PrivyAuthMod.default && PrivyAuthMod.default.useWallets);

  if (
    typeof createRoot !== "function" ||
    typeof createElement !== "function" ||
    typeof PrivyProvider !== "function" ||
    typeof useLogin !== "function" ||
    typeof usePrivy !== "function" ||
    typeof useWallets !== "function"
  ) {
    throw new Error("Privy React interface exports are unavailable.");
  }

  const clientId = String(
    (state.bootstrap && state.bootstrap.privy_client_id) || ""
  ).trim();

  const ConnectPrivyWallet = function () {
    const privy = usePrivy();
    const walletState = useWallets();
    const wallets =
      walletState && Array.isArray(walletState.wallets)
        ? walletState.wallets
        : [];
    const walletsReady = !!(walletState && walletState.ready);
    const ready = !!(privy && privy.ready);
    const user = privy && privy.user ? privy.user : null;
    const authenticated = !!user;

    const ethereumEmbeddedAddress = React.useMemo(
      function () {
        return user ? getEthereumEmbeddedWalletAddress(user) : "";
      },
      [user]
    );
    const walletAddress =
      ethereumEmbeddedAddress ||
      (user ? extractWalletFromPrivyPayload({ user }) : "") ||
      state.walletAddress;

    const getIdentityToken =
      privy && typeof privy.getIdentityToken === "function"
        ? privy.getIdentityToken.bind(privy)
        : async function () {
            return "";
          };
    const getAccessToken =
      privy && typeof privy.getAccessToken === "function"
        ? privy.getAccessToken.bind(privy)
        : async function () {
            return "";
          };

    const fireLoginComplete = React.useCallback(
      async function (payload) {
        const payloadUser = payload && payload.user ? payload.user : user;
        const builtPayload = {
          user: payloadUser,
          walletAddress:
            payload && payload.walletAddress
              ? payload.walletAddress
              : extractWalletFromPrivyPayload({ user: payloadUser }),
        };
        let identityToken = String((payload && payload.identityToken) || "");
        let accessToken = String((payload && payload.accessToken) || "");
        let resolvedWallet =
          builtPayload.walletAddress || extractWalletFromPrivyPayload(builtPayload);
        let chainId = payload && payload.chainId ? String(payload.chainId) : "";
        if (!identityToken || !accessToken) {
          try {
            identityToken = String((await getIdentityToken()) || "");
            accessToken = String((await getAccessToken()) || "");
          } catch (_) {}
        }
        try {
          const sync = await synchronizePrivyWalletBinding(wallets, resolvedWallet);
          if (sync && sync.walletAddress) {
            resolvedWallet = sync.walletAddress;
          }
          if (sync && sync.chainId) {
            chainId = sync.chainId;
          }
        } catch (_) {}

        if (typeof window.__enclagentPrivyLoginComplete === "function") {
          window.__enclagentPrivyLoginComplete({
            walletAddress: resolvedWallet,
            privyUserId: extractPrivyUserIdFromPayload(builtPayload),
            identityToken,
            accessToken,
            chainId,
            ethereumProvider:
              state.ethereumProvider &&
              typeof state.ethereumProvider.request === "function"
                ? state.ethereumProvider
                : null,
            user: payloadUser,
          });
        }
      },
      [user, wallets, getIdentityToken, getAccessToken]
    );

    React.useEffect(() => {
      if (!walletsReady || wallets.length === 0) return;
      synchronizePrivyWalletBinding(wallets, state.walletAddress).catch(() => {});
    }, [walletsReady, wallets]);

    React.useEffect(() => {
      if (!ready || !authenticated || !user) return;
      fireLoginComplete({ user }).catch(() => {});
    }, [ready, authenticated, user, fireLoginComplete]);

    const { login } = useLogin({
      onComplete: async function (payload) {
        await fireLoginComplete(payload);
      },
      onError: function (err) {
        addMessage("error", "Privy login failed: " + errorMessage(err));
      },
    });

    const logout = privy && typeof privy.logout === "function" ? privy.logout.bind(privy) : null;
    const handleLogout = React.useCallback(
      async function () {
        if (logout) {
          await logout();
          if (typeof window.__enclagentPrivyLogout === "function") {
            window.__enclagentPrivyLogout();
          }
        }
      },
      [logout]
    );

    if (!ready) {
      return createElement(
        "div",
        { className: "privy-connect-loading" },
        "Loading auth..."
      );
    }

    return createElement(
      "div",
      { className: "privy-connect-wallet" },
      walletAddress
        ? createElement(
            "div",
            { className: "privy-wallet-address-block" },
            createElement("div", { className: "privy-wallet-label" }, "Connected wallet"),
            createElement(
              "div",
              { className: "privy-wallet-address", title: walletAddress },
              walletAddress.slice(0, 6) + "\u2022\u2022\u2022" + walletAddress.slice(-4)
            )
          )
        : null,
      !user
        ? createElement(
            "button",
            {
              type: "button",
              className: "privy-auth-btn",
              onClick: function () {
                login();
              },
            },
            "Connect Wallet"
          )
        : createElement(
            "button",
            {
              type: "button",
              className: "privy-logout-btn",
              onClick: handleLogout,
            },
            "Disconnect"
          )
    );
  };

  const providerConfig = {
    appId,
    config: {
      loginMethods: ["wallet", "email", "google"],
      embeddedWallets: {
        ethereum: { createOnLogin: "all-users" },
      },
      appearance: {
        theme: "dark",
      },
    },
  };
  if (clientId) {
    providerConfig.clientId = clientId;
  }

  el.privyRoot.innerHTML = "";
  const root = createRoot(el.privyRoot);
  root.render(
    createElement(PrivyProvider, providerConfig, createElement(ConnectPrivyWallet))
  );
  el.identityMeta.textContent = "Use Privy login to continue.";
}

async function connectPrivyEmailIdentity(emailInputValue) {
  const client = await getPrivyClient();
  if (!client || !client.auth || !client.auth.email) {
    throw new Error("Privy email login is unavailable.");
  }

  const email = String(emailInputValue || "").trim();
  if (!email || !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email)) {
    throw new Error("Enter a valid email address.");
  }

  addMessage("system", "Sending email verification code...");
  await client.auth.email.sendCode(email);

  const code = String(
    window.prompt("Enter the one-time code sent to " + email + ":", "")
  ).trim();
  if (!code) {
    throw new Error("Email verification code is required.");
  }

  const authResponse = await client.auth.email.loginWithCode(
    email,
    code,
    "login-or-sign-up"
  );
  await applyPrivyAuthResponse(client, authResponse, "Email login complete");
}

async function connectPrivyOauthIdentity(provider) {
  const normalizedProvider = String(provider || "").trim().toLowerCase();
  if (!normalizedProvider) {
    throw new Error("Invalid OAuth provider.");
  }

  const client = await getPrivyClient();
  if (!client || !client.auth || !client.auth.oauth) {
    throw new Error("Privy social login is unavailable.");
  }

  addMessage("system", "Opening " + providerLabel(normalizedProvider) + " login...");
  const authResponse = await loginWithPrivyOauthPopup(client, normalizedProvider);
  await applyPrivyAuthResponse(
    client,
    authResponse,
    providerLabel(normalizedProvider) + " login complete"
  );
}

function isEvmWalletCandidate(wallet) {
  if (!wallet || typeof wallet !== "object") return false;
  if (wallet.type === "ethereum" || wallet.chainType === "ethereum") return true;
  if (typeof wallet.getEthereumProvider === "function") return true;
  return !!normalizeWallet(wallet.address);
}

function normalizeChainIdString(value) {
  if (value == null) return "";
  if (typeof value === "number" && Number.isFinite(value)) {
    return "0x" + value.toString(16);
  }
  const raw = String(value).trim();
  if (!raw) return "";
  if (raw.startsWith("eip155:")) {
    const numeric = parseInt(raw.slice("eip155:".length), 10);
    if (!Number.isFinite(numeric)) return "";
    return "0x" + numeric.toString(16);
  }
  if (raw.startsWith("0x")) {
    const numeric = parseInt(raw, 16);
    if (!Number.isFinite(numeric)) return "";
    return "0x" + numeric.toString(16);
  }
  const numeric = parseInt(raw, 10);
  if (!Number.isFinite(numeric)) return "";
  return "0x" + numeric.toString(16);
}

function selectPrivyWallet(wallets, preferredWalletAddress) {
  if (!Array.isArray(wallets) || wallets.length === 0) {
    return null;
  }
  const target = normalizeWallet(preferredWalletAddress);
  let firstCandidate = null;
  for (let i = 0; i < wallets.length; i += 1) {
    const wallet = wallets[i];
    if (!isEvmWalletCandidate(wallet)) continue;
    if (!firstCandidate) firstCandidate = wallet;
    if (!target) continue;
    const address = normalizeWallet(wallet.address);
    if (address && address === target) {
      return wallet;
    }
  }
  return firstCandidate;
}

async function synchronizePrivyWalletBinding(wallets, preferredWalletAddress) {
  const pool =
    Array.isArray(wallets) && wallets.length > 0
      ? wallets
      : state.privyWallet
        ? [state.privyWallet]
        : [];
  const selected = selectPrivyWallet(pool, preferredWalletAddress);
  if (!selected) {
    return null;
  }

  state.privyWallet = selected;
  const walletAddress = normalizeWallet(selected.address);
  if (walletAddress) {
    state.walletAddress = walletAddress;
  }

  let chainId = normalizeChainIdString(selected.chainId);
  if (typeof selected.getEthereumProvider === "function") {
    try {
      const provider = await selected.getEthereumProvider();
      if (provider && typeof provider.request === "function") {
        state.ethereumProvider = provider;
        state.walletProviderSource = "privy";
        if (!chainId) {
          try {
            const providerChainId = await provider.request({ method: "eth_chainId" });
            chainId = normalizeChainIdString(providerChainId);
          } catch (_) {}
        }
      }
    } catch (_) {}
  }

  if (chainId) {
    state.chainId = chainId;
  }

  return {
    walletAddress: state.walletAddress,
    chainId: state.chainId,
    hasProvider:
      !!(state.ethereumProvider && typeof state.ethereumProvider.request === "function"),
  };
}

async function connectWalletIdentity(client) {
  await synchronizePrivyWalletBinding([], state.walletAddress);

  if (!state.walletAddress || !state.ethereumProvider) {
    if (!window.ethereum) {
      throw new Error("No wallet provider detected in this browser.");
    }
    state.ethereumProvider = window.ethereum;
    state.walletProviderSource = "extension";

    const accounts = await state.ethereumProvider.request({ method: "eth_requestAccounts" });
    if (!accounts || !accounts[0]) {
      throw new Error("Wallet provider did not return an account.");
    }

    const walletAddress = String(accounts[0] || "").trim();
    if (!/^0x[a-fA-F0-9]{40}$/.test(walletAddress)) {
      throw new Error("Wallet provider returned an invalid EVM address.");
    }

    state.walletAddress = walletAddress;
    const extensionChainId = await state.ethereumProvider.request({ method: "eth_chainId" });
    state.chainId = normalizeChainIdString(extensionChainId);
  }

  if (!state.chainId && state.ethereumProvider && typeof state.ethereumProvider.request === "function") {
    try {
      const providerChainId = await state.ethereumProvider.request({ method: "eth_chainId" });
      state.chainId = normalizeChainIdString(providerChainId);
    } catch (_) {}
  }

  if (client) {
    const privySession = await readCurrentPrivySession(client, state.walletAddress);
    state.privyUserId = privySession.privyUserId || buildPrivyWalletHandle(state.walletAddress);
    state.privyIdentityToken = privySession.identityToken || "";
    state.privyAccessToken = privySession.accessToken || "";

    if (privySession.walletLinked) {
      handleIdentityReady("Wallet connected and linked to Privy.");
      return;
    }

    if (!state.privyUserId) {
      state.privyUserId = buildPrivyWalletHandle(state.walletAddress);
    }
    handleIdentityReady("Wallet connected. Privy session initialized.");
    return;
  }

  state.privyUserId = state.privyUserId || buildPrivyWalletHandle(state.walletAddress);
  handleIdentityReady("Wallet connected through browser provider.");
}

function handlePrivyLogout() {
  state.phase = "await_identity";
  state.walletAddress = "";
  state.chainId = "";
  state.privyUserId = "";
  state.privyIdentityToken = "";
  state.privyAccessToken = "";
  state.privyWallet = null;
  state.ethereumProvider = null;
  state.walletProviderSource = "";
  state.objective = "";
  state.config = null;
  state.decision = null;
  state.sessionId = "";
  state.challengeMessage = "";
  state.gatewayAuthKey = "";
  stopProvisioningPolling();
  setStepState("identity", "active", "Waiting for Privy login.");
  setStepState("objective", "pending", "Waiting for your goal.");
  setStepState("config", "pending", "Not generated yet.");
  setStepState("decision", "pending", "No decision yet.");
  setStepState("signature", "pending", "Awaiting challenge issue.");
  setStepState("provisioning", "pending", "No instance yet.");
  setComposerEnabled(false);
  setChatAction(null, null);
  if (el.identityMeta) el.identityMeta.textContent = "Not connected.";
  if (el.configSummary) renderConfigSummary({ profile_name: "Pending", profile_domain: "Pending", custody_mode: "Pending", verification_backend: "Pending", gateway_auth_key: "" });
  if (el.sessionSummary) updateSessionSummary({ session_id: "-", status: "Not started", provisioning_source: "-", runtime_state: "-" });
  addMessage("system", "Logged out. Sign in again to continue.");
}

function enclagentPrivyLoginComplete(payload) {
  const walletAddress = extractWalletFromPrivyPayload(payload);
  const privyUserId =
    extractPrivyUserIdFromPayload(payload) ||
    (walletAddress ? "wallet:" + walletAddress.toLowerCase() : "");
  const identityToken =
    payload && payload.identityToken ? String(payload.identityToken).trim() : "";
  const accessToken =
    payload && payload.accessToken ? String(payload.accessToken).trim() : "";

  state.privyUserId = privyUserId;
  state.privyIdentityToken = identityToken;
  state.privyAccessToken = accessToken;
  if (
    payload &&
    payload.ethereumProvider &&
    typeof payload.ethereumProvider.request === "function"
  ) {
    state.ethereumProvider = payload.ethereumProvider;
    state.walletProviderSource = "privy";
  }

  if (!walletAddress) {
    setStepState(
      "identity",
      "active",
      "Privy authenticated. Wallet binding required before launch."
    );
    el.identityMeta.textContent =
      "Privy user: " +
      (state.privyUserId || "resolved") +
      ". Connect wallet to continue.";
    el.connectWalletBtn.classList.remove("hidden");
    addMessage(
      "assistant",
      "Privy login is complete. Connect your wallet now to continue configuration."
    );
    return;
  }

  state.walletAddress = walletAddress;
  state.chainId =
    payload && payload.chainId != null
      ? normalizeChainIdString(payload.chainId)
      : state.chainId;
  synchronizePrivyWalletBinding([], walletAddress).catch(() => {});
  handleIdentityReady("Privy identity connected.");
}

function extractPrivyUserIdFromPayload(payload) {
  const user =
    payload && payload.user && typeof payload.user === "object"
      ? payload.user
      : null;
  if (user && typeof user.id === "string" && user.id.trim()) {
    return user.id.trim();
  }
  if (user && typeof user.user_id === "string" && user.user_id.trim()) {
    return user.user_id.trim();
  }
  if (user && typeof user.did === "string" && user.did.trim()) {
    return user.did.trim();
  }
  if (payload && typeof payload.privyUserId === "string" && payload.privyUserId.trim()) {
    return payload.privyUserId.trim();
  }
  const loginAccount = payload && payload.loginAccount && typeof payload.loginAccount === "object" ? payload.loginAccount : null;
  if (loginAccount && typeof loginAccount.userId === "string" && loginAccount.userId.trim()) {
    return loginAccount.userId.trim();
  }
  return "";
}

function extractWalletFromPrivyPayload(payload) {
  if (payload && typeof payload.walletAddress === "string") {
    const wallet = payload.walletAddress.trim();
    if (/^0x[a-fA-F0-9]{40}$/.test(wallet)) {
      return wallet;
    }
  }

  const user =
    payload && payload.user && typeof payload.user === "object"
      ? payload.user
      : null;
  if (!user) return "";

  const embeddedAddr = getEthereumEmbeddedWalletAddress(user);
  if (embeddedAddr) return embeddedAddr;

  const candidates = [];
  const addAddress = (account) => {
    if (account && typeof account.address === "string") {
      const addr = String(account.address).trim();
      if (/^0x[a-fA-F0-9]{40}$/.test(addr)) candidates.push(addr);
    }
  };

  if (Array.isArray(user.accounts)) {
    for (let i = 0; i < user.accounts.length; i += 1) {
      addAddress(user.accounts[i]);
    }
  }
  const linked = user.linkedAccounts || user.linked_accounts;
  if (Array.isArray(linked)) {
    for (let i = 0; i < linked.length; i += 1) {
      addAddress(linked[i]);
    }
  }
  if (user.wallet && typeof user.wallet.address === "string") {
    addAddress(user.wallet);
  }
  if (user.wallet_address && typeof user.wallet_address === "string") {
    const addr = String(user.wallet_address).trim();
    if (/^0x[a-fA-F0-9]{40}$/.test(addr)) candidates.push(addr);
  }

  return candidates.length > 0 ? candidates[0] : "";
}

function getEthereumEmbeddedWalletAddress(user) {
  if (!user || typeof user !== "object") return "";
  const linked = user.linkedAccounts || user.linked_accounts;
  if (!Array.isArray(linked)) return "";
  for (let i = 0; i < linked.length; i += 1) {
    const acc = linked[i];
    if (!acc || typeof acc !== "object") continue;
    const isEmbedded =
      acc.type === "wallet" &&
      (acc.walletClientType === "privy" || acc.wallet_client_type === "privy") &&
      (acc.chainType === "ethereum" || acc.chain_type === "ethereum");
    if (isEmbedded && typeof acc.address === "string") {
      const addr = String(acc.address).trim();
      if (/^0x[a-fA-F0-9]{40}$/.test(addr)) return addr;
    }
  }
  return "";
}

async function applyPrivyAuthResponse(client, authResponse, successStatus) {
  let user = extractPrivyUser(authResponse);
  if (!user && client.user && typeof client.user.get === "function") {
    try {
      const userResponse = await client.user.get();
      user = extractPrivyUser(userResponse);
    } catch (_) {}
  }

  const resolvedPrivyUserId = resolvePrivyUserId(user, state.walletAddress || "");
  if (!resolvedPrivyUserId) {
    throw new Error("Privy login did not return a user identifier.");
  }

  state.privyUserId = resolvedPrivyUserId;
  state.privyIdentityToken =
    (authResponse && typeof authResponse.identity_token === "string" && authResponse.identity_token.trim()) ||
    (await readPrivyToken(client, "identity")) ||
    "";
  state.privyAccessToken =
    (authResponse && typeof authResponse.privy_access_token === "string" && authResponse.privy_access_token.trim()) ||
    (await readPrivyToken(client, "access")) ||
    "";

  if (!state.walletAddress) {
    setStepState("identity", "active", "Privy authenticated. Wallet binding required.");
    el.identityMeta.textContent = successStatus + ". Privy user: " + state.privyUserId;
    addMessage(
      "assistant",
      successStatus + ". Now click Sign in with wallets to bind your wallet before configuration starts."
    );
    return;
  }

  handleIdentityReady(successStatus + ". Wallet already bound.");
}

async function readCurrentPrivySession(client, walletAddress) {
  let user = null;
  if (client && client.user && typeof client.user.get === "function") {
    try {
      const userResponse = await client.user.get();
      user = extractPrivyUser(userResponse);
    } catch (_) {
      user = null;
    }
  }

  return {
    privyUserId: resolvePrivyUserId(user, walletAddress),
    identityToken: (await readPrivyToken(client, "identity")) || "",
    accessToken: (await readPrivyToken(client, "access")) || "",
    walletLinked: isWalletLinkedToPrivyUser(user, walletAddress),
  };
}

function isWalletLinkedToPrivyUser(user, walletAddress) {
  const normalizedWallet = normalizeWallet(walletAddress);
  if (!normalizedWallet || !user || typeof user !== "object") {
    return false;
  }

  const linkedAccounts = Array.isArray(user.linked_accounts)
    ? user.linked_accounts
    : Array.isArray(user.linkedAccounts)
      ? user.linkedAccounts
      : [];

  for (let i = 0; i < linkedAccounts.length; i += 1) {
    const account = linkedAccounts[i];
    if (!account || typeof account !== "object") continue;
    const address = typeof account.address === "string" ? normalizeWallet(account.address) : "";
    if (address && address === normalizedWallet) return true;
  }

  return false;
}

async function loginWithPrivyOauthPopup(client, provider) {
  const redirectUri = buildPrivyOauthRedirectUri();
  const initResponse = await client.auth.oauth.generateURL(provider, redirectUri);
  const url = initResponse && typeof initResponse.url === "string" ? initResponse.url : "";
  if (!url) {
    throw new Error("Privy OAuth did not return an authorization URL.");
  }

  const popup = window.open(
    url,
    "enclagent_privy_oauth",
    "width=520,height=700,menubar=no,toolbar=no,location=yes,resizable=yes,scrollbars=yes"
  );
  if (!popup) {
    throw new Error("OAuth popup blocked by browser.");
  }

  const callback = await waitForPrivyOauthCallback(popup, 120000);
  if (callback.error) {
    throw new Error("OAuth failed: " + callback.error);
  }
  if (!callback.code || !callback.state) {
    throw new Error("OAuth callback is missing code/state.");
  }

  return client.auth.oauth.loginWithCode(
    callback.code,
    callback.state,
    provider,
    undefined,
    "login-or-sign-up"
  );
}

function buildPrivyOauthRedirectUri() {
  const url = new URL(window.location.href);
  url.searchParams.set("privy_oauth_callback", "1");
  url.searchParams.delete("code");
  url.searchParams.delete("state");
  url.searchParams.delete("error");
  url.searchParams.delete("error_description");
  url.hash = "";
  return url.toString();
}

async function waitForPrivyOauthCallback(popup, timeoutMs) {
  return new Promise((resolve, reject) => {
    let complete = false;
    const timeout = window.setTimeout(() => {
      cleanup();
      reject(new Error("OAuth callback timed out."));
    }, timeoutMs);
    const popupMonitor = window.setInterval(() => {
      if (popup.closed && !complete) {
        cleanup();
        reject(new Error("OAuth popup was closed before completion."));
      }
    }, 400);

    function cleanup() {
      complete = true;
      window.clearTimeout(timeout);
      window.clearInterval(popupMonitor);
      window.removeEventListener("message", onMessage);
      try {
        popup.close();
      } catch (_) {}
    }

    function onMessage(event) {
      if (event.origin !== window.location.origin) {
        return;
      }
      const payload = event.data;
      if (!payload || payload.source !== "enclagent:privy_oauth_callback") {
        return;
      }
      cleanup();
      resolve({
        code: payload.code && typeof payload.code === "string" ? payload.code.trim() : "",
        state: payload.state && typeof payload.state === "string" ? payload.state.trim() : "",
        error: payload.error && typeof payload.error === "string" ? payload.error.trim() : "",
      });
    }

    window.addEventListener("message", onMessage);
  });
}

function handleIdentityReady(detail) {
  el.identityMeta.textContent =
    detail +
    " Wallet: " +
    state.walletAddress +
    (state.privyUserId ? " | Privy user: " + state.privyUserId : "");

  setStepState("identity", "done", "Privy + wallet identity confirmed.");
  setStepState("objective", "active", "Tell me what you want this agent to do.");

  const firstObjectivePrompt = state.phase !== "await_objective";
  state.phase = "await_objective";
  setComposerEnabled(true);
  if (firstObjectivePrompt) {
    addMessage(
      "assistant",
      "Identity confirmed. Tell me what you want the agent to do. I will draft configuration, show the runtime decision, then wait for your explicit launch confirmation."
    );
  }
}

async function getPrivyClient() {
  if (state.privyClient) {
    return state.privyClient;
  }

  const appId = String(state.bootstrap && state.bootstrap.privy_app_id || "").trim();
  if (!appId) {
    throw new Error("Privy App ID is required for identity authentication.");
  }

  let sdk;
  try {
    sdk = await import(PRIVY_SDK_IMPORT_URL);
  } catch (err) {
    throw new Error("Failed to load Privy SDK: " + String(err && err.message ? err.message : err));
  }

  const PrivyClient = sdk && sdk.default;
  const LocalStorage = sdk && sdk.LocalStorage;
  if (typeof PrivyClient !== "function" || typeof LocalStorage !== "function") {
    throw new Error("Privy SDK did not expose required client constructors.");
  }

  const options = {
    appId,
    storage: new LocalStorage(),
  };

  const clientId = String(state.bootstrap && state.bootstrap.privy_client_id || "").trim();
  if (clientId) {
    options.clientId = clientId;
  }

  const client = new PrivyClient(options);
  await client.initialize();
  state.privyClient = client;
  return client;
}

async function maybeGetPrivyClient() {
  const appId = String(state.bootstrap && state.bootstrap.privy_app_id || "").trim();
  if (!appId) return null;
  try {
    return await getPrivyClient();
  } catch (err) {
    addMessage("system", "Privy client unavailable: " + errorMessage(err));
    return null;
  }
}

function extractPrivyUser(payload) {
  if (!payload || typeof payload !== "object") return null;
  if (payload.user && typeof payload.user === "object") {
    return payload.user;
  }
  return null;
}

function resolvePrivyUserId(user, walletAddress) {
  if (user && typeof user.id === "string" && user.id.trim()) {
    return user.id.trim();
  }
  if (user && typeof user.user_id === "string" && user.user_id.trim()) {
    return user.user_id.trim();
  }
  return buildPrivyWalletHandle(walletAddress);
}

async function readPrivyToken(client, tokenType) {
  try {
    if (tokenType === "identity" && typeof client.getIdentityToken === "function") {
      return await client.getIdentityToken();
    }
    if (tokenType === "access" && typeof client.getAccessToken === "function") {
      return await client.getAccessToken();
    }
    return null;
  } catch (_) {
    return null;
  }
}

function buildPrivyWalletHandle(walletAddress) {
  const normalized = normalizeWallet(walletAddress);
  if (!normalized) return "";
  return "wallet:" + normalized;
}

function normalizeWallet(value) {
  const wallet = String(value || "").trim();
  if (!/^0x[a-fA-F0-9]{40}$/.test(wallet)) {
    return "";
  }
  return wallet.toLowerCase();
}

function providerLabel(provider) {
  const p = String(provider || "").toLowerCase();
  if (p === "google") return "Google";
  if (p === "apple") return "Apple";
  if (p === "github") return "GitHub";
  if (p === "discord") return "Discord";
  if (p === "twitter") return "X / Twitter";
  return p || "OAuth";
}

async function withBusyButton(button, fn) {
  if (!button) {
    await fn();
    return;
  }

  const prev = button.textContent;
  button.disabled = true;
  button.textContent = "Working...";
  try {
    await fn();
  } catch (err) {
    addMessage("error", errorMessage(err));
  } finally {
    button.disabled = false;
    button.textContent = prev;
  }
}

function addMessage(role, text) {
  const row = document.createElement("div");
  row.className = "lp-msg " + role;
  row.textContent = String(text || "");
  el.chatStream.appendChild(row);
  el.chatStream.scrollTop = el.chatStream.scrollHeight;
}

function setChatAction(label, handler) {
  el.chatActions.innerHTML = "";
  if (!label || typeof handler !== "function") return;
  const button = document.createElement("button");
  button.type = "button";
  button.className = "lp-action-btn";
  button.textContent = label;
  button.addEventListener("click", handler);
  el.chatActions.appendChild(button);
}

function setComposerEnabled(enabled) {
  el.chatInput.disabled = !enabled;
  el.chatSend.disabled = !enabled;
  if (enabled) {
    el.chatInput.focus();
  }
}

function resizeComposer() {
  el.chatInput.style.height = "auto";
  el.chatInput.style.height = Math.min(el.chatInput.scrollHeight, 160) + "px";
}

function setStepState(step, status, detail) {
  const node = document.querySelector('.lp-step[data-step="' + step + '"]');
  const detailNode = document.getElementById("step-desc-" + step);
  if (!node || !detailNode) return;

  node.classList.remove("is-active", "is-done", "is-error");
  if (status === "active") node.classList.add("is-active");
  if (status === "done") node.classList.add("is-done");
  if (status === "error") node.classList.add("is-error");
  detailNode.textContent = detail;
}

async function handleUserInput(message) {
  addMessage("user", message);

  if (state.phase === "await_identity") {
    addMessage(
      "assistant",
      "Complete Privy login first. Once your wallet is connected I will ask for your objective."
    );
    return;
  }

  if (state.phase === "await_objective") {
    await handleObjective(message);
    return;
  }

  if (state.phase === "await_launch_confirmation") {
    if (looksAffirmative(message)) {
      await beginLaunchSequence();
    } else {
      addMessage(
        "assistant",
        "Type continue when you are ready. Provisioning will only start after challenge signature."
      );
    }
    return;
  }

  if (state.phase === "provisioning") {
    addMessage(
      "assistant",
      "Provisioning is in progress. I will post status updates automatically."
    );
    return;
  }

  if (state.phase === "ready") {
    addMessage(
      "assistant",
      "Session is ready. Use the links in Session Status to open the instance or verification app."
    );
  }
}

function looksAffirmative(message) {
  return /^(continue|launch|proceed|yes|y|confirm)$/i.test(String(message || "").trim());
}

async function handleObjective(message) {
  state.objective = message;
  state.phase = "planning";

  setStepState("objective", "done", "Objective captured.");
  setStepState("config", "active", "Generating configuration draft...");
  setComposerEnabled(false);
  setChatAction(null, null);

  try {
    const gatewayAuthKey = generateGatewayAuthKey();
    state.gatewayAuthKey = gatewayAuthKey;

    const suggestion = await fetchJson("/api/frontdoor/suggest-config", {
      method: "POST",
      body: {
        wallet_address: state.walletAddress,
        intent: message,
        gateway_auth_key: gatewayAuthKey,
      },
    });

    const config = normalizeSuggestedConfig(
      suggestion && suggestion.config ? suggestion.config : {}
    );
    state.config = config;
    renderConfigSummary(config);

    setStepState("config", "done", "Config drafted and validated.");

    const decision = deriveRuntimeDecision(message, config);
    state.decision = decision;

    setStepState("decision", "done", decision.title);
    setStepState("signature", "active", "Pending challenge issuance and signature.");

    addMessage(
      "assistant",
      "Configuration draft is ready. Runtime decision: " +
        decision.title +
        ". " +
        decision.reason +
        " Reply continue to issue your challenge and sign."
    );

    if (Array.isArray(suggestion.assumptions) && suggestion.assumptions.length > 0) {
      addMessage("system", "Assumptions: " + suggestion.assumptions.join(" | "));
    }
    if (Array.isArray(suggestion.warnings) && suggestion.warnings.length > 0) {
      addMessage("system", "Warnings: " + suggestion.warnings.join(" | "));
    }

    state.phase = "await_launch_confirmation";
    setComposerEnabled(true);
    setChatAction("Continue to Signature", async () => {
      await beginLaunchSequence();
    });
  } catch (err) {
    setStepState("config", "error", "Config draft failed.");
    state.phase = "await_objective";
    setComposerEnabled(true);
    addMessage("error", "Failed to draft configuration: " + errorMessage(err));
  }
}

function normalizeSuggestedConfig(config) {
  const out = Object.assign({}, config);

  if (!out.profile_name) {
    out.profile_name = "ironclaw_profile_" + Date.now();
  }
  if (!out.profile_domain) {
    out.profile_domain = "general";
  }
  if (!out.custody_mode) {
    out.custody_mode = "user_wallet";
  }
  if (!out.verification_backend) {
    out.verification_backend = "eigencloud_primary";
  }

  out.gateway_auth_key = out.gateway_auth_key || state.gatewayAuthKey;
  out.accept_terms = true;
  out.user_wallet_address = out.user_wallet_address || state.walletAddress;

  if (
    (out.custody_mode === "operator_wallet" || out.custody_mode === "dual_mode") &&
    !out.operator_wallet_address
  ) {
    out.operator_wallet_address = state.walletAddress;
  }

  if (!Array.isArray(out.symbol_allowlist) || out.symbol_allowlist.length === 0) {
    out.symbol_allowlist = ["BTC", "ETH"];
  }

  if (!Object.prototype.hasOwnProperty.call(out, "enable_memory")) {
    out.enable_memory = true;
  }

  return out;
}

function renderConfigSummary(config) {
  const rows = [
    ["Profile", config.profile_name || "-"],
    ["Domain", config.profile_domain || "-"],
    ["Custody", config.custody_mode || "-"],
    ["Verification", config.verification_backend || "-"],
    ["Gateway Auth Key", maskKey(config.gateway_auth_key || "")],
  ];

  el.configSummary.innerHTML = "";
  for (let i = 0; i < rows.length; i += 1) {
    const row = document.createElement("div");
    row.className = "lp-kv-row";

    const left = document.createElement("span");
    left.textContent = rows[i][0];
    row.appendChild(left);

    const right = document.createElement("strong");
    right.textContent = String(rows[i][1]);
    row.appendChild(right);

    el.configSummary.appendChild(row);
  }
}

function deriveRuntimeDecision(objective, config) {
  const text = String(objective || "").toLowerCase();
  const livePattern = /(live|execution|execute|trade|production|autonomous|deploy|24\/7)/;
  const lightPattern = /(research|analysis|read-only|explore|prototype|draft|planning|test)/;

  let dedicated = livePattern.test(text);
  if (!dedicated && lightPattern.test(text)) {
    dedicated = false;
  }

  if (String(config.paper_live_policy || "").toLowerCase() === "live_allowed") {
    dedicated = true;
  }

  if (dedicated) {
    return {
      mode: "dedicated",
      title: "Dedicated Enclaved IronClaw Instance",
      reason:
        "Objective indicates continuous or execution-sensitive behavior, so dedicated isolation is selected.",
    };
  }

  return {
    mode: "shared",
    title: "Shared Runtime First",
    reason:
      "Objective indicates research/planning posture. Shared runtime fallback is preferred unless policy escalates.",
  };
}

async function beginLaunchSequence() {
  if (!state.config) {
    addMessage("error", "Cannot launch because configuration is missing.");
    return;
  }
  if (!state.walletAddress) {
    addMessage("error", "Cannot launch because wallet identity is missing.");
    return;
  }

  state.phase = "launching";
  setComposerEnabled(false);
  setChatAction(null, null);

  setStepState("signature", "active", "Issuing challenge and preparing signature.");
  setStepState("provisioning", "active", "Waiting for verification and provisioning start.");

  try {
    const challenge = await fetchJson("/api/frontdoor/challenge", {
      method: "POST",
      body: {
        wallet_address: state.walletAddress,
        privy_user_id: state.privyUserId || null,
        chain_id: parseChainId(state.chainId),
      },
    });

    state.sessionId = String(challenge.session_id || "");
    state.challengeMessage = String(challenge.message || "");

    updateSessionSummary({
      session_id: state.sessionId,
      status: "challenge_issued",
      detail: "Awaiting onboarding confirmations and wallet signature.",
      provisioning_source: "pending",
      runtime_state: "pending",
    });

    await ensureOnboardingReadyForLaunch(state.sessionId, state.config, state.objective);

    const signature = await signMessage(state.challengeMessage, state.walletAddress);

    await fetchJson("/api/frontdoor/verify", {
      method: "POST",
      body: {
        session_id: state.sessionId,
        wallet_address: state.walletAddress,
        privy_user_id: state.privyUserId || null,
        privy_identity_token: state.privyIdentityToken || null,
        privy_access_token: state.privyAccessToken || null,
        message: state.challengeMessage,
        signature,
        config: state.config,
      },
    });

    setStepState("signature", "done", "Signature accepted.");
    setStepState("provisioning", "active", "Provisioning started. Polling status...");

    addMessage(
      "assistant",
      "Signature verified. Provisioning has started. I will post state transitions until your instance is ready."
    );

    state.phase = "provisioning";
    startProvisioningPolling();
  } catch (err) {
    setStepState("signature", "error", "Challenge/signature flow failed.");
    setStepState("provisioning", "error", "Provisioning did not start.");
    state.phase = "await_launch_confirmation";
    setComposerEnabled(true);
    setChatAction("Retry Signature", async () => {
      await beginLaunchSequence();
    });
    addMessage("error", "Launch failed: " + errorMessage(err));
  }
}

async function ensureOnboardingReadyForLaunch(sessionId, config, objective) {
  const normalizedSessionId = normalizeSessionId(sessionId);
  const objectiveText =
    String(objective || "").trim() || buildObjectiveFallback(config);
  const assignments = buildOnboardingAssignmentsMessage(config);

  let onboardingState = await fetchJson(
    "/api/frontdoor/onboarding/state?session_id=" +
      encodeURIComponent(normalizedSessionId)
  );

  if (!onboardingState.objective) {
    onboardingState = await postOnboardingChatMessage(
      normalizedSessionId,
      objectiveText
    );
  }
  if (
    Array.isArray(onboardingState.missing_fields) &&
    onboardingState.missing_fields.length > 0
  ) {
    onboardingState = await postOnboardingChatMessage(
      normalizedSessionId,
      assignments
    );
  }
  if (onboardingState.current_step !== "ready_to_sign" && !onboardingState.completed) {
    onboardingState = await postOnboardingChatMessage(
      normalizedSessionId,
      "confirm plan"
    );
  }
  if (
    Array.isArray(onboardingState.missing_fields) &&
    onboardingState.missing_fields.length > 0
  ) {
    onboardingState = await postOnboardingChatMessage(
      normalizedSessionId,
      assignments
    );
    onboardingState = await postOnboardingChatMessage(
      normalizedSessionId,
      "confirm plan"
    );
  }
  if (
    Array.isArray(onboardingState.missing_fields) &&
    onboardingState.missing_fields.length > 0
  ) {
    throw new Error(
      "Onboarding required variables unresolved: " +
        onboardingState.missing_fields.join(", ")
    );
  }
  if (onboardingState.current_step !== "ready_to_sign" && !onboardingState.completed) {
    onboardingState = await postOnboardingChatMessage(
      normalizedSessionId,
      "confirm sign"
    );
  }
  if (onboardingState.current_step !== "ready_to_sign" && !onboardingState.completed) {
    throw new Error("Onboarding did not reach ready_to_sign state.");
  }
}

async function postOnboardingChatMessage(sessionId, message) {
  const payload = await fetchJson("/api/frontdoor/onboarding/chat", {
    method: "POST",
    body: {
      session_id: sessionId,
      message,
    },
  });

  if (
    !payload ||
    !payload.state ||
    String(payload.state.session_id || "") !== String(sessionId)
  ) {
    throw new Error("Onboarding session mismatch.");
  }
  return payload.state;
}

function buildObjectiveFallback(config) {
  return (
    "Launch profile " +
    String(config.profile_name || "frontdoor_profile") +
    " with deterministic verification, strict policy gates, and auditable runtime controls."
  );
}

function buildOnboardingAssignmentsMessage(config) {
  const profileName = String(config.profile_name || "frontdoor_profile")
    .trim()
    .replace(/[\n\r,;=]/g, "_");
  const acceptTerms = config.accept_terms ? "true" : "false";

  return (
    "profile_name=" +
    profileName +
    ", gateway_auth_key=__from_config__, accept_terms=" +
    acceptTerms
  );
}

function startProvisioningPolling() {
  stopProvisioningPolling();
  const interval = Math.max(
    1200,
    Number(state.bootstrap && state.bootstrap.poll_interval_ms) || 1500
  );
  state.pollingTimer = window.setInterval(async () => {
    try {
      await pollSessionStatus();
    } catch (err) {
      addMessage("system", "Status polling issue: " + errorMessage(err));
    }
  }, interval);
  pollSessionStatus().catch(() => {});
}

function stopProvisioningPolling() {
  if (state.pollingTimer) {
    window.clearInterval(state.pollingTimer);
    state.pollingTimer = null;
  }
}

async function pollSessionStatus() {
  if (!state.sessionId) return;

  const session = await fetchJson(
    "/api/frontdoor/session/" + encodeURIComponent(state.sessionId)
  );

  updateSessionSummary(session);

  if (session.status !== state.latestSessionStatus) {
    state.latestSessionStatus = session.status;
    addMessage(
      "system",
      "Session status: " + session.status + " - " + session.detail
    );
  }

  if (session.status === "ready") {
    stopProvisioningPolling();
    state.phase = "ready";
    setStepState("provisioning", "done", "Instance ready.");
    addMessage(
      "assistant",
      "Your enclave is ready. Use the session links to open the runtime and verification surface."
    );
    setComposerEnabled(false);
    return;
  }

  if (
    session.status === "failed" ||
    session.status === "error" ||
    session.status === "verification_failed"
  ) {
    stopProvisioningPolling();
    state.phase = "await_launch_confirmation";
    setStepState("provisioning", "error", "Provisioning failed.");
    addMessage(
      "error",
      "Provisioning failed: " +
        String(session.error || session.detail || "Unknown")
    );
    setComposerEnabled(true);
    setChatAction("Retry Launch", async () => {
      await beginLaunchSequence();
    });
  }
}

function updateSessionSummary(session) {
  const rows = [
    ["Session", session.session_id || "-"],
    ["Status", session.status || "-"],
    ["Provisioning Source", session.provisioning_source || "-"],
    ["Runtime", session.runtime_state || "-"],
  ];

  el.sessionSummary.innerHTML = "";
  for (let i = 0; i < rows.length; i += 1) {
    const row = document.createElement("div");
    row.className = "lp-kv-row";

    const left = document.createElement("span");
    left.textContent = rows[i][0];
    row.appendChild(left);

    const right = document.createElement("strong");
    right.textContent = String(rows[i][1]);
    row.appendChild(right);

    el.sessionSummary.appendChild(row);
  }

  el.sessionLinks.innerHTML = "";
  if (session.instance_url) {
    el.sessionLinks.appendChild(linkChip("Open Runtime", session.instance_url));
  }
  if (session.verify_url) {
    el.sessionLinks.appendChild(linkChip("Open Verify", session.verify_url));
  }

  if (session.status === "ready") {
    setStepState("provisioning", "done", "Instance ready.");
  }
}

function linkChip(label, href) {
  const anchor = document.createElement("a");
  anchor.href = href;
  anchor.target = "_blank";
  anchor.rel = "noopener noreferrer";
  anchor.textContent = label;
  return anchor;
}

function maskKey(value) {
  const raw = String(value || "");
  if (!raw) return "-";
  if (raw.length <= 8) return raw;
  return raw.slice(0, 4) + "..." + raw.slice(-4);
}

function normalizeSessionId(value) {
  const raw = String(value || "").trim();
  if (!/^[0-9a-fA-F-]{36}$/.test(raw)) {
    throw new Error("Invalid session id.");
  }
  return raw;
}

async function signMessage(message, walletAddress) {
  const targetWallet = normalizeWallet(walletAddress || state.walletAddress);
  if (!targetWallet) {
    throw new Error("Wallet address unavailable for signing.");
  }

  if (!state.ethereumProvider) {
    await synchronizePrivyWalletBinding([], targetWallet);
  }

  if (state.privyWallet && typeof state.privyWallet.sign === "function") {
    try {
      const signed = await state.privyWallet.sign(String(message || ""));
      if (signed && typeof signed === "string") {
        return signed;
      }
    } catch (_) {}
  }

  if (!state.ethereumProvider || typeof state.ethereumProvider.request !== "function") {
    throw new Error("Wallet provider unavailable for signing.");
  }

  const attempts = buildPersonalSignParamAttempts(message, targetWallet);
  let lastErr = null;

  for (let i = 0; i < attempts.length; i += 1) {
    try {
      const signature = await state.ethereumProvider.request({
        method: "personal_sign",
        params: attempts[i],
      });
      if (signature && typeof signature === "string") return signature;
    } catch (err) {
      lastErr = err;
    }
  }

  throw new Error(
    lastErr && lastErr.message ? lastErr.message : "Wallet signature failed."
  );
}

function buildPersonalSignParamAttempts(message, address) {
  const hexMessage = toHexUtf8(message);
  const msg = String(message || "");
  const wallet = String(address || "");

  return [
    [hexMessage, wallet],
    [msg, wallet],
    [wallet, hexMessage],
    [wallet, msg],
  ];
}

function toHexUtf8(value) {
  const bytes = new TextEncoder().encode(String(value || ""));
  let out = "0x";
  for (let i = 0; i < bytes.length; i += 1) {
    out += bytes[i].toString(16).padStart(2, "0");
  }
  return out;
}

function parseChainId(value) {
  const raw = String(value || "").trim();
  if (!raw) return null;
  if (raw.startsWith("0x")) {
    const parsedHex = parseInt(raw, 16);
    return Number.isFinite(parsedHex) ? parsedHex : null;
  }
  const parsed = parseInt(raw, 10);
  return Number.isFinite(parsed) ? parsed : null;
}

function generateGatewayAuthKey() {
  const alphabet = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
  const bytes = new Uint8Array(32);
  window.crypto.getRandomValues(bytes);
  let out = "";
  for (let i = 0; i < bytes.length; i += 1) {
    out += alphabet[bytes[i] % alphabet.length];
  }
  return out;
}

function errorMessage(err) {
  if (!err) return "Unknown error";
  if (typeof err === "string") return err;
  if (err.message && typeof err.message === "string") return err.message;
  return String(err);
}

async function fetchJson(path, options) {
  const opts = options || {};
  opts.headers = opts.headers || {};
  if (opts.body && typeof opts.body === "object") {
    opts.headers["Content-Type"] = "application/json";
    opts.body = JSON.stringify(opts.body);
  }

  const response = await fetch(path, opts);
  let body = null;
  try {
    body = await response.json();
  } catch (_) {
    body = null;
  }

  if (!response.ok) {
    const message =
      body && typeof body.error === "string"
        ? body.error
        : body && typeof body.message === "string"
          ? body.message
          : "Request failed";
    throw new Error(message + " (" + response.status + ")");
  }

  return body;
}

function handlePrivyOauthPopupCallbackWindow() {
  const params = new URLSearchParams(window.location.search);
  if (params.get("privy_oauth_callback") !== "1" || !window.opener) {
    return false;
  }

  window.opener.postMessage(
    {
      source: "enclagent:privy_oauth_callback",
      code: String(params.get("code") || ""),
      state: String(params.get("state") || ""),
      error: String(params.get("error") || params.get("error_description") || ""),
    },
    window.location.origin
  );

  try {
    document.body.innerHTML =
      '<main style="font-family: sans-serif; padding: 24px;">Authentication complete. You can close this window.</main>';
  } catch (_) {}

  window.setTimeout(() => {
    window.close();
  }, 80);

  return true;
}

window.addEventListener("beforeunload", () => {
  stopProvisioningPolling();
});
