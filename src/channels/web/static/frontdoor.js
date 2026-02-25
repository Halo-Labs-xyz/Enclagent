const state = {
  bootstrap: null,
  walletAddress: "",
  chainId: "",
  sessionId: "",
  challengeMessage: "",
  pollTimer: null,
  progress: 6,
  privyUserId: "",
  privyIdentityToken: "",
  privyAccessToken: "",
  ethereumProvider: null,
  monitorSnapshot: null,
};

const el = {
  bootstrapStatus: document.getElementById("bootstrap-status"),
  environmentBadge: document.getElementById("environment-badge"),
  walletAddress: document.getElementById("wallet-address"),
  walletChainId: document.getElementById("wallet-chain-id"),
  privyUserId: document.getElementById("privy-user-id"),
  privyAuthStatus: document.getElementById("privy-auth-status"),
  connectWalletBtn: document.getElementById("connect-wallet-btn"),
  walletError: document.getElementById("wallet-error"),
  configForm: document.getElementById("config-form"),
  launchSessionBtn: document.getElementById("launch-session-btn"),
  suggestConfigBtn: document.getElementById("suggest-config-btn"),
  intentPrompt: document.getElementById("intent-prompt"),
  suggestionMessage: document.getElementById("suggestion-message"),
  suggestionError: document.getElementById("suggestion-error"),
  configError: document.getElementById("config-error"),
  loadingPanel: document.getElementById("loading-panel"),
  loadingTitle: document.getElementById("loading-title"),
  loadingCopy: document.getElementById("loading-copy"),
  loadingError: document.getElementById("loading-error"),
  sessionKv: document.getElementById("session-kv"),
  readyActions: document.getElementById("ready-actions"),
  openInstanceLink: document.getElementById("open-instance-link"),
  openVerifyLink: document.getElementById("open-verify-link"),
  loaderProgressFill: document.getElementById("loader-progress-fill"),
  refreshMonitorBtn: document.getElementById("refresh-monitor-btn"),
  monitorSummary: document.getElementById("monitor-summary"),
  monitorTableWrap: document.getElementById("monitor-table-wrap"),
  monitorTableBody: document.getElementById("monitor-table-body"),
  monitorEmpty: document.getElementById("monitor-empty"),
  monitorError: document.getElementById("monitor-error"),
};

async function main() {
  bindEvents();
  renderEnvironmentBadge();
  try {
    const bootstrap = await fetchJson("/api/frontdoor/bootstrap");
    state.bootstrap = bootstrap;
    if (!bootstrap.enabled) {
      setBootstrapStatus("Frontdoor mode is disabled for this gateway deployment.", "warn");
      disableLaunch("Frontdoor disabled");
      return;
    }
    if (bootstrap.require_privy && !bootstrap.privy_app_id) {
      setBootstrapStatus("Privy is required but no Privy App ID was resolved from gateway environment.", "warn");
      disableLaunch(
        "Missing Privy App ID. Set GATEWAY_FRONTDOOR_PRIVY_APP_ID (or PRIVY_APP_ID / NEXT_PUBLIC_PRIVY_APP_ID)."
      );
      return;
    }
    if (bootstrap.provisioning_backend === "unconfigured") {
      setBootstrapStatus(
        "No provisioning backend configured. Set GATEWAY_FRONTDOOR_PROVISION_COMMAND for per-session enclave deployment.",
        "warn"
      );
      disableProvisioning(
        "Provisioning backend missing. Configure GATEWAY_FRONTDOOR_PROVISION_COMMAND or GATEWAY_FRONTDOOR_DEFAULT_INSTANCE_URL."
      );
      renderMonitorHintOnly();
      return;
    }
    if (bootstrap.require_privy) {
      el.connectWalletBtn.textContent = "Connect Wallet With Privy";
    }
    if (bootstrap.provisioning_backend === "default_instance_url") {
      setBootstrapStatus(
        "Gateway ready in static fallback mode. Sessions will reuse one configured instance URL unless command provisioning is enabled.",
        "warn"
      );
    } else {
      setBootstrapStatus(
        "Gateway ready. Connect wallet, review policy, then sign a gasless authorization transaction.",
        "ok"
      );
    }
    syncWalletLinkedInputs(false);
    syncVerificationControls();
    renderMonitorHintOnly();
  } catch (err) {
    setBootstrapStatus("Failed to load gateway bootstrap.", "warn");
    disableLaunch("Bootstrap unavailable");
    el.walletError.textContent = "Bootstrap failed: " + String(err.message || err);
  }
}

function bindEvents() {
  document.getElementById("custody-mode").addEventListener("change", () => {
    syncWalletLinkedInputs(false);
  });
  document
    .getElementById("verification-fallback-enabled")
    .addEventListener("change", syncVerificationControls);
  if (el.refreshMonitorBtn) {
    el.refreshMonitorBtn.addEventListener("click", refreshSessionMonitor);
  }

  el.connectWalletBtn.addEventListener("click", async () => {
    el.walletError.textContent = "";
    try {
      await connectWalletAndPrivy();
      await refreshSessionMonitor();
    } catch (err) {
      el.walletError.textContent = String(err.message || err);
      setPrivyStatus("Wallet connect failed");
    }
  });

  el.suggestConfigBtn.addEventListener("click", async () => {
    el.suggestionError.textContent = "";
    el.suggestionMessage.textContent = "";
    el.configError.textContent = "";

    if (!state.walletAddress) {
      el.suggestionError.textContent = "Connect wallet first.";
      return;
    }

    const intent = String(el.intentPrompt.value || "").trim();
    if (!intent) {
      el.suggestionError.textContent = "Enter an intent description first.";
      return;
    }

    try {
      el.suggestConfigBtn.disabled = true;
      const suggestion = await fetchJson("/api/frontdoor/suggest-config", {
        method: "POST",
        body: {
          wallet_address: state.walletAddress,
          intent,
          domain: "general",
          gateway_auth_key: optionalValue("gateway-auth-key"),
        },
      });
      applySuggestedConfig(suggestion.config || {});
      const assumptions = Array.isArray(suggestion.assumptions)
        ? suggestion.assumptions
        : [];
      const warnings = Array.isArray(suggestion.warnings) ? suggestion.warnings : [];
      const messages = assumptions.concat(warnings);
      el.suggestionMessage.textContent = messages.length
        ? messages.join(" ")
        : "Suggested config applied. Review fields before launch.";
    } catch (err) {
      el.suggestionError.textContent = String(
        err && err.message ? err.message : err
      );
    } finally {
      el.suggestConfigBtn.disabled = false;
    }
  });

  el.configForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    el.configError.textContent = "";
    el.loadingError.textContent = "";

    if (!state.walletAddress) {
      el.configError.textContent = "Connect wallet first.";
      return;
    }
    if (!state.bootstrap || !state.bootstrap.enabled) {
      el.configError.textContent = "Frontdoor flow is not enabled.";
      return;
    }

    try {
      el.launchSessionBtn.disabled = true;
      const cfg = readConfig();
      const challenge = await fetchJson("/api/frontdoor/challenge", {
        method: "POST",
        body: {
          wallet_address: state.walletAddress,
          privy_user_id: normalizedPrivyId(),
          chain_id: parseChainId(state.chainId),
        },
      });

      state.sessionId = challenge.session_id;
      state.challengeMessage = challenge.message;
      showLoadingPanel();
      advanceLoading("Gasless authorization prepared. Awaiting wallet signature...", 20);

      const signature = await signMessage(challenge.message, state.walletAddress);
      advanceLoading("Signature accepted. Starting enclave provisioning...", 38);

      await fetchJson("/api/frontdoor/verify", {
        method: "POST",
        body: {
          session_id: challenge.session_id,
          wallet_address: state.walletAddress,
          privy_user_id: normalizedPrivyId(),
          privy_identity_token: state.privyIdentityToken || null,
          privy_access_token: state.privyAccessToken || null,
          message: challenge.message,
          signature,
          config: cfg,
        },
      });

      renderSessionKv({
        wallet: state.walletAddress,
        session: challenge.session_id,
        version: challenge.version,
      });
      startPolling();
    } catch (err) {
      el.launchSessionBtn.disabled = false;
      const message = String(err.message || err);
      el.loadingError.textContent = message;
      el.configError.textContent = message;
      el.loadingTitle.textContent = "Provisioning failed";
      el.loadingCopy.textContent = "Fix the configuration and retry.";
    }
  });
}

async function connectWalletAndPrivy() {
  if (!window.ethereum) {
    throw new Error("No EVM wallet provider detected in this browser.");
  }
  state.ethereumProvider = window.ethereum;

  const accounts = await state.ethereumProvider.request({ method: "eth_requestAccounts" });
  if (!accounts || !accounts[0]) {
    throw new Error("Wallet provider did not return an account.");
  }

  const chainId = await state.ethereumProvider.request({ method: "eth_chainId" });
  const walletAddress = String(accounts[0] || "").trim();
  if (!/^0x[a-fA-F0-9]{40}$/.test(walletAddress)) {
    throw new Error("Wallet provider returned an invalid EVM address.");
  }
  const normalizedChainId = await ensureExpectedChain(state.ethereumProvider, chainId);
  state.walletAddress = walletAddress;
  state.chainId = String(normalizedChainId || "");
  el.walletAddress.value = state.walletAddress;
  el.walletChainId.value = state.chainId;
  syncWalletLinkedInputs(true);

  if (state.bootstrap && state.bootstrap.require_privy) {
    state.privyUserId = buildPrivyWalletHandle(state.walletAddress);
    state.privyIdentityToken = "";
    state.privyAccessToken = "";
    el.privyUserId.value = state.privyUserId;
    setPrivyStatus("Wallet connected (Privy mode)");
  } else {
    state.privyUserId = "";
    state.privyIdentityToken = "";
    state.privyAccessToken = "";
    el.privyUserId.value = "";
    setPrivyStatus("Wallet connected");
  }
}

function buildPrivyWalletHandle(walletAddress) {
  const normalized = normalizeOptionalWallet(walletAddress);
  if (!normalized) {
    return "";
  }
  return "wallet:" + normalized;
}

async function signMessage(message, walletAddress) {
  if (!state.ethereumProvider) {
    throw new Error("Wallet provider unavailable for signing.");
  }
  const attempts = buildPersonalSignParamAttempts(message, walletAddress);
  let lastErr = null;
  for (const params of attempts) {
    try {
      const signature = await state.ethereumProvider.request({
        method: "personal_sign",
        params,
      });
      if (signature && typeof signature === "string") {
        return signature;
      }
    } catch (err) {
      lastErr = err;
    }
  }
  throw new Error(lastErr && lastErr.message ? lastErr.message : "Wallet signature failed.");
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

function readConfig() {
  const profileName = getValue("profile-name");
  const hyperliquidNetwork = getValue("hyperliquid-network");
  const paperLivePolicy = getValue("paper-live-policy");
  const hyperliquidApiBaseUrl = optionalValue("hyperliquid-api-base-url");
  const hyperliquidWsUrl = optionalValue("hyperliquid-ws-url");
  const requestTimeoutMs = readInteger("request-timeout-ms", 1000, 120000);
  const maxRetries = readInteger("max-retries", 0, 10);
  const retryBackoffMs = readInteger("retry-backoff-ms", 0, 30000);
  const maxPosition = readInteger("max-position-usd", 1, 10000000);
  const leverageCap = readInteger("leverage-cap", 1, 20);
  const maxLeverage = readInteger("max-leverage", 1, 20);
  const maxAllocation = readInteger("max-allocation-usd", 1, 10000000);
  const perTradeCap = readInteger("per-trade-cap-usd", 1, 10000000);
  const maxSlippage = readInteger("max-slippage-bps", 1, 5000);
  const symbolAllowlist = parseSymbols(getValue("symbol-allowlist"));
  const symbolDenylist = parseSymbols(optionalValue("symbol-denylist") || "");
  const custodyMode = getValue("custody-mode");
  const operatorWalletAddress = normalizeOptionalWallet(optionalValue("operator-wallet-address"));
  const userWalletAddress = normalizeOptionalWallet(optionalValue("user-wallet-address"));
  const vaultAddress = normalizeOptionalWallet(optionalValue("vault-address"));
  const informationSharingScope = getValue("information-sharing-scope");
  const killSwitchEnabled = document.getElementById("kill-switch-enabled").checked;
  const killSwitchBehavior = getValue("kill-switch-behavior");
  const enableMemory = document.getElementById("enable-memory").checked;
  const gatewayAuthKey = getValue("gateway-auth-key");
  const eigencloudAuthKey = optionalValue("eigencloud-auth-key");
  const verificationBackend = getValue("verification-backend");
  const verificationEigencloudEndpoint = optionalValue("verification-eigencloud-endpoint");
  const verificationEigencloudAuthScheme = getValue("verification-eigencloud-auth-scheme");
  const verificationEigencloudTimeoutMs = readInteger(
    "verification-eigencloud-timeout-ms",
    1,
    120000
  );
  const verificationFallbackEnabled = document.getElementById("verification-fallback-enabled").checked;
  const verificationFallbackSigningKeyId = optionalValue("verification-fallback-signing-key-id");
  const verificationFallbackChainPath = optionalValue("verification-fallback-chain-path");
  const verificationFallbackRequireSignedReceipts = document.getElementById(
    "verification-fallback-require-signed"
  ).checked;
  const acceptTerms = document.getElementById("accept-terms").checked;

  if (!profileName) throw new Error("Profile name is required.");
  if (!acceptTerms) throw new Error("Risk acknowledgement is required.");
  if (perTradeCap > maxAllocation) {
    throw new Error("Per-trade cap must be less than or equal to max allocation.");
  }
  if (maxLeverage > leverageCap) {
    throw new Error("Copy max leverage must be less than or equal to leverage cap.");
  }
  if (!symbolAllowlist.length) {
    throw new Error("Symbol allowlist must include at least one market.");
  }
  if (custodyMode !== "operator_wallet" && custodyMode !== "user_wallet" && custodyMode !== "dual_mode") {
    throw new Error("Invalid custody mode.");
  }
  if ((custodyMode === "operator_wallet" || custodyMode === "dual_mode") && !operatorWalletAddress) {
    throw new Error("Operator wallet address is required for operator_wallet/dual_mode.");
  }
  const connectedWallet = normalizeOptionalWallet(state.walletAddress);
  if ((custodyMode === "user_wallet" || custodyMode === "dual_mode") && !connectedWallet) {
    throw new Error("Connected wallet address is required for user_wallet/dual_mode.");
  }
  const effectiveUserWalletAddress = userWalletAddress || connectedWallet;
  if ((custodyMode === "user_wallet" || custodyMode === "dual_mode") && !effectiveUserWalletAddress) {
    throw new Error("User wallet address is required for user_wallet/dual_mode.");
  }
  if ((custodyMode === "user_wallet" || custodyMode === "dual_mode") && effectiveUserWalletAddress !== connectedWallet) {
    throw new Error("User wallet address must match the connected wallet.");
  }
  if (gatewayAuthKey.length < 16 || gatewayAuthKey.length > 128 || /\s/.test(gatewayAuthKey)) {
    throw new Error("Gateway auth key must be 16-128 chars with no whitespace.");
  }
  if (verificationBackend !== "eigencloud_primary" && verificationBackend !== "fallback_only") {
    throw new Error("Invalid verification backend.");
  }
  if (
    verificationEigencloudAuthScheme !== "bearer" &&
    verificationEigencloudAuthScheme !== "api_key"
  ) {
    throw new Error("Invalid verification auth scheme.");
  }
  if (verificationBackend === "fallback_only" && !verificationFallbackEnabled) {
    throw new Error(
      "Fallback must be enabled when verification backend is fallback_only."
    );
  }
  if (
    verificationFallbackChainPath &&
    /[\r\n]/.test(verificationFallbackChainPath)
  ) {
    throw new Error("Fallback chain path must not include newlines.");
  }

  return {
    config_version: 2,
    profile_domain: "general",
    domain_overrides: {},
    inference_summary: null,
    inference_confidence: null,
    inference_warnings: [],
    profile_name: profileName,
    hyperliquid_network: hyperliquidNetwork,
    paper_live_policy: paperLivePolicy,
    hyperliquid_api_base_url: hyperliquidApiBaseUrl,
    hyperliquid_ws_url: hyperliquidWsUrl,
    request_timeout_ms: requestTimeoutMs,
    max_retries: maxRetries,
    retry_backoff_ms: retryBackoffMs,
    max_position_size_usd: maxPosition,
    leverage_cap: leverageCap,
    max_allocation_usd: maxAllocation,
    per_trade_notional_cap_usd: perTradeCap,
    max_leverage: maxLeverage,
    max_slippage_bps: maxSlippage,
    symbol_allowlist: symbolAllowlist,
    symbol_denylist: symbolDenylist,
    custody_mode: custodyMode,
    operator_wallet_address: operatorWalletAddress,
    user_wallet_address: effectiveUserWalletAddress,
    vault_address: vaultAddress,
    information_sharing_scope: informationSharingScope,
    kill_switch_enabled: killSwitchEnabled,
    kill_switch_behavior: killSwitchBehavior,
    enable_memory: enableMemory,
    gateway_auth_key: gatewayAuthKey,
    eigencloud_auth_key: eigencloudAuthKey,
    verification_backend: verificationBackend,
    verification_eigencloud_endpoint: verificationEigencloudEndpoint,
    verification_eigencloud_auth_scheme: verificationEigencloudAuthScheme,
    verification_eigencloud_timeout_ms: verificationEigencloudTimeoutMs,
    verification_fallback_enabled: verificationFallbackEnabled,
    verification_fallback_signing_key_id: verificationFallbackSigningKeyId,
    verification_fallback_chain_path: verificationFallbackChainPath,
    verification_fallback_require_signed_receipts:
      verificationFallbackRequireSignedReceipts,
    accept_terms: acceptTerms,
  };
}

function applySuggestedConfig(config) {
  setInputValue("profile-name", config.profile_name);
  setInputValue("hyperliquid-network", config.hyperliquid_network);
  setInputValue("paper-live-policy", config.paper_live_policy);
  setInputValue("hyperliquid-api-base-url", config.hyperliquid_api_base_url);
  setInputValue("hyperliquid-ws-url", config.hyperliquid_ws_url);
  setInputValue("request-timeout-ms", config.request_timeout_ms);
  setInputValue("max-retries", config.max_retries);
  setInputValue("retry-backoff-ms", config.retry_backoff_ms);
  setInputValue("max-position-usd", config.max_position_size_usd);
  setInputValue("leverage-cap", config.leverage_cap);
  setInputValue("max-allocation-usd", config.max_allocation_usd);
  setInputValue("per-trade-cap-usd", config.per_trade_notional_cap_usd);
  setInputValue("max-leverage", config.max_leverage);
  setInputValue("max-slippage-bps", config.max_slippage_bps);
  if (Array.isArray(config.symbol_allowlist)) {
    setInputValue("symbol-allowlist", config.symbol_allowlist.join(","));
  }
  if (Array.isArray(config.symbol_denylist)) {
    setInputValue("symbol-denylist", config.symbol_denylist.join(","));
  }
  setInputValue("custody-mode", config.custody_mode);
  setInputValue("operator-wallet-address", config.operator_wallet_address);
  setInputValue("user-wallet-address", config.user_wallet_address);
  setInputValue("vault-address", config.vault_address);
  setInputValue("information-sharing-scope", config.information_sharing_scope);
  setInputValue("kill-switch-behavior", config.kill_switch_behavior);
  setInputValue("gateway-auth-key", config.gateway_auth_key);
  setInputValue("eigencloud-auth-key", config.eigencloud_auth_key);
  setInputValue("verification-backend", config.verification_backend);
  setInputValue(
    "verification-eigencloud-endpoint",
    config.verification_eigencloud_endpoint
  );
  setInputValue(
    "verification-eigencloud-auth-scheme",
    config.verification_eigencloud_auth_scheme
  );
  setInputValue(
    "verification-eigencloud-timeout-ms",
    config.verification_eigencloud_timeout_ms
  );
  setInputValue(
    "verification-fallback-signing-key-id",
    config.verification_fallback_signing_key_id
  );
  setInputValue(
    "verification-fallback-chain-path",
    config.verification_fallback_chain_path
  );

  setCheckboxValue("kill-switch-enabled", config.kill_switch_enabled);
  setCheckboxValue("enable-memory", config.enable_memory);
  setCheckboxValue(
    "verification-fallback-enabled",
    config.verification_fallback_enabled
  );
  setCheckboxValue(
    "verification-fallback-require-signed",
    config.verification_fallback_require_signed_receipts
  );

  syncWalletLinkedInputs(true);
  syncVerificationControls();
}

function setInputValue(id, value) {
  if (value === undefined || value === null) return;
  const node = document.getElementById(id);
  if (!node) return;
  node.value = String(value);
}

function setCheckboxValue(id, value) {
  if (typeof value !== "boolean") return;
  const node = document.getElementById(id);
  if (!node) return;
  node.checked = value;
}

function getValue(id) {
  const value = String(document.getElementById(id).value || "").trim();
  if (!value) {
    throw new Error(id + " is required.");
  }
  return value;
}

function optionalValue(id) {
  const value = String(document.getElementById(id).value || "").trim();
  return value || null;
}

function readInteger(id, min, max) {
  const raw = Number(document.getElementById(id).value);
  if (!Number.isFinite(raw)) {
    throw new Error(id + " must be a valid number.");
  }
  const value = Math.floor(raw);
  if (value < min || value > max) {
    throw new Error(id + " must be between " + String(min) + " and " + String(max) + ".");
  }
  return value;
}

function parseSymbols(value) {
  return String(value || "")
    .split(",")
    .map((v) => v.trim().toUpperCase())
    .filter((v) => v.length > 0);
}

function normalizeOptionalWallet(value) {
  const trimmed = String(value || "").trim();
  if (!trimmed) {
    return null;
  }
  const lower = trimmed.toLowerCase();
  if (!/^0x[a-f0-9]{40}$/.test(lower)) {
    throw new Error("Wallet addresses must be 0x-prefixed 40-hex values.");
  }
  return lower;
}

function syncWalletLinkedInputs(forceUpdate) {
  const custodyMode = String(document.getElementById("custody-mode").value || "").trim();
  const userWalletInput = document.getElementById("user-wallet-address");
  const connectedWallet = normalizeOptionalWallet(state.walletAddress);
  const requiresUserWallet = custodyMode === "user_wallet" || custodyMode === "dual_mode";

  if (requiresUserWallet && connectedWallet) {
    const current = normalizeOptionalWallet(userWalletInput.value);
    if (forceUpdate || !current) {
      userWalletInput.value = connectedWallet;
    }
  }

  if (requiresUserWallet) {
    userWalletInput.setAttribute("readonly", "readonly");
  } else {
    userWalletInput.removeAttribute("readonly");
  }
}

function syncVerificationControls() {
  const fallbackEnabled = document.getElementById("verification-fallback-enabled").checked;
  const fallbackKey = document.getElementById("verification-fallback-signing-key-id");
  const fallbackPath = document.getElementById("verification-fallback-chain-path");
  const fallbackSigned = document.getElementById("verification-fallback-require-signed");

  fallbackKey.disabled = !fallbackEnabled;
  fallbackPath.disabled = !fallbackEnabled;
  fallbackSigned.disabled = !fallbackEnabled;
  if (!fallbackEnabled) {
    fallbackSigned.checked = false;
  }
}

function showLoadingPanel() {
  state.progress = 8;
  el.loaderProgressFill.style.width = "8%";
  el.loadingPanel.classList.remove("hidden");
  el.readyActions.classList.add("hidden");
  el.openInstanceLink.href = "#";
  el.openVerifyLink.href = "#";
  el.openVerifyLink.classList.add("hidden");
}

function startPolling() {
  stopPolling();
  const intervalMs = Number(state.bootstrap && state.bootstrap.poll_interval_ms) || 1500;
  const poll = async () => {
    try {
      const session = await fetchJson(
        "/api/frontdoor/session/" + encodeURIComponent(state.sessionId)
      );
      handleSessionStatus(session);
      if (session.status === "ready" || session.status === "failed" || session.status === "expired") {
        await refreshSessionMonitor();
        stopPolling();
        return;
      }
    } catch (err) {
      el.loadingError.textContent = "Status poll failed: " + String(err.message || err);
    }
    state.pollTimer = setTimeout(poll, intervalMs);
  };
  poll();
}

function stopPolling() {
  if (state.pollTimer) {
    clearTimeout(state.pollTimer);
    state.pollTimer = null;
  }
}

function handleSessionStatus(session) {
  renderSessionKv({
    wallet: session.wallet_address,
    session: session.session_id,
    version: session.version,
    status: session.status,
    profile: session.profile_name,
    provisioningSource: session.provisioning_source,
    dedicatedInstance: session.dedicated_instance,
    launchedOnEigencloud: session.launched_on_eigencloud,
    verificationBackend: session.verification_backend,
    verificationLevel: session.verification_level,
    fallbackSigned: session.verification_fallback_require_signed_receipts,
    appId: session.eigen_app_id,
    verifyUrl: session.verify_url,
  });

  if (session.status === "provisioning") {
    advanceLoading(session.detail || "Provisioning in progress...", Math.min(state.progress + 12, 86));
    return;
  }

  if (session.status === "ready" && (session.instance_url || session.verify_url)) {
    const destination = sanitizeRedirectUrl(session.instance_url || session.verify_url);
    if (!destination) {
      el.loadingTitle.textContent = "Provisioning returned an invalid URL";
      el.loadingCopy.textContent = "Refusing redirect because destination is not http/https.";
      el.loadingError.textContent = "Invalid destination URL from provisioning backend.";
      return;
    }
    advanceLoading("Enclave ready. Redirecting...", 100);
    el.loadingTitle.textContent = "Your enclaved interface is live";
    if (session.dedicated_instance) {
      el.loadingCopy.textContent = "Launching your dedicated Enclagent instance now.";
    } else {
      el.loadingCopy.textContent = "Opening configured instance URL for this gateway.";
    }
    el.openInstanceLink.href = destination;
    const safeVerifyUrl = sanitizeRedirectUrl(session.verify_url);
    if (safeVerifyUrl) {
      el.openVerifyLink.href = safeVerifyUrl;
      el.openVerifyLink.classList.remove("hidden");
    }
    el.readyActions.classList.remove("hidden");
    setTimeout(() => {
      window.location.assign(destination);
    }, 1400);
    return;
  }

  const err = session.error || session.detail || "Provisioning failed.";
  el.loadingTitle.textContent = "Provisioning failed";
  el.loadingCopy.textContent = "No enclave was launched for this session.";
  el.loadingError.textContent = err;
}

function advanceLoading(message, progress) {
  state.progress = progress;
  el.loaderProgressFill.style.width = String(progress) + "%";
  el.loadingTitle.textContent = message;
}

function renderSessionKv(model) {
  const rows = [];
  if (model.wallet) rows.push("<p><strong>Wallet:</strong> " + escapeHtml(model.wallet) + "</p>");
  if (model.session) rows.push("<p><strong>Session:</strong> " + escapeHtml(model.session) + "</p>");
  if (model.version) rows.push("<p><strong>Version:</strong> v" + escapeHtml(String(model.version)) + "</p>");
  if (model.profile) rows.push("<p><strong>Profile:</strong> " + escapeHtml(String(model.profile)) + "</p>");
  if (model.status) rows.push("<p><strong>Status:</strong> " + escapeHtml(String(model.status)) + "</p>");
  if (model.provisioningSource) {
    rows.push(
      "<p><strong>Provisioning Source:</strong> " +
        escapeHtml(String(model.provisioningSource)) +
        "</p>"
    );
  }
  if (typeof model.dedicatedInstance === "boolean") {
    rows.push(
      "<p><strong>Dedicated Instance:</strong> " +
        escapeHtml(model.dedicatedInstance ? "yes" : "no") +
        "</p>"
    );
  }
  if (typeof model.launchedOnEigencloud === "boolean") {
    rows.push(
      "<p><strong>EigenCloud Launch:</strong> " +
        escapeHtml(model.launchedOnEigencloud ? "detected" : "not detected") +
        "</p>"
    );
  }
  if (model.verificationBackend) {
    rows.push(
      "<p><strong>Verification Backend:</strong> " +
        escapeHtml(String(model.verificationBackend)) +
        "</p>"
    );
  }
  if (model.verificationLevel) {
    rows.push(
      "<p><strong>Verification Level:</strong> " +
        escapeHtml(String(model.verificationLevel)) +
        "</p>"
    );
  }
  if (typeof model.fallbackSigned === "boolean") {
    rows.push(
      "<p><strong>Fallback Signed Receipts:</strong> " +
        escapeHtml(model.fallbackSigned ? "required" : "optional") +
        "</p>"
    );
  }
  if (model.appId) rows.push("<p><strong>Eigen App:</strong> " + escapeHtml(String(model.appId)) + "</p>");
  if (model.verifyUrl) rows.push("<p><strong>Verify:</strong> " + escapeHtml(String(model.verifyUrl)) + "</p>");
  el.sessionKv.innerHTML = rows.join("");
}

function setBootstrapStatus(message, tone) {
  el.bootstrapStatus.textContent = message;
  el.bootstrapStatus.classList.remove("ok", "warn");
  if (tone) el.bootstrapStatus.classList.add(tone);
}

function setPrivyStatus(text) {
  el.privyAuthStatus.value = text;
}

function disableLaunch(reason) {
  el.connectWalletBtn.disabled = true;
  el.launchSessionBtn.disabled = true;
  el.configError.textContent = reason;
}

function disableProvisioning(reason) {
  el.launchSessionBtn.disabled = true;
  el.configError.textContent = reason;
}

function renderMonitorHintOnly() {
  if (!el.monitorSummary) return;
  const backend = describeProvisioningBackend(
    state.bootstrap && state.bootstrap.provisioning_backend
  );
  el.monitorSummary.textContent = "Provisioning backend: " + backend + ".";
  if (el.monitorTableWrap) el.monitorTableWrap.classList.add("hidden");
  if (el.monitorEmpty) el.monitorEmpty.classList.remove("hidden");
  if (el.monitorTableBody) el.monitorTableBody.innerHTML = "";
}

async function refreshSessionMonitor() {
  if (!el.monitorSummary) return;
  if (el.monitorError) el.monitorError.textContent = "";

  if (!state.walletAddress) {
    renderMonitorHintOnly();
    return;
  }

  const wallet = normalizeOptionalWallet(state.walletAddress);
  if (!wallet) {
    renderMonitorHintOnly();
    return;
  }

  try {
    const payload = await fetchJson(
      "/api/frontdoor/sessions?wallet_address=" +
        encodeURIComponent(wallet) +
        "&limit=12"
    );
    state.monitorSnapshot = payload;
    renderMonitorTable(payload);
  } catch (err) {
    if (el.monitorError) {
      el.monitorError.textContent =
        "Session monitor load failed: " + String(err.message || err);
    }
  }
}

function renderMonitorTable(payload) {
  if (!el.monitorSummary || !el.monitorTableBody || !el.monitorTableWrap || !el.monitorEmpty) {
    return;
  }

  const sessions = Array.isArray(payload && payload.sessions) ? payload.sessions : [];
  const backend = describeProvisioningBackend(
    state.bootstrap && state.bootstrap.provisioning_backend
  );
  el.monitorSummary.textContent =
    "Provisioning backend: " +
    backend +
    ". Sessions loaded: " +
    String(sessions.length) +
    " of " +
    String(Number(payload && payload.total) || 0) +
    ".";

  if (!sessions.length) {
    el.monitorTableBody.innerHTML = "";
    el.monitorTableWrap.classList.add("hidden");
    el.monitorEmpty.classList.remove("hidden");
    return;
  }

  const rows = sessions.map((session) => {
    const status = String(session.status || "-");
    const statusClass = "status-pill status-" + status.replace(/[^a-z0-9_-]/gi, "");
    const sessionId = String(session.session_id || "");
    const shortSession = sessionId ? sessionId.slice(0, 8) + "..." : "-";
    const provisioningSource = String(session.provisioning_source || "unknown");
    const dedicated = session.dedicated_instance ? "dedicated" : "shared";
    const eigencloud = typeof session.launched_on_eigencloud === "boolean"
      ? (session.launched_on_eigencloud ? "yes" : "no")
      : "pending";
    const verification = [
      session.verification_level || "-",
      session.verification_backend || "-",
    ].join(" / ");
    return (
      "<tr>" +
      "<td title=\"" +
      escapeHtml(sessionId) +
      "\">" +
      escapeHtml(shortSession) +
      "</td>" +
      "<td><span class=\"" +
      escapeHtml(statusClass) +
      "\">" +
      escapeHtml(status) +
      "</span></td>" +
      "<td>" +
      escapeHtml(provisioningSource) +
      " (" +
      escapeHtml(dedicated) +
      ")</td>" +
      "<td>" +
      escapeHtml(eigencloud) +
      "</td>" +
      "<td>" +
      escapeHtml(verification) +
      "</td>" +
      "<td>" +
      escapeHtml(formatTimestamp(session.updated_at)) +
      "</td>" +
      "</tr>"
    );
  });

  el.monitorTableBody.innerHTML = rows.join("");
  el.monitorEmpty.classList.add("hidden");
  el.monitorTableWrap.classList.remove("hidden");
}

function describeProvisioningBackend(value) {
  const backend = String(value || "").trim();
  if (backend === "command") return "command dynamic provisioning";
  if (backend === "default_instance_url") return "static fallback URL";
  if (backend === "unconfigured") return "unconfigured";
  return backend || "unknown";
}

function formatTimestamp(value) {
  if (!value) return "-";
  const d = new Date(String(value));
  if (Number.isNaN(d.getTime())) return String(value);
  return d.toLocaleString();
}

function normalizedPrivyId() {
  if (state.privyUserId) return state.privyUserId;
  const value = String(el.privyUserId.value || "").trim();
  return value || null;
}

function parseChainId(value) {
  const v = String(value || "").trim();
  if (!v) return null;
  if (v.startsWith("0x")) {
    const parsed = parseInt(v, 16);
    return Number.isFinite(parsed) ? parsed : null;
  }
  const parsed = parseInt(v, 10);
  return Number.isFinite(parsed) ? parsed : null;
}

async function ensureExpectedChain(provider, currentChainId) {
  const required = requiredChainIdForHost(window.location.hostname);
  if (!required) {
    return String(currentChainId || "");
  }
  const parsedCurrent = parseChainId(currentChainId);
  if (parsedCurrent === required) {
    return String(currentChainId || "");
  }
  await switchChain(provider, required);
  const switched = await provider.request({ method: "eth_chainId" });
  const parsedSwitched = parseChainId(switched);
  if (parsedSwitched !== required) {
    throw new Error(
      "Wallet must be connected to chain " +
        String(required) +
        " for this gateway."
    );
  }
  return String(switched || "");
}

function requiredChainIdForHost(hostname) {
  const host = String(hostname || "").toLowerCase();
  if (host.includes("verify-sepolia")) {
    return 11155111;
  }
  return null;
}

function renderEnvironmentBadge() {
  if (!el.environmentBadge) return;
  const env = detectDeployEnvironment(window.location.hostname);
  el.environmentBadge.textContent = env.label;
  el.environmentBadge.classList.remove("env-prod", "env-staging", "env-dev");
  el.environmentBadge.classList.add(env.tone);
}

function detectDeployEnvironment(hostname) {
  const host = String(hostname || "").toLowerCase();
  if (!host || host === "localhost" || host === "127.0.0.1") {
    return { label: "Local", tone: "env-dev" };
  }
  if (
    host.includes("staging") ||
    host.includes("stage") ||
    host.includes("-stg") ||
    host.includes(".stg.")
  ) {
    return { label: "Staging", tone: "env-staging" };
  }
  return { label: "Production", tone: "env-prod" };
}

async function switchChain(provider, chainId) {
  const hexChainId = "0x" + Number(chainId).toString(16);
  try {
    await provider.request({
      method: "wallet_switchEthereumChain",
      params: [{ chainId: hexChainId }],
    });
    return;
  } catch (err) {
    const code = Number(err && err.code);
    if (code !== 4902 || chainId !== 11155111) {
      throw err;
    }
  }

  await provider.request({
    method: "wallet_addEthereumChain",
    params: [
      {
        chainId: "0xaa36a7",
        chainName: "Sepolia",
        nativeCurrency: { name: "Sepolia Ether", symbol: "ETH", decimals: 18 },
        rpcUrls: ["https://rpc.sepolia.org"],
        blockExplorerUrls: ["https://sepolia.etherscan.io"],
      },
    ],
  });
}

async function fetchJson(url, options) {
  const opts = Object.assign({ method: "GET" }, options || {});
  opts.headers = Object.assign({}, opts.headers || {});
  if (opts.body && typeof opts.body === "object") {
    opts.headers["Content-Type"] = "application/json";
    opts.body = JSON.stringify(opts.body);
  }
  const res = await fetch(url, opts);
  const text = await res.text();
  let payload = null;
  if (text) {
    try {
      payload = JSON.parse(text);
    } catch (_) {
      payload = null;
    }
  }
  if (!res.ok) {
    const detail =
      (payload && (payload.error || payload.message || payload.detail)) ||
      text ||
      (res.status + " " + res.statusText);
    throw new Error(detail);
  }
  return payload;
}

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function sanitizeRedirectUrl(value) {
  if (!value) return null;
  try {
    const url = new URL(String(value), window.location.origin);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return null;
    }
    return url.toString();
  } catch (_) {
    return null;
  }
}

main();
