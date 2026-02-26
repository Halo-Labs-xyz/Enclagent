const MODULE_IDS = [
  "identity",
  "policy",
  "verification",
  "provisioning",
  "runtimecontrols",
  "evidence",
];
const PRIVY_SDK_IMPORT_URL = "https://esm.sh/@privy-io/js-sdk-core@0.58.7";

const state = {
  bootstrap: null,
  walletAddress: "",
  chainId: "",
  sessionId: "",
  typedSessionId: "",
  typedLastStatus: "",
  challengeMessage: "",
  pollTimer: null,
  progress: 6,
  privyUserId: "",
  privyIdentityToken: "",
  privyAccessToken: "",
  privyClient: null,
  ethereumProvider: null,
  monitorSnapshot: null,
  sessionSnapshot: null,
  experienceManifest: null,
  configContract: null,
  policyTemplatesEnabled: false,
  policyTemplates: [],
  selectedPolicyTemplate: "",
  selectedPolicyDomain: "general",
  identityEntryPath: "wallet",
  identityProvider: "wallet",
  walletBindingVerified: false,
  lastConfigDraft: null,
  runtimeActionPending: false,
  moduleContracts: {},
  moduleExpanded: {},
  modulePopoverId: "",
  typed: {
    onboarding: null,
    timeline: null,
    verification: null,
    todos: null,
    funding: null,
    runtimeControl: null,
    errors: {
      onboarding: "",
      timeline: "",
      verification: "",
      todos: "",
      funding: "",
      runtimeControl: "",
    },
  },
};

const el = {
  bootstrapStatus: document.getElementById("bootstrap-status"),
  environmentBadge: document.getElementById("environment-badge"),
  moduleStateNote: document.getElementById("module-state-note"),
  moduleNextActionNote: document.getElementById("module-next-action-note"),
  walletAddress: document.getElementById("wallet-address"),
  walletChainId: document.getElementById("wallet-chain-id"),
  privyUserId: document.getElementById("privy-user-id"),
  privyAuthStatus: document.getElementById("privy-auth-status"),
  connectWalletBtn: document.getElementById("connect-wallet-btn"),
  walletError: document.getElementById("wallet-error"),
  identityProvider: document.getElementById("identity-provider"),
  identityProviderNote: document.getElementById("identity-provider-note"),
  identityEntryNote: document.getElementById("identity-entry-note"),
  identityBindingNote: document.getElementById("identity-binding-note"),
  policyTemplateSelect: document.getElementById("policy-template-select"),
  policyTemplateDomain: document.getElementById("policy-template-domain"),
  policyTemplateDetails: document.getElementById("policy-template-details"),
  applyPolicyTemplateBtn: document.getElementById("apply-policy-template-btn"),
  policyTemplateMessage: document.getElementById("policy-template-message"),
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
  typedSessionId: document.getElementById("typed-session-id"),
  loadTypedSessionBtn: document.getElementById("load-typed-session-btn"),
  typedSessionError: document.getElementById("typed-session-error"),
  proofVerification: document.getElementById("proof-verification"),
  proofTimeline: document.getElementById("proof-timeline"),
  proofProvisioning: document.getElementById("proof-provisioning"),
  proofFallback: document.getElementById("proof-fallback"),
  proofRuntime: document.getElementById("proof-runtime"),
  proofTodo: document.getElementById("proof-todo"),
  proofError: document.getElementById("proof-error"),
  onboardingSummary: document.getElementById("onboarding-summary"),
  onboardingTranscript: document.getElementById("onboarding-transcript"),
  onboardingChatInput: document.getElementById("onboarding-chat-input"),
  sendOnboardingChatBtn: document.getElementById("send-onboarding-chat-btn"),
  onboardingConfirmPlanBtn: document.getElementById("onboarding-confirm-plan-btn"),
  onboardingConfirmSignBtn: document.getElementById("onboarding-confirm-sign-btn"),
  onboardingError: document.getElementById("onboarding-error"),
  timelineSummary: document.getElementById("timeline-summary"),
  timelineEvents: document.getElementById("timeline-events"),
  timelineError: document.getElementById("timeline-error"),
  verificationExplanationKv: document.getElementById("verification-explanation-kv"),
  verificationError: document.getElementById("verification-error"),
  gatewayTodoSummary: document.getElementById("gateway-todo-summary"),
  gatewayTodoList: document.getElementById("gateway-todo-list"),
  gatewayTodoError: document.getElementById("gateway-todo-error"),
  runtimeControlSummary: document.getElementById("runtime-control-summary"),
  runtimeControlError: document.getElementById("runtime-control-error"),
  runtimePauseBtn: document.getElementById("runtime-pause-btn"),
  runtimeResumeBtn: document.getElementById("runtime-resume-btn"),
  runtimeTerminateBtn: document.getElementById("runtime-terminate-btn"),
  runtimeRotateKeyBtn: document.getElementById("runtime-rotate-key-btn"),
  fundingPreflightSummary: document.getElementById("funding-preflight-summary"),
  fundingPreflightChecks: document.getElementById("funding-preflight-checks"),
  fundingPreflightError: document.getElementById("funding-preflight-error"),
  modulePopoverBackdrop: document.getElementById("module-popover-backdrop"),
  modulePopoverTitle: document.getElementById("module-popover-title"),
  modulePopoverBody: document.getElementById("module-popover-body"),
  modulePopoverClose: document.getElementById("module-popover-close"),
};

state.moduleContracts = buildDefaultModuleContracts();
const moduleDom = buildModuleDomRefs();
initializeModuleInteractivity();

function buildDefaultModuleContracts() {
  return {
    identity: {
      title: "Identity",
      purpose_id: "frontdoor.identity",
      backend_contract: "POST /api/frontdoor/challenge",
      artifact_binding: "challenge_message",
      success_state: "awaiting_signature",
      failure_state: "challenge_rejected",
    },
    policy: {
      title: "Policy",
      purpose_id: "frontdoor.policy",
      backend_contract: "POST /api/frontdoor/suggest-config",
      artifact_binding: "frontdoor_user_config",
      success_state: "config_validated",
      failure_state: "config_invalid",
    },
    verification: {
      title: "Verification",
      purpose_id: "frontdoor.verification",
      backend_contract: "POST /api/frontdoor/verify",
      artifact_binding: "signature_receipt",
      success_state: "provisioning",
      failure_state: "verification_failed",
    },
    provisioning: {
      title: "Provisioning",
      purpose_id: "frontdoor.provisioning",
      backend_contract: "GET /api/frontdoor/session/{session_id}",
      artifact_binding: "provisioning_receipt",
      success_state: "ready",
      failure_state: "failed",
    },
    runtimecontrols: {
      title: "RuntimeControls",
      purpose_id: "frontdoor.runtime_controls",
      backend_contract: "POST /api/frontdoor/session/{session_id}/runtime-control",
      artifact_binding: "runtime_state_transition",
      success_state: "runtime_control_applied",
      failure_state: "runtime_control_blocked",
    },
    evidence: {
      title: "Evidence",
      purpose_id: "frontdoor.evidence",
      backend_contract:
        "GET /api/frontdoor/session/{session_id}/timeline + verification-explanation + gateway-todos + funding-preflight",
      artifact_binding: "timeline_event + verification_explanation + gateway_todo_feed",
      success_state: "evidence_loaded",
      failure_state: "evidence_unavailable",
    },
  };
}

function buildModuleDomRefs() {
  const refs = {};
  for (let i = 0; i < MODULE_IDS.length; i += 1) {
    const id = MODULE_IDS[i];
    refs[id] = {
      card: document.getElementById("module-card-" + id),
      state: document.getElementById("module-state-" + id),
      summary: document.getElementById("module-summary-" + id),
      artifactValue: document.getElementById("module-artifact-value-" + id),
      contractRoot: document.querySelector(
        "#module-card-" + id + " .module-contract"
      ),
      purpose: document.getElementById("module-purpose-" + id),
      contract: document.getElementById("module-contract-" + id),
      artifact: document.getElementById("module-artifact-" + id),
      success: document.getElementById("module-success-" + id),
      failure: document.getElementById("module-failure-" + id),
      toggleBtn: null,
      popoutBtn: null,
      dropdown: null,
    };
  }
  return refs;
}

function initializeModuleInteractivity() {
  for (let i = 0; i < MODULE_IDS.length; i += 1) {
    const moduleId = MODULE_IDS[i];
    const refs = moduleDom[moduleId];
    if (!refs || !refs.card || !refs.contractRoot) {
      continue;
    }
    if (refs.card.dataset.moduleUiReady === "1") {
      continue;
    }

    const controls = document.createElement("div");
    controls.className = "module-controls";

    const toggleBtn = document.createElement("button");
    toggleBtn.type = "button";
    toggleBtn.className = "module-chip-btn";
    toggleBtn.textContent = "Show details";
    toggleBtn.setAttribute("aria-expanded", "false");
    toggleBtn.addEventListener("click", () => {
      const isExpanded = !!state.moduleExpanded[moduleId];
      setModuleExpanded(moduleId, !isExpanded);
    });

    const popoutBtn = document.createElement("button");
    popoutBtn.type = "button";
    popoutBtn.className = "module-chip-btn";
    popoutBtn.textContent = "Pop out";
    popoutBtn.addEventListener("click", () => {
      openModulePopover(moduleId);
    });

    controls.appendChild(toggleBtn);
    controls.appendChild(popoutBtn);

    const dropdown = document.createElement("div");
    dropdown.className = "module-dropdown";
    dropdown.hidden = true;
    refs.contractRoot.parentNode.insertBefore(dropdown, refs.contractRoot);
    dropdown.appendChild(refs.contractRoot);
    refs.card.insertBefore(controls, dropdown);

    refs.toggleBtn = toggleBtn;
    refs.popoutBtn = popoutBtn;
    refs.dropdown = dropdown;
    refs.card.dataset.moduleUiReady = "1";
    state.moduleExpanded[moduleId] = false;
  }
}

function setModuleExpanded(moduleId, expanded) {
  const refs = moduleDom[moduleId];
  if (!refs || !refs.card || !refs.dropdown || !refs.toggleBtn) {
    return;
  }
  const isExpanded = !!expanded;
  state.moduleExpanded[moduleId] = isExpanded;
  refs.card.classList.toggle("module-open", isExpanded);
  refs.dropdown.hidden = !isExpanded;
  refs.toggleBtn.setAttribute("aria-expanded", isExpanded ? "true" : "false");
  refs.toggleBtn.textContent = isExpanded ? "Hide details" : "Show details";
}

function openModulePopover(moduleId) {
  const refs = moduleDom[moduleId];
  if (!refs || !refs.card || !el.modulePopoverBackdrop) {
    return;
  }
  state.modulePopoverId = moduleId;
  renderModulePopover();
  el.modulePopoverBackdrop.classList.remove("hidden");
}

function closeModulePopover() {
  state.modulePopoverId = "";
  if (el.modulePopoverBackdrop) {
    el.modulePopoverBackdrop.classList.add("hidden");
  }
}

function renderModulePopover() {
  if (!el.modulePopoverTitle || !el.modulePopoverBody || !state.modulePopoverId) {
    return;
  }
  const moduleId = state.modulePopoverId;
  const refs = moduleDom[moduleId];
  const contract = state.moduleContracts[moduleId] || null;
  const title = contract && contract.title ? contract.title : moduleId;
  const status = refs && refs.state ? refs.state.textContent : "unknown";
  const summary = refs && refs.summary ? refs.summary.textContent : "No summary available.";
  const artifactValue =
    refs && refs.artifactValue ? refs.artifactValue.textContent : "Artifact pending.";

  el.modulePopoverTitle.textContent = title + " module state";
  el.modulePopoverBody.innerHTML =
    '<div class="module-popover-grid">' +
    '<p><strong>Status:</strong> ' +
    escapeHtml(status || "unknown") +
    "</p>" +
    '<p><strong>Summary:</strong> ' +
    escapeHtml(summary || "") +
    "</p>" +
    '<p><strong>Artifact Value:</strong> ' +
    escapeHtml(artifactValue || "") +
    "</p>" +
    '<p><strong>Purpose ID:</strong> ' +
    escapeHtml((contract && contract.purpose_id) || "-") +
    "</p>" +
    '<p><strong>Backend Contract:</strong> ' +
    escapeHtml((contract && contract.backend_contract) || "-") +
    "</p>" +
    '<p><strong>Artifact Binding:</strong> ' +
    escapeHtml((contract && contract.artifact_binding) || "-") +
    "</p>" +
    '<p><strong>Success State:</strong> ' +
    escapeHtml((contract && contract.success_state) || "-") +
    "</p>" +
    '<p><strong>Failure State:</strong> ' +
    escapeHtml((contract && contract.failure_state) || "-") +
    "</p>" +
    "</div>";
}

function createFrontdoorError(message, code, operatorHint) {
  const err = new Error(String(message || "Frontdoor request failed."));
  err.code = String(code || "FRONTDOOR_REQUEST_FAILED");
  err.operatorHint = String(
    operatorHint || "Inspect gateway configuration and session state, then retry."
  );
  return err;
}

function errorMessage(err) {
  if (!err) return "Unknown error.";
  if (typeof err === "string") return err;
  if (typeof err.message === "string" && err.message.trim()) {
    return err.message.trim();
  }
  return String(err);
}

function formatTypedError(prefix, err) {
  const code =
    err && typeof err.code === "string" && err.code.trim()
      ? err.code.trim()
      : "FRONTDOOR_REQUEST_FAILED";
  const operatorHint =
    err && typeof err.operatorHint === "string" && err.operatorHint.trim()
      ? err.operatorHint.trim()
      : "Inspect gateway configuration and session state, then retry.";
  const base = errorMessage(err);
  if (prefix) {
    return (
      prefix +
      ": [" +
      code +
      "] " +
      base +
      " Operator hint: " +
      operatorHint
    );
  }
  return "[" + code + "] " + base + " Operator hint: " + operatorHint;
}

function setTypedFailureText(target, prefix, err) {
  if (!target) return;
  target.textContent = formatTypedError(prefix, err);
}

async function main() {
  bindEvents();
  renderEnvironmentBadge();
  renderTypedPlaceholders();
  renderIdentityEntryPathState();
  renderModuleStateMachine();
  renderProofSpotlight();
  try {
    const bootstrap = await fetchJson("/api/frontdoor/bootstrap");
    state.bootstrap = bootstrap;
    await Promise.allSettled([
      loadExperienceManifest(),
      loadTypedPolicyTemplates(),
    ]);

    if (!bootstrap.enabled) {
      setBootstrapStatus("Frontdoor mode is disabled for this gateway deployment.", "warn");
      disableLaunch("Frontdoor disabled");
      renderModuleStateMachine();
      return;
    }
    if (bootstrap.require_privy && !bootstrap.privy_app_id) {
      setBootstrapStatus("Privy is required but no Privy App ID was resolved from gateway environment.", "warn");
      disableLaunch(
        "Missing Privy App ID. Set GATEWAY_FRONTDOOR_PRIVY_APP_ID (or PRIVY_APP_ID / NEXT_PUBLIC_PRIVY_APP_ID)."
      );
      renderModuleStateMachine();
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
      renderModuleStateMachine();
      return;
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
    renderIdentityEntryPathState();
    renderMonitorHintOnly();
    renderModuleStateMachine();
    if (bootstrap.privy_app_id) {
      mountPrivyNativeLoginButton(bootstrap);
    } else {
      showFallbackConnectButton();
    }
  } catch (err) {
    setBootstrapStatus("Failed to load gateway bootstrap.", "warn");
    disableLaunch("Bootstrap unavailable");
    setTypedFailureText(el.walletError, "Bootstrap failed", err);
    renderModuleStateMachine();
    showFallbackConnectButton();
  }
}

function showFallbackConnectButton() {
  const root = document.getElementById("privy-connect-root");
  const btn = document.getElementById("connect-wallet-btn");
  if (root) root.innerHTML = "";
  if (btn) {
    btn.classList.remove("hidden");
    btn.textContent = "Connect wallet";
  }
}

function enclagentPrivyLoginComplete(payload) {
  const walletAddress = (payload && payload.walletAddress) ? String(payload.walletAddress).trim() : "";
  const privyUserId = (payload && payload.privyUserId) ? String(payload.privyUserId).trim() : "";
  const identityToken = (payload && payload.identityToken) ? String(payload.identityToken).trim() : "";
  const accessToken = (payload && payload.accessToken) ? String(payload.accessToken).trim() : "";
  const chainId = (payload && payload.chainId) != null ? String(payload.chainId) : "";
  if (walletAddress && /^0x[a-fA-F0-9]{40}$/.test(walletAddress)) {
    state.walletAddress = walletAddress;
    state.chainId = chainId;
    state.privyUserId = privyUserId || ("wallet:" + walletAddress);
    state.privyIdentityToken = identityToken || null;
    state.privyAccessToken = accessToken || null;
    state.walletBindingVerified = true;
    if (el.walletAddress) el.walletAddress.value = state.walletAddress;
    if (el.walletChainId) el.walletChainId.value = state.chainId;
    if (el.privyUserId) el.privyUserId.value = state.privyUserId;
    setPrivyStatus("Wallet connected (Privy)");
    syncWalletLinkedInputs(true);
    refreshSessionMonitor().catch(() => {});
    renderIdentityEntryPathState();
    renderModuleStateMachine();
  }
}

async function mountPrivyNativeLoginButton(bootstrap) {
  const appId = (bootstrap && bootstrap.privy_app_id) ? String(bootstrap.privy_app_id).trim() : "";
  if (!appId) {
    showFallbackConnectButton();
    return;
  }
  const rootEl = document.getElementById("privy-connect-root");
  const fallbackBtn = document.getElementById("connect-wallet-btn");
  if (!rootEl) {
    if (fallbackBtn) fallbackBtn.classList.remove("hidden");
    return;
  }
  if (fallbackBtn) fallbackBtn.classList.add("hidden");
  window.__enclagentPrivyLoginComplete = enclagentPrivyLoginComplete;
  try {
    const [ReactMod, ReactDOMClientMod, PrivyAuth] = await Promise.all([
      import("https://esm.sh/react@18?bundle"),
      import("https://esm.sh/react-dom@18/client?bundle"),
      import("https://esm.sh/@privy-io/react-auth@0.58.7?bundle"),
    ]);
    const React = ReactMod.default || ReactMod;
    const ReactDOMClient = ReactDOMClientMod.default || ReactDOMClientMod;
    const createRoot = ReactDOMClient.createRoot;
    if (typeof createRoot !== "function") {
      showFallbackConnectButton();
      return;
    }
    const createElement = React.createElement;
    const PrivyProvider = PrivyAuth.PrivyProvider || (PrivyAuth.default && PrivyAuth.default.PrivyProvider);
    const useLogin = PrivyAuth.useLogin || (PrivyAuth.default && PrivyAuth.default.useLogin);
    const usePrivy = PrivyAuth.usePrivy || (PrivyAuth.default && PrivyAuth.default.usePrivy);
    if (!PrivyProvider || !useLogin || !usePrivy) {
      showFallbackConnectButton();
      return;
    }
    const clientId = (bootstrap.privy_client_id && String(bootstrap.privy_client_id).trim()) || undefined;
    const LoginButtonWrapper = function () {
      const privy = usePrivy();
      const getIdentityToken = privy && typeof privy.getIdentityToken === "function" ? privy.getIdentityToken.bind(privy) : function () { return Promise.resolve(""); };
      const getAccessToken = privy && typeof privy.getAccessToken === "function" ? privy.getAccessToken.bind(privy) : function () { return Promise.resolve(""); };
      const { login } = useLogin({
        onComplete: async function (payload) {
          const user = payload && payload.user;
          const walletAccount = user && user.accounts && user.accounts.find(function (a) { return a.type === "wallet"; });
          const walletAddress = (walletAccount && walletAccount.address) ? String(walletAccount.address).trim() : "";
          const privyUserId = (user && (user.id || user.user_id)) ? String(user.id || user.user_id).trim() : (walletAddress ? "wallet:" + walletAddress : "");
          let identityToken = "";
          let accessToken = "";
          let chainId = "";
          try {
            identityToken = (await getIdentityToken()) || "";
            accessToken = (await getAccessToken()) || "";
          } catch (_) {}
          if (typeof window.__enclagentPrivyLoginComplete === "function") {
            window.__enclagentPrivyLoginComplete({ walletAddress, privyUserId, identityToken, accessToken, chainId });
          }
        },
        onError: function (err) {
          if (el.walletError) el.walletError.textContent = err && err.message ? err.message : "Privy login failed.";
          setPrivyStatus("Login failed");
        },
      });
      const ready = privy && privy.ready;
      const authenticated = privy && privy.authenticated;
      const disabled = !ready || !!authenticated;
      return createElement("button", {
        type: "button",
        className: "btn-primary",
        onClick: function () { login(); },
        disabled: disabled,
      }, authenticated ? "Connected" : "Log in with Privy");
    };
    const providerConfig = {
      appId,
      config: { loginMethods: ["wallet", "email", "google", "apple", "github", "discord", "twitter"] },
    };
    if (clientId) providerConfig.clientId = clientId;
    const app = createElement(PrivyProvider, providerConfig, createElement(LoginButtonWrapper));
    const root = createRoot(rootEl);
    root.render(app);
  } catch (err) {
    if (el.walletError) el.walletError.textContent = "Could not load Privy. Use the connect button below.";
    showFallbackConnectButton();
  }
}

function bindEvents() {
  document.getElementById("custody-mode").addEventListener("change", () => {
    syncWalletLinkedInputs(false);
  });
  document
    .getElementById("verification-fallback-enabled")
    .addEventListener("change", syncVerificationControls);

  if (el.identityProvider) {
    el.identityProvider.addEventListener("change", () => {
      const provider = normalizeIdentityProvider(el.identityProvider.value);
      state.identityProvider = provider;
      state.identityEntryPath = identityPathForProvider(provider);
      state.walletBindingVerified = !!normalizeOptionalWallet(state.walletAddress);
      renderIdentityEntryPathState();
      renderModuleStateMachine();
    });
  }

  if (el.policyTemplateSelect) {
    el.policyTemplateSelect.addEventListener("change", () => {
      state.selectedPolicyTemplate = String(el.policyTemplateSelect.value || "");
      syncSelectedPolicyTemplateMetadata();
      renderModuleStateMachine();
    });
  }
  if (el.applyPolicyTemplateBtn) {
    el.applyPolicyTemplateBtn.addEventListener("click", applySelectedPolicyTemplate);
  }

  if (el.refreshMonitorBtn) {
    el.refreshMonitorBtn.addEventListener("click", refreshSessionMonitor);
  }
  if (el.loadTypedSessionBtn) {
    el.loadTypedSessionBtn.addEventListener("click", async () => {
      clearTypedSessionError();
      try {
        const sessionId = readTypedSessionIdFromInput();
        setTypedSessionId(sessionId);
        await refreshTypedApiSurfaces(sessionId);
      } catch (err) {
        setTypedSessionError(formatTypedError("Typed session load failed", err));
      }
    });
  }
  if (el.sendOnboardingChatBtn) {
    el.sendOnboardingChatBtn.addEventListener("click", sendOnboardingChatMessage);
  }
  if (el.onboardingConfirmPlanBtn) {
    el.onboardingConfirmPlanBtn.addEventListener("click", () =>
      sendOnboardingPresetMessage("confirm plan")
    );
  }
  if (el.onboardingConfirmSignBtn) {
    el.onboardingConfirmSignBtn.addEventListener("click", () =>
      sendOnboardingPresetMessage("confirm sign")
    );
  }
  if (el.runtimePauseBtn) {
    el.runtimePauseBtn.addEventListener("click", () =>
      applyRuntimeControlAction("pause")
    );
  }
  if (el.runtimeResumeBtn) {
    el.runtimeResumeBtn.addEventListener("click", () =>
      applyRuntimeControlAction("resume")
    );
  }
  if (el.runtimeTerminateBtn) {
    el.runtimeTerminateBtn.addEventListener("click", () =>
      applyRuntimeControlAction("terminate")
    );
  }
  if (el.runtimeRotateKeyBtn) {
    el.runtimeRotateKeyBtn.addEventListener("click", () =>
      applyRuntimeControlAction("rotate_auth_key")
    );
  }
  if (el.modulePopoverClose) {
    el.modulePopoverClose.addEventListener("click", closeModulePopover);
  }
  if (el.modulePopoverBackdrop) {
    el.modulePopoverBackdrop.addEventListener("click", (event) => {
      if (event.target === el.modulePopoverBackdrop) {
        closeModulePopover();
      }
    });
  }
  document.addEventListener("keydown", (event) => {
    if (
      event.key === "Escape" &&
      el.modulePopoverBackdrop &&
      !el.modulePopoverBackdrop.classList.contains("hidden")
    ) {
      closeModulePopover();
    }
  });

  el.connectWalletBtn.addEventListener("click", async () => {
    el.walletError.textContent = "";
    try {
      await connectIdentityWithProvider();
      await refreshSessionMonitor();
      renderIdentityEntryPathState();
      renderModuleStateMachine();
    } catch (err) {
      setTypedFailureText(el.walletError, "Identity connect failed", err);
      setPrivyStatus("Wallet connect failed");
      renderModuleStateMachine();
    }
  });

  el.suggestConfigBtn.addEventListener("click", async () => {
    el.suggestionError.textContent = "";
    el.suggestionMessage.textContent = "";
    if (el.policyTemplateMessage) {
      el.policyTemplateMessage.textContent = "";
    }
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
          domain: state.selectedPolicyDomain || "general",
          gateway_auth_key: optionalValue("gateway-auth-key"),
        },
      });
      applySuggestedConfig(suggestion.config || {});
      state.lastConfigDraft = suggestion.config || null;
      renderModuleStateMachine();
      const assumptions = Array.isArray(suggestion.assumptions)
        ? suggestion.assumptions
        : [];
      const warnings = Array.isArray(suggestion.warnings) ? suggestion.warnings : [];
      const messages = assumptions.concat(warnings);
      el.suggestionMessage.textContent = messages.length
        ? messages.join(" ")
        : "Suggested config applied. Review fields before launch.";
    } catch (err) {
      setTypedFailureText(el.suggestionError, "Config suggestion failed", err);
      renderModuleStateMachine();
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
      renderModuleStateMachine();
      return;
    }
    if (!state.bootstrap || !state.bootstrap.enabled) {
      el.configError.textContent = "Frontdoor flow is not enabled.";
      renderModuleStateMachine();
      return;
    }
    if (
      (state.identityEntryPath === "email" || state.identityEntryPath === "social") &&
      !state.walletBindingVerified
    ) {
      el.configError.textContent =
        "Wallet binding is required before launch when initial entry path is email/social.";
      renderIdentityEntryPathState();
      renderModuleStateMachine();
      return;
    }

    try {
      el.launchSessionBtn.disabled = true;
      const cfg = readConfig();
      state.lastConfigDraft = cfg;
      renderModuleStateMachine();
      const challenge = await fetchJson("/api/frontdoor/challenge", {
        method: "POST",
        body: {
          wallet_address: state.walletAddress,
          privy_user_id: normalizedPrivyId(),
          chain_id: parseChainId(state.chainId),
        },
      });

      state.sessionId = challenge.session_id;
      state.typedLastStatus = "";
      state.challengeMessage = challenge.message;
      renderModuleStateMachine();
      if (looksLikeSessionId(challenge.session_id)) {
        setTypedSessionId(challenge.session_id);
      } else {
        setTypedSessionError(
          "Launch returned a non-UUID session identifier. Typed session modules are unavailable for this run."
        );
      }
      showLoadingPanel();
      advanceLoading("Challenge issued. Running onboarding confirmation...", 14);
      await ensureOnboardingReadyForLaunch(challenge.session_id, cfg);
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
      renderModuleStateMachine();
      try {
        await refreshTypedApiSurfaces(challenge.session_id);
      } catch (typedErr) {
        setTypedSessionError(
          "Typed surface bootstrap failed: " + String(typedErr.message || typedErr)
        );
      }
      startPolling();
    } catch (err) {
      el.launchSessionBtn.disabled = false;
      const message = formatTypedError("Launch failed", err);
      el.loadingError.textContent = message;
      el.configError.textContent = message;
      el.loadingTitle.textContent = "Provisioning failed";
      el.loadingCopy.textContent = "Fix the configuration and retry.";
      renderModuleStateMachine();
    }
  });
}

async function connectIdentityWithProvider() {
  const provider = normalizeIdentityProvider(
    el.identityProvider ? el.identityProvider.value : state.identityProvider
  );
  state.identityProvider = provider;
  state.identityEntryPath = identityPathForProvider(provider);

  let client = null;
  if (shouldInitializePrivyClient()) {
    setPrivyStatus("Initializing Privy...");
    client = await getPrivyClient();
  }

  if (provider === "wallet") {
    await connectWalletIdentity(client);
    return;
  }

  if (!client) {
    throw new Error(
      "Privy App ID is required for email/social identity. Configure GATEWAY_FRONTDOOR_PRIVY_APP_ID."
    );
  }

  if (provider === "email") {
    await connectPrivyEmailIdentity(client);
    return;
  }

  await connectPrivyOauthIdentity(client, provider);
}

function shouldInitializePrivyClient() {
  const appId =
    state.bootstrap && typeof state.bootstrap.privy_app_id === "string"
      ? state.bootstrap.privy_app_id.trim()
      : "";
  return appId.length > 0;
}

async function connectWalletIdentity(client) {
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

  if (client) {
    const privySession = await readCurrentPrivySession(client, state.walletAddress);
    state.privyUserId = privySession.privyUserId || buildPrivyWalletHandle(state.walletAddress);
    state.privyIdentityToken = privySession.identityToken;
    state.privyAccessToken = privySession.accessToken;
    el.privyUserId.value = state.privyUserId;
    if (privySession.walletLinked) {
      setPrivyStatus("Wallet connected and linked to Privy");
    } else {
      setPrivyStatus("Wallet connected. Privy session initialized");
    }
  } else {
    state.privyUserId = "";
    state.privyIdentityToken = "";
    state.privyAccessToken = "";
    el.privyUserId.value = "";
    setPrivyStatus("Wallet connected");
  }

  state.walletBindingVerified = true;
  renderIdentityEntryPathState();
  renderModuleStateMachine();
}

async function connectPrivyEmailIdentity(client) {
  if (!client || !client.auth || !client.auth.email) {
    throw new Error("Privy email login is unavailable.");
  }
  const email = String(
    window.prompt("Enter your email for Privy login:", "")
  ).trim();
  if (!email) {
    throw new Error("Email login canceled.");
  }
  if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email)) {
    throw new Error("Enter a valid email address.");
  }

  setPrivyStatus("Sending email code...");
  await client.auth.email.sendCode(email);
  const code = String(
    window.prompt("Enter the one-time code sent to " + email + ":", "")
  ).trim();
  if (!code) {
    throw new Error("Email verification code is required.");
  }

  setPrivyStatus("Verifying email code...");
  const authResponse = await client.auth.email.loginWithCode(
    email,
    code,
    "login-or-sign-up"
  );
  await applyPrivyAuthResponse(client, authResponse, "Email login complete");
}

async function connectPrivyOauthIdentity(client, provider) {
  if (!client || !client.auth || !client.auth.oauth) {
    throw new Error("Privy social login is unavailable.");
  }

  setPrivyStatus("Opening " + providerLabel(provider) + " login...");
  const authResponse = await loginWithPrivyOauthPopup(client, provider);
  await applyPrivyAuthResponse(
    client,
    authResponse,
    providerLabel(provider) + " login complete"
  );
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
    (authResponse &&
      typeof authResponse.identity_token === "string" &&
      authResponse.identity_token.trim()) ||
    (await readPrivyToken(client, "identity")) ||
    "";
  state.privyAccessToken =
    (authResponse &&
      typeof authResponse.privy_access_token === "string" &&
      authResponse.privy_access_token.trim()) ||
    (await readPrivyToken(client, "access")) ||
    "";
  el.privyUserId.value = state.privyUserId;

  if (!state.walletAddress) {
    state.walletBindingVerified = false;
    setPrivyStatus(successStatus + ". Wallet binding required.");
  } else {
    state.walletBindingVerified = true;
    setPrivyStatus(successStatus + ". Wallet already bound.");
  }
  renderIdentityEntryPathState();
  renderModuleStateMachine();
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
  const normalizedWallet = normalizeOptionalWallet(walletAddress);
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
    if (!account || typeof account !== "object") {
      continue;
    }
    const address =
      typeof account.address === "string" ? normalizeOptionalWallet(account.address) : null;
    if (address === normalizedWallet) {
      return true;
    }
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
        code:
          payload.code && typeof payload.code === "string"
            ? payload.code.trim()
            : "",
        state:
          payload.state && typeof payload.state === "string"
            ? payload.state.trim()
            : "",
        error:
          payload.error && typeof payload.error === "string"
            ? payload.error.trim()
            : "",
      });
    }

    window.addEventListener("message", onMessage);
  });
}

async function getPrivyClient() {
  if (state.privyClient) {
    return state.privyClient;
  }
  const appId =
    state.bootstrap && typeof state.bootstrap.privy_app_id === "string"
      ? state.bootstrap.privy_app_id.trim()
      : "";
  if (!appId) {
    throw new Error("Privy App ID is required for identity authentication.");
  }

  let sdk;
  try {
    sdk = await import(PRIVY_SDK_IMPORT_URL);
  } catch (err) {
    throw new Error(
      "Failed to load Privy SDK: " + String(err && err.message ? err.message : err)
    );
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
  const clientId =
    state.bootstrap && typeof state.bootstrap.privy_client_id === "string"
      ? state.bootstrap.privy_client_id.trim()
      : "";
  if (clientId) {
    options.clientId = clientId;
  }

  const client = new PrivyClient(options);
  await client.initialize();
  state.privyClient = client;
  return client;
}

function extractPrivyUser(payload) {
  if (!payload || typeof payload !== "object") {
    return null;
  }
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
    profile_domain: state.selectedPolicyDomain || "general",
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
  if (typeof config.profile_domain === "string" && config.profile_domain.trim()) {
    state.selectedPolicyDomain = config.profile_domain.trim();
    if (el.policyTemplateDomain) {
      el.policyTemplateDomain.value = state.selectedPolicyDomain;
    }
    if (el.policyTemplateSelect) {
      const matchingTemplate = state.policyTemplates.find(
        (item) => item.domain === state.selectedPolicyDomain
      );
      if (matchingTemplate) {
        state.selectedPolicyTemplate = matchingTemplate.id;
        el.policyTemplateSelect.value = matchingTemplate.id;
      }
    }
    syncSelectedPolicyTemplateMetadata();
  }

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
  renderModuleStateMachine();
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
      setTypedFailureText(el.loadingError, "Status poll failed", err);
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
  state.sessionSnapshot = session;
  if (session && typeof session.session_id === "string") {
    state.sessionId = session.session_id;
  }
  syncTypedSurfacesFromSession(session);
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
  renderRuntimeControlState();
  renderModuleStateMachine();
  renderProofSpotlight();

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
  renderModuleStateMachine();
  renderProofSpotlight();
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
      setTypedFailureText(el.monitorError, "Session monitor load failed", err);
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
    const sessionRef = String(session.session_ref || "-");
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
      escapeHtml(sessionRef) +
      "\">" +
      escapeHtml(sessionRef) +
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

function renderTypedPlaceholders() {
  state.typed.onboarding = null;
  state.typed.timeline = null;
  state.typed.verification = null;
  state.typed.todos = null;
  state.typed.funding = null;
  state.typed.runtimeControl = null;

  if (el.onboardingSummary) {
    el.onboardingSummary.textContent = "Load a session to inspect onboarding progress.";
  }
  if (el.onboardingTranscript) {
    el.onboardingTranscript.innerHTML = '<p class="typed-empty">No onboarding transcript loaded.</p>';
  }
  if (el.timelineSummary) {
    el.timelineSummary.textContent = "No timeline loaded.";
  }
  if (el.timelineEvents) {
    el.timelineEvents.innerHTML = '<p class="typed-empty">No timeline events loaded.</p>';
  }
  if (el.verificationExplanationKv) {
    el.verificationExplanationKv.innerHTML = '<p class="typed-empty">No verification explanation loaded.</p>';
  }
  if (el.gatewayTodoSummary) {
    el.gatewayTodoSummary.textContent = "No TODO summary loaded.";
  }
  if (el.gatewayTodoList) {
    el.gatewayTodoList.innerHTML = '<p class="typed-empty">No gateway TODOs loaded.</p>';
  }
  if (el.runtimeControlSummary) {
    el.runtimeControlSummary.textContent = "Load a session to inspect and apply runtime controls.";
  }
  if (el.fundingPreflightSummary) {
    el.fundingPreflightSummary.textContent = "No funding preflight loaded.";
  }
  if (el.fundingPreflightChecks) {
    el.fundingPreflightChecks.innerHTML = '<p class="typed-empty">No funding preflight checks loaded.</p>';
  }
  if (el.sendOnboardingChatBtn) {
    el.sendOnboardingChatBtn.disabled = true;
  }
  if (el.onboardingConfirmPlanBtn) {
    el.onboardingConfirmPlanBtn.disabled = true;
  }
  if (el.onboardingConfirmSignBtn) {
    el.onboardingConfirmSignBtn.disabled = true;
  }
  setRuntimeActionButtonsDisabled(true);
  clearTypedSessionError();
  clearTypedModuleErrors();
  renderProofSpotlight();
  renderRuntimeControlState();
  renderModuleStateMachine();
}

function clearTypedSessionError() {
  if (el.typedSessionError) {
    el.typedSessionError.textContent = "";
  }
}

function setTypedSessionError(message) {
  if (el.typedSessionError) {
    el.typedSessionError.textContent = message;
  }
}

function clearTypedModuleErrors() {
  state.typed.errors.onboarding = "";
  state.typed.errors.timeline = "";
  state.typed.errors.verification = "";
  state.typed.errors.todos = "";
  state.typed.errors.funding = "";
  state.typed.errors.runtimeControl = "";
  if (el.onboardingError) el.onboardingError.textContent = "";
  if (el.timelineError) el.timelineError.textContent = "";
  if (el.verificationError) el.verificationError.textContent = "";
  if (el.gatewayTodoError) el.gatewayTodoError.textContent = "";
  if (el.fundingPreflightError) el.fundingPreflightError.textContent = "";
  if (el.runtimeControlError) el.runtimeControlError.textContent = "";
  if (el.proofError) el.proofError.textContent = "";
}

function looksLikeSessionId(value) {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
    String(value || "").trim()
  );
}

function normalizeSessionId(value) {
  const trimmed = String(value || "").trim();
  if (!trimmed) {
    throw new Error("Session ID is required.");
  }
  if (!looksLikeSessionId(trimmed)) {
    throw new Error("Session ID must be a UUID.");
  }
  return trimmed.toLowerCase();
}

function readTypedSessionIdFromInput() {
  if (!el.typedSessionId) {
    throw new Error("Typed session input is unavailable.");
  }
  return normalizeSessionId(el.typedSessionId.value);
}

function setTypedSessionId(sessionId) {
  const normalized = normalizeSessionId(sessionId);
  state.typedSessionId = normalized;
  if (el.typedSessionId) {
    el.typedSessionId.value = normalized;
  }
  updateOnboardingActionButtons(state.typed.onboarding);
  setRuntimeActionButtonsDisabled(false);
  renderRuntimeControlState();
  renderModuleStateMachine();
}

function syncTypedSurfacesFromSession(session) {
  const sessionId = String(session && session.session_id ? session.session_id : "").trim();
  if (!looksLikeSessionId(sessionId)) {
    return;
  }

  const normalizedSessionId = sessionId.toLowerCase();
  const status = String(session && session.status ? session.status : "").trim();

  if (state.typedSessionId !== normalizedSessionId) {
    state.typedLastStatus = "";
    setTypedSessionId(normalizedSessionId);
  }
  if (!status || state.typedLastStatus === status) {
    return;
  }

  state.typedLastStatus = status;
  refreshTypedApiSurfaces(normalizedSessionId).catch((err) => {
    setTypedSessionError(formatTypedError("Typed surface refresh failed", err));
  });
}

async function refreshTypedApiSurfaces(sessionId, options) {
  const opts = options || {};
  const normalizedSessionId = normalizeSessionId(
    sessionId || state.typedSessionId || (el.typedSessionId ? el.typedSessionId.value : "")
  );
  setTypedSessionId(normalizedSessionId);
  clearTypedSessionError();

  if (!opts.skipOnboarding && el.onboardingError) {
    el.onboardingError.textContent = "";
    state.typed.errors.onboarding = "";
  }
  if (el.timelineError) {
    el.timelineError.textContent = "";
    state.typed.errors.timeline = "";
  }
  if (el.verificationError) {
    el.verificationError.textContent = "";
    state.typed.errors.verification = "";
  }
  if (el.gatewayTodoError) {
    el.gatewayTodoError.textContent = "";
    state.typed.errors.todos = "";
  }
  if (el.fundingPreflightError) {
    el.fundingPreflightError.textContent = "";
    state.typed.errors.funding = "";
  }
  if (el.runtimeControlError) {
    el.runtimeControlError.textContent = "";
    state.typed.errors.runtimeControl = "";
  }

  if (el.loadTypedSessionBtn) {
    el.loadTypedSessionBtn.disabled = true;
  }

  let successCount = 0;
  let attemptedCount = 0;

  attemptedCount += 1;
  try {
    const sessionPayload = await fetchJson(
      "/api/frontdoor/session/" + encodeURIComponent(normalizedSessionId)
    );
    const sessionModel = parseSessionResponse(sessionPayload);
    state.sessionSnapshot = sessionModel;
    state.sessionId = sessionModel.session_id;
    state.typedLastStatus = sessionModel.status;
    renderSessionKv({
      wallet: sessionModel.wallet_address,
      session: sessionModel.session_id,
      version: sessionModel.version,
      status: sessionModel.status,
      profile: sessionModel.profile_name,
      provisioningSource: sessionModel.provisioning_source,
      dedicatedInstance: sessionModel.dedicated_instance,
      launchedOnEigencloud: sessionModel.launched_on_eigencloud,
      verificationBackend: sessionModel.verification_backend,
      verificationLevel: sessionModel.verification_level,
      fallbackSigned: sessionModel.verification_fallback_require_signed_receipts,
      appId: sessionModel.eigen_app_id,
      verifyUrl: sessionModel.verify_url,
    });
    successCount += 1;
  } catch (err) {
    setTypedSessionError(formatTypedError("Session state failed", err));
  }

  if (!opts.skipOnboarding) {
    attemptedCount += 1;
    try {
      const onboardingPayload = await fetchJson(
        "/api/frontdoor/onboarding/state?session_id=" + encodeURIComponent(normalizedSessionId)
      );
      const onboardingState = parseOnboardingStateResponse(onboardingPayload);
      renderOnboardingState(onboardingState);
      state.typed.onboarding = onboardingState;
      state.typed.errors.onboarding = "";
      successCount += 1;
    } catch (err) {
      state.typed.errors.onboarding = formatTypedError("", err);
      state.typed.onboarding = null;
      if (el.onboardingError) {
        setTypedFailureText(el.onboardingError, "Onboarding state failed", err);
      }
      updateOnboardingActionButtons(null);
    }
  }

  attemptedCount += 1;
  try {
    const timelinePayload = await fetchJson(
      "/api/frontdoor/session/" +
        encodeURIComponent(normalizedSessionId) +
        "/timeline"
    );
    const timeline = parseTimelineResponse(timelinePayload);
    renderTimeline(timeline);
    state.typed.timeline = timeline;
    state.typed.errors.timeline = "";
    successCount += 1;
  } catch (err) {
    state.typed.timeline = null;
    state.typed.errors.timeline = formatTypedError("", err);
    if (el.timelineError) {
      setTypedFailureText(el.timelineError, "Timeline load failed", err);
    }
  }

  attemptedCount += 1;
  try {
    const verificationPayload = await fetchJson(
      "/api/frontdoor/session/" +
        encodeURIComponent(normalizedSessionId) +
        "/verification-explanation"
    );
    const explanation = parseVerificationExplanationResponse(verificationPayload);
    renderVerificationExplanation(explanation);
    state.typed.verification = explanation;
    state.typed.errors.verification = "";
    successCount += 1;
  } catch (err) {
    state.typed.verification = null;
    state.typed.errors.verification = formatTypedError("", err);
    if (el.verificationError) {
      setTypedFailureText(
        el.verificationError,
        "Verification explanation failed",
        err
      );
    }
  }

  attemptedCount += 1;
  try {
    const todosPayload = await fetchJson(
      "/api/frontdoor/session/" +
        encodeURIComponent(normalizedSessionId) +
        "/gateway-todos"
    );
    const todos = parseGatewayTodosResponse(todosPayload);
    renderGatewayTodos(todos);
    state.typed.todos = todos;
    state.typed.errors.todos = "";
    successCount += 1;
  } catch (err) {
    state.typed.todos = null;
    state.typed.errors.todos = formatTypedError("", err);
    if (el.gatewayTodoError) {
      setTypedFailureText(el.gatewayTodoError, "Gateway TODO load failed", err);
    }
  }

  attemptedCount += 1;
  try {
    const fundingPayload = await fetchJson(
      "/api/frontdoor/session/" +
        encodeURIComponent(normalizedSessionId) +
        "/funding-preflight"
    );
    const funding = parseFundingPreflightResponse(fundingPayload);
    renderFundingPreflight(funding);
    state.typed.funding = funding;
    state.typed.errors.funding = "";
    successCount += 1;
  } catch (err) {
    state.typed.funding = null;
    state.typed.errors.funding = formatTypedError("", err);
    if (el.fundingPreflightError) {
      setTypedFailureText(
        el.fundingPreflightError,
        "Funding preflight load failed",
        err
      );
    }
  }

  if (el.loadTypedSessionBtn) {
    el.loadTypedSessionBtn.disabled = false;
  }

  renderRuntimeControlState();
  renderProofSpotlight();
  renderModuleStateMachine();

  if (successCount === 0 && attemptedCount > 0) {
    setTypedSessionError("No typed session surfaces loaded. Verify the session ID and retry.");
    return false;
  }
  clearTypedSessionError();
  return true;
}

function readActiveSessionId() {
  let sessionId = String(state.typedSessionId || state.sessionId || "").trim();
  if (sessionId) {
    return normalizeSessionId(sessionId);
  }
  sessionId = readTypedSessionIdFromInput();
  setTypedSessionId(sessionId);
  return normalizeSessionId(sessionId);
}

async function postOnboardingChatMessage(sessionId, message) {
  const payload = await fetchJson("/api/frontdoor/onboarding/chat", {
    method: "POST",
    body: {
      session_id: sessionId,
      message,
    },
  });
  const response = parseOnboardingChatResponse(payload);
  if (response.session_id.toLowerCase() !== sessionId.toLowerCase()) {
    throw createFrontdoorError(
      "Onboarding chat returned a mismatched session_id.",
      "FRONTDOOR_ONBOARDING_SESSION_MISMATCH",
      "Reload the session and retry onboarding actions with the same session_id."
    );
  }
  renderOnboardingState(response.state);
  state.typed.onboarding = response.state;
  state.typed.errors.onboarding = "";
  return response.state;
}

async function sendOnboardingPresetMessage(message) {
  if (el.onboardingError) {
    el.onboardingError.textContent = "";
  }
  clearTypedSessionError();
  let sessionId = "";
  try {
    sessionId = readActiveSessionId();
  } catch (err) {
    if (el.onboardingError) {
      setTypedFailureText(el.onboardingError, "Onboarding action blocked", err);
    }
    return;
  }

  if (el.sendOnboardingChatBtn) {
    el.sendOnboardingChatBtn.disabled = true;
  }
  if (el.onboardingConfirmPlanBtn) {
    el.onboardingConfirmPlanBtn.disabled = true;
  }
  if (el.onboardingConfirmSignBtn) {
    el.onboardingConfirmSignBtn.disabled = true;
  }
  try {
    await postOnboardingChatMessage(sessionId, message);
    await refreshTypedApiSurfaces(sessionId, { skipOnboarding: true });
  } catch (err) {
    state.typed.errors.onboarding = formatTypedError("", err);
    if (el.onboardingError) {
      setTypedFailureText(el.onboardingError, "Onboarding action failed", err);
    }
  } finally {
    updateOnboardingActionButtons(state.typed.onboarding);
  }
}

function buildOnboardingObjective(cfg) {
  const intent = String(el.intentPrompt && el.intentPrompt.value ? el.intentPrompt.value : "").trim();
  if (intent) return intent;
  const inferred = String((cfg && cfg.inference_summary) || "").trim();
  if (inferred) return inferred;
  const profileName = String((cfg && cfg.profile_name) || "frontdoor_profile").trim();
  return (
    "Launch profile " +
    profileName +
    " with deterministic verification, strict policy gates, and auditable runtime controls."
  );
}

function buildOnboardingAssignmentsMessage(cfg) {
  const profileName = String((cfg && cfg.profile_name) || "frontdoor_profile")
    .trim()
    .replace(/[\n\r,;=]/g, "_");
  const acceptTerms = cfg && cfg.accept_terms ? "true" : "false";
  return (
    "profile_name=" +
    profileName +
    ", gateway_auth_key=__from_config__, accept_terms=" +
    acceptTerms
  );
}

async function ensureOnboardingReadyForLaunch(sessionId, cfg) {
  const normalizedSessionId = normalizeSessionId(sessionId);
  const objective = buildOnboardingObjective(cfg);
  const assignments = buildOnboardingAssignmentsMessage(cfg);

  let onboardingState = parseOnboardingStateResponse(
    await fetchJson(
      "/api/frontdoor/onboarding/state?session_id=" + encodeURIComponent(normalizedSessionId)
    )
  );
  renderOnboardingState(onboardingState);
  state.typed.onboarding = onboardingState;
  state.typed.errors.onboarding = "";

  if (!onboardingState.objective) {
    onboardingState = await postOnboardingChatMessage(normalizedSessionId, objective);
  }
  if (onboardingState.missing_fields.length > 0) {
    onboardingState = await postOnboardingChatMessage(normalizedSessionId, assignments);
  }
  if (onboardingState.current_step !== "ready_to_sign" && !onboardingState.completed) {
    onboardingState = await postOnboardingChatMessage(normalizedSessionId, "confirm plan");
  }
  if (onboardingState.missing_fields.length > 0) {
    onboardingState = await postOnboardingChatMessage(normalizedSessionId, assignments);
    onboardingState = await postOnboardingChatMessage(normalizedSessionId, "confirm plan");
  }
  if (onboardingState.missing_fields.length > 0) {
    throw createFrontdoorError(
      "Onboarding step-3 still has unresolved required variables: " +
        onboardingState.missing_fields.join(", "),
      "FRONTDOOR_ONBOARDING_REQUIRED_VARIABLES",
      "Set profile_name, accept_terms, and gateway_auth_key source before signing."
    );
  }
  if (onboardingState.current_step !== "ready_to_sign" && !onboardingState.completed) {
    onboardingState = await postOnboardingChatMessage(normalizedSessionId, "confirm sign");
  }
  if (onboardingState.current_step !== "ready_to_sign" && !onboardingState.completed) {
    throw createFrontdoorError(
      "Onboarding did not reach ready_to_sign terminal state.",
      "FRONTDOOR_ONBOARDING_STEP4_INCOMPLETE",
      "Run onboarding confirm plan/sign actions, then relaunch."
    );
  }
  updateOnboardingActionButtons(onboardingState);
}

async function sendOnboardingChatMessage() {
  if (el.onboardingError) {
    el.onboardingError.textContent = "";
  }
  clearTypedSessionError();

  let sessionId = "";
  try {
    sessionId = readActiveSessionId();
  } catch (err) {
    if (el.onboardingError) {
      setTypedFailureText(el.onboardingError, "Onboarding action blocked", err);
    }
    return;
  }

  const message = String(el.onboardingChatInput && el.onboardingChatInput.value ? el.onboardingChatInput.value : "").trim();
  if (!message) {
    if (el.onboardingError) {
      el.onboardingError.textContent = "Onboarding message is required.";
    }
    return;
  }

  if (el.sendOnboardingChatBtn) {
    el.sendOnboardingChatBtn.disabled = true;
  }
  if (el.onboardingConfirmPlanBtn) {
    el.onboardingConfirmPlanBtn.disabled = true;
  }
  if (el.onboardingConfirmSignBtn) {
    el.onboardingConfirmSignBtn.disabled = true;
  }
  try {
    await postOnboardingChatMessage(sessionId, message);
    if (el.onboardingChatInput) {
      el.onboardingChatInput.value = "";
    }
    await refreshTypedApiSurfaces(sessionId, { skipOnboarding: true });
  } catch (err) {
    state.typed.errors.onboarding = formatTypedError("", err);
    if (el.onboardingError) {
      setTypedFailureText(el.onboardingError, "Onboarding chat send failed", err);
    }
  } finally {
    updateOnboardingActionButtons(state.typed.onboarding);
  }
}

function renderOnboardingState(stateModel) {
  if (!el.onboardingSummary || !el.onboardingTranscript) {
    return;
  }
  const missing = stateModel.missing_fields.length
    ? stateModel.missing_fields.join(", ")
    : "none";
  const objective = stateModel.objective ? " Objective: " + stateModel.objective + "." : "";
  const step4 =
    stateModel.step4_payload && typeof stateModel.step4_payload.signature_action === "string"
      ? " Step4: " + stateModel.step4_payload.signature_action + "."
      : "";
  el.onboardingSummary.textContent =
    "Step: " +
    stateModel.current_step +
    ". Completed: " +
    (stateModel.completed ? "yes" : "no") +
    ". Missing: " +
    missing +
    ". Updated: " +
    formatTimestamp(stateModel.updated_at) +
    "." +
    objective +
    step4;

  if (!stateModel.transcript.length) {
    el.onboardingTranscript.innerHTML = '<p class="typed-empty">No onboarding transcript turns yet.</p>';
    updateOnboardingActionButtons(stateModel);
    return;
  }

  const turns = stateModel.transcript.map((turn) => {
    return (
      '<div class="typed-turn">' +
      '<p class="typed-turn-meta">' +
      escapeHtml(turn.role) +
      " | " +
      escapeHtml(formatTimestamp(turn.created_at)) +
      "</p>" +
      '<p class="typed-turn-body">' +
      escapeHtml(turn.message) +
      "</p>" +
      "</div>"
    );
  });
  el.onboardingTranscript.innerHTML = turns.join("");
  updateOnboardingActionButtons(stateModel);
}

function updateOnboardingActionButtons(stateModel) {
  const hasSession = looksLikeSessionId(
    String(state.typedSessionId || state.sessionId || "").trim()
  );
  const model = stateModel || state.typed.onboarding;
  const hasState = !!model;
  const missingCount =
    hasState && Array.isArray(model.missing_fields) ? model.missing_fields.length : 0;
  const currentStep = hasState ? String(model.current_step || "") : "";
  const completed = hasState ? !!model.completed : false;

  if (el.sendOnboardingChatBtn) {
    el.sendOnboardingChatBtn.disabled = !hasSession;
  }
  if (el.onboardingConfirmPlanBtn) {
    const planDisabled =
      !hasSession ||
      !hasState ||
      completed ||
      currentStep === "ready_to_sign";
    el.onboardingConfirmPlanBtn.disabled = planDisabled;
  }
  if (el.onboardingConfirmSignBtn) {
    const signDisabled =
      !hasSession ||
      !hasState ||
      completed ||
      missingCount > 0 ||
      (currentStep !== "confirm_and_sign" && currentStep !== "ready_to_sign");
    el.onboardingConfirmSignBtn.disabled = signDisabled;
  }
}

function renderTimeline(model) {
  if (!el.timelineSummary || !el.timelineEvents) {
    return;
  }
  el.timelineSummary.textContent =
    "Session: " +
    model.session_id +
    ". Events loaded: " +
    String(model.events.length) +
    ".";
  if (!model.events.length) {
    el.timelineEvents.innerHTML = '<p class="typed-empty">No timeline events returned.</p>';
    return;
  }

  const rows = model.events.map((event) => {
    const statusClass = "typed-pill typed-pill-" + sanitizeToken(event.status);
    return (
      '<div class="typed-timeline-item">' +
      '<div class="typed-item-head">' +
      '<p class="typed-item-title">#' +
      String(event.seq_id) +
      " " +
      escapeHtml(event.event_type) +
      "</p>" +
      '<div class="typed-pills"><span class="' +
      escapeHtml(statusClass) +
      '">' +
      escapeHtml(event.status) +
      "</span></div>" +
      "</div>" +
      '<p class="typed-item-time">' +
      escapeHtml(formatTimestamp(event.created_at)) +
      " | " +
      escapeHtml(event.actor) +
      "</p>" +
      '<p class="typed-item-body">' +
      escapeHtml(event.detail) +
      "</p>" +
      "</div>"
    );
  });
  el.timelineEvents.innerHTML = rows.join("");
}

function renderVerificationExplanation(model) {
  if (!el.verificationExplanationKv) {
    return;
  }
  const failureReason = model.failure_reason || "none";
  el.verificationExplanationKv.innerHTML =
    '<div class="typed-kv-row"><span class="typed-kv-key">Session</span><span class="typed-kv-value">' +
    escapeHtml(model.session_id) +
    "</span></div>" +
    '<div class="typed-kv-row"><span class="typed-kv-key">Backend</span><span class="typed-kv-value">' +
    escapeHtml(model.backend) +
    "</span></div>" +
    '<div class="typed-kv-row"><span class="typed-kv-key">Assurance</span><span class="typed-kv-value">' +
    escapeHtml(model.level) +
    "</span></div>" +
    '<div class="typed-kv-row"><span class="typed-kv-key">Fallback Used</span><span class="typed-kv-value">' +
    escapeHtml(model.fallback_used ? "yes" : "no") +
    "</span></div>" +
    '<div class="typed-kv-row"><span class="typed-kv-key">Latency</span><span class="typed-kv-value">' +
    escapeHtml(String(model.latency_ms)) +
    " ms</span></div>" +
    '<div class="typed-kv-row"><span class="typed-kv-key">Failure Reason</span><span class="typed-kv-value">' +
    escapeHtml(failureReason) +
    "</span></div>";
}

function renderGatewayTodos(model) {
  if (!el.gatewayTodoSummary || !el.gatewayTodoList) {
    return;
  }
  el.gatewayTodoSummary.textContent =
    model.todo_status_summary +
    " Required open: " +
    String(model.todo_open_required_count) +
    ". Recommended open: " +
    String(model.todo_open_recommended_count) +
    ".";

  if (!model.todos.length) {
    el.gatewayTodoList.innerHTML = '<p class="typed-empty">No TODO items returned.</p>';
    return;
  }

  const rows = model.todos.map((todo) => {
    const severityClass = "typed-pill typed-pill-" + sanitizeToken(todo.severity);
    const statusClass = "typed-pill typed-pill-" + sanitizeToken(todo.status);
    return (
      '<div class="typed-todo-item">' +
      '<div class="typed-item-head">' +
      '<p class="typed-item-title">' +
      escapeHtml(todo.todo_id) +
      "</p>" +
      '<div class="typed-pills">' +
      '<span class="' +
      escapeHtml(severityClass) +
      '">' +
      escapeHtml(todo.severity) +
      "</span>" +
      '<span class="' +
      escapeHtml(statusClass) +
      '">' +
      escapeHtml(todo.status) +
      "</span>" +
      "</div>" +
      "</div>" +
      '<p class="typed-item-time">Owner: ' +
      escapeHtml(todo.owner) +
      "</p>" +
      '<p class="typed-item-body">' +
      escapeHtml(todo.action) +
      "</p>" +
      "</div>"
    );
  });
  el.gatewayTodoList.innerHTML = rows.join("");
}

async function loadExperienceManifest() {
  const fallbackContracts = buildDefaultModuleContracts();
  state.moduleContracts = fallbackContracts;

  let manifest = null;
  let configContract = null;
  let manifestError = "";
  let contractError = "";

  try {
    manifest = parseExperienceManifestResponse(
      await fetchJson("/api/frontdoor/experience/manifest")
    );
    state.experienceManifest = manifest;
  } catch (err) {
    state.experienceManifest = null;
    manifestError = formatTypedError("", err);
  }

  try {
    configContract = parseConfigContractResponse(
      await fetchJson("/api/frontdoor/config-contract")
    );
    state.configContract = configContract;
  } catch (err) {
    state.configContract = null;
    contractError = formatTypedError("", err);
  }

  if (manifest && Array.isArray(manifest.steps)) {
    const mergedContracts = buildDefaultModuleContracts();
    const moduleIdMap = {
      identity: "identity",
      policy: "policy",
      verification: "verification",
      provisioning: "provisioning",
      runtimecontrols: "runtimecontrols",
      runtime_controls: "runtimecontrols",
      evidence: "evidence",
    };
    for (let i = 0; i < manifest.steps.length; i += 1) {
      const step = manifest.steps[i];
      const moduleId = moduleIdMap[String(step.step_id || "").toLowerCase()];
      if (!moduleId || !mergedContracts[moduleId]) {
        continue;
      }
      mergedContracts[moduleId] = {
        title: step.title,
        purpose_id: step.purpose_id,
        backend_contract: step.backend_contract,
        artifact_binding: step.artifact_binding,
        success_state: step.success_state,
        failure_state: step.failure_state,
      };
    }
    state.moduleContracts = mergedContracts;
  }

  if (
    configContract &&
    configContract.defaults &&
    typeof configContract.defaults.profile_domain === "string" &&
    !state.selectedPolicyTemplate
  ) {
    const suggested = configContract.defaults.profile_domain.trim();
    if (suggested) {
      state.selectedPolicyDomain = suggested;
      if (el.policyTemplateDomain) {
        el.policyTemplateDomain.value = suggested;
      }
    }
  }

  if (el.moduleStateNote) {
    if (manifest && configContract) {
      el.moduleStateNote.textContent =
        "Experience manifest and config contract loaded. Module contracts are bound to typed backend surfaces.";
    } else if (manifestError || contractError) {
      const detail = [manifestError, contractError].filter(Boolean).join(" | ");
      el.moduleStateNote.textContent =
        "Experience metadata partially unavailable. Using deterministic fallback module contracts. " +
        detail;
    }
  }
  renderModuleStateMachine();
}

async function loadTypedPolicyTemplates() {
  if (!el.policyTemplateSelect) {
    return;
  }
  if (el.policyTemplateMessage) {
    el.policyTemplateMessage.textContent = "";
  }

  try {
    const payload = await fetchJson("/api/frontdoor/policy-templates");
    const parsed = parsePolicyTemplateLibraryResponse(payload);
    state.policyTemplates = parsed.templates;
    state.policyTemplatesEnabled = parsed.templates.length > 0;

    el.policyTemplateSelect.innerHTML = "";
    if (!parsed.templates.length) {
      const option = document.createElement("option");
      option.value = "";
      option.textContent = "No templates available";
      el.policyTemplateSelect.appendChild(option);
      if (el.policyTemplateDetails) {
        el.policyTemplateDetails.textContent =
          "No typed policy templates were returned by the gateway.";
      }
      if (el.applyPolicyTemplateBtn) {
        el.applyPolicyTemplateBtn.disabled = true;
      }
      return;
    }

    for (let i = 0; i < parsed.templates.length; i += 1) {
      const template = parsed.templates[i];
      const option = document.createElement("option");
      option.value = template.id;
      option.textContent = template.title + " [" + template.domain + "]";
      el.policyTemplateSelect.appendChild(option);
    }
    if (el.applyPolicyTemplateBtn) {
      el.applyPolicyTemplateBtn.disabled = false;
    }

    const hasCurrent = parsed.templates.some(
      (template) => template.id === state.selectedPolicyTemplate
    );
    const selectedTemplateId = hasCurrent
      ? state.selectedPolicyTemplate
      : parsed.templates[0].id;
    state.selectedPolicyTemplate = selectedTemplateId;
    el.policyTemplateSelect.value = selectedTemplateId;
    syncSelectedPolicyTemplateMetadata();
  } catch (err) {
    state.policyTemplates = [];
    state.policyTemplatesEnabled = false;
    if (el.policyTemplateDetails) {
      el.policyTemplateDetails.textContent =
        "Typed policy template library unavailable.";
    }
    if (el.policyTemplateMessage) {
      el.policyTemplateMessage.textContent = formatTypedError(
        "Template load failed",
        err
      );
    }
    if (el.applyPolicyTemplateBtn) {
      el.applyPolicyTemplateBtn.disabled = true;
    }
  }
}

function syncSelectedPolicyTemplateMetadata() {
  if (!el.policyTemplateSelect) {
    return;
  }
  state.selectedPolicyTemplate = String(el.policyTemplateSelect.value || "").trim();
  const selected = state.policyTemplates.find(
    (template) => template.id === state.selectedPolicyTemplate
  );
  if (!selected) {
    if (el.policyTemplateDomain) {
      el.policyTemplateDomain.value = state.selectedPolicyDomain || "general";
    }
    if (el.policyTemplateDetails) {
      el.policyTemplateDetails.textContent =
        "Select a typed policy template to inspect objective, risk posture, and module plan.";
    }
    renderModuleStateMachine();
    return;
  }

  state.selectedPolicyDomain = selected.domain;
  if (el.policyTemplateDomain) {
    el.policyTemplateDomain.value = selected.domain;
  }
  if (el.policyTemplateDetails) {
    el.policyTemplateDetails.textContent =
      selected.objective +
      " Risk posture=" +
      selected.riskProfile.posture +
      ", max_position_size_usd=" +
      String(selected.riskProfile.maxPositionSizeUsd) +
      ", max_leverage=" +
      String(selected.riskProfile.maxLeverage) +
      ", max_slippage_bps=" +
      String(selected.riskProfile.maxSlippageBps) +
      ". Module plan: " +
      selected.modulePlan.join(", ") +
      ". " +
      selected.rationale;
  }
  renderModuleStateMachine();
}

function applySelectedPolicyTemplate() {
  if (el.policyTemplateMessage) {
    el.policyTemplateMessage.textContent = "";
  }
  const selected = state.policyTemplates.find(
    (template) => template.id === state.selectedPolicyTemplate
  );
  if (!selected) {
    if (el.policyTemplateMessage) {
      el.policyTemplateMessage.textContent =
        "Select a typed policy template before applying defaults.";
    }
    return;
  }

  state.selectedPolicyDomain = selected.domain;
  if (el.policyTemplateDomain) {
    el.policyTemplateDomain.value = selected.domain;
  }

  setInputValue("paper-live-policy", selected.config.paperLivePolicy);
  setInputValue("custody-mode", selected.config.custodyMode);
  setInputValue("verification-backend", selected.config.verificationBackend);
  setInputValue(
    "information-sharing-scope",
    selected.config.informationSharingScope
  );
  setCheckboxValue(
    "verification-fallback-require-signed",
    selected.config.verificationFallbackRequireSignedReceipts
  );
  if (selected.config.verificationBackend === "fallback_only") {
    setCheckboxValue("verification-fallback-enabled", true);
  }

  setInputValue(
    "max-position-usd",
    String(selected.riskProfile.maxPositionSizeUsd)
  );
  setInputValue("leverage-cap", String(selected.riskProfile.maxLeverage));
  setInputValue("max-leverage", String(selected.riskProfile.maxLeverage));
  setInputValue(
    "max-slippage-bps",
    String(selected.riskProfile.maxSlippageBps)
  );

  const profileInput = document.getElementById("profile-name");
  if (profileInput && !String(profileInput.value || "").trim()) {
    profileInput.value = selected.domain + "-profile-v1";
  }

  syncWalletLinkedInputs(true);
  syncVerificationControls();
  renderModuleStateMachine();
  if (el.policyTemplateMessage) {
    el.policyTemplateMessage.textContent =
      "Applied template defaults: " + selected.title + ".";
  }
}

function renderIdentityEntryPathState() {
  const provider = normalizeIdentityProvider(
    (el.identityProvider && el.identityProvider.value) || state.identityProvider
  );
  state.identityProvider = provider;
  state.identityEntryPath = identityPathForProvider(provider);
  if (el.identityProvider && el.identityProvider.value !== provider) {
    el.identityProvider.value = provider;
  }

  const selectedPath = state.identityEntryPath;
  const walletBound = /^0x[a-fA-F0-9]{40}$/.test(
    String(state.walletAddress || "").trim()
  );
  state.walletBindingVerified = walletBound;

  if (el.identityProviderNote) {
    el.identityProviderNote.textContent =
      "Provider: " + providerLabel(provider) + ".";
  }
  if (el.identityEntryNote) {
    if (selectedPath === "email") {
      el.identityEntryNote.textContent =
        "Email-first path selected through Privy. Wallet binding remains mandatory before provisioning.";
    } else if (selectedPath === "social") {
      el.identityEntryNote.textContent =
        "Social-first path selected through Privy. Wallet binding remains mandatory before provisioning.";
    } else {
      el.identityEntryNote.textContent =
        "Wallet-first path selected for direct gasless authorization signing.";
    }
  }
  if (el.identityBindingNote) {
    if (walletBound) {
      el.identityBindingNote.textContent =
        "Wallet binding verified: " + state.walletAddress + ".";
    } else if (selectedPath === "wallet") {
      el.identityBindingNote.textContent = "Wallet binding pending.";
    } else {
      el.identityBindingNote.textContent =
        "Wallet binding required before launch for non-wallet initial entry.";
    }
  }
  if (el.connectWalletBtn) {
    if (provider === "wallet") {
      el.connectWalletBtn.textContent = "Connect Wallet (Privy)";
    } else if (provider === "email") {
      el.connectWalletBtn.textContent = "Continue With Email (Privy)";
    } else {
      el.connectWalletBtn.textContent =
        "Continue With " + providerLabel(provider) + " (Privy)";
    }
  }
}

function normalizeIdentityProvider(value) {
  const raw = String(value || "").trim().toLowerCase();
  const allowed = [
    "wallet",
    "email",
    "google",
    "apple",
    "github",
    "discord",
    "twitter",
  ];
  return allowed.includes(raw) ? raw : "wallet";
}

function identityPathForProvider(provider) {
  if (provider === "wallet") {
    return "wallet";
  }
  if (provider === "email") {
    return "email";
  }
  return "social";
}

function providerLabel(provider) {
  if (provider === "google") return "Google";
  if (provider === "apple") return "Apple";
  if (provider === "github") return "GitHub";
  if (provider === "discord") return "Discord";
  if (provider === "twitter") return "X / Twitter";
  if (provider === "email") return "Email";
  return "Wallet";
}

function renderModuleStateMachine() {
  const safeWalletAddress = state.walletAddress
    ? String(state.walletAddress).trim()
    : "";
  const hasWallet = /^0x[a-fA-F0-9]{40}$/.test(safeWalletAddress);
  const hasPolicyDraft =
    !!state.lastConfigDraft ||
    (state.policyTemplatesEnabled && !!state.selectedPolicyTemplate);
  const activeSessionId = String(state.typedSessionId || state.sessionId || "").trim();
  const hasSession = looksLikeSessionId(activeSessionId);
  const session = state.sessionSnapshot;
  const sessionStatus = String(
    (session && session.status) || state.typedLastStatus || ""
  ).trim();
  const runtimeState = String(
    (state.typed.runtimeControl && state.typed.runtimeControl.runtime_state) ||
      (session && session.runtime_state) ||
      ""
  ).trim();
  const typedLoadedCount = [
    state.typed.timeline,
    state.typed.verification,
    state.typed.todos,
    state.typed.funding,
  ].filter(Boolean).length;
  const typedErrorCount = Object.values(state.typed.errors).filter(Boolean).length;

  const contracts = state.moduleContracts || buildDefaultModuleContracts();
  for (let i = 0; i < MODULE_IDS.length; i += 1) {
    const moduleId = MODULE_IDS[i];
    const refs = moduleDom[moduleId];
    const contract = contracts[moduleId];
    if (!refs || !contract) {
      continue;
    }
    if (refs.purpose) refs.purpose.textContent = contract.purpose_id || "-";
    if (refs.contract) refs.contract.textContent = contract.backend_contract || "-";
    if (refs.artifact) refs.artifact.textContent = contract.artifact_binding || "-";
    if (refs.success) refs.success.textContent = contract.success_state || "-";
    if (refs.failure) refs.failure.textContent = contract.failure_state || "-";
  }

  updateModuleState(
    "identity",
    hasWallet ? "ready" : "pending",
    hasWallet
      ? "Wallet binding complete. Gasless authorization challenge can be issued."
      : "Connect wallet and bind identity before policy and provisioning.",
    hasWallet ? "wallet_binding:" + safeWalletAddress : "challenge_message pending"
  );

  if (!hasWallet) {
    updateModuleState(
      "policy",
      "locked",
      "Identity module must be complete before policy controls unlock.",
      "frontdoor_user_config pending"
    );
  } else if (hasPolicyDraft) {
    updateModuleState(
      "policy",
      "ready",
      "Policy controls configured and ready for signature submission.",
      state.selectedPolicyTemplate
        ? "policy_template:" + state.selectedPolicyTemplate
        : "frontdoor_user_config drafted"
    );
  } else {
    updateModuleState(
      "policy",
      "active",
      "Capture objective and apply a typed template or suggestion before launch.",
      "frontdoor_user_config pending"
    );
  }

  if (!hasPolicyDraft) {
    updateModuleState(
      "verification",
      "locked",
      "Policy module must be complete before verification can start.",
      "signature_receipt pending"
    );
  } else if (!hasSession) {
    updateModuleState(
      "verification",
      "active",
      "Launch flow to generate challenge and collect signature receipt.",
      "signature_receipt pending"
    );
  } else if (sessionStatus === "failed" || sessionStatus === "expired") {
    updateModuleState(
      "verification",
      "failed",
      "Verification or preflight failed for this session.",
      "session_id:" + activeSessionId
    );
  } else {
    updateModuleState(
      "verification",
      "ready",
      "Verification completed and provisioning handshake accepted.",
      "session_id:" + activeSessionId
    );
  }

  if (!hasSession) {
    updateModuleState(
      "provisioning",
      "locked",
      "Verification completion is required before provisioning starts.",
      "provisioning_receipt pending"
    );
  } else if (sessionStatus === "ready") {
    updateModuleState(
      "provisioning",
      "ready",
      "Provisioning completed and runtime endpoint is available.",
      "provisioning_source:" + String(session && session.provisioning_source)
    );
  } else if (sessionStatus === "failed" || sessionStatus === "expired") {
    updateModuleState(
      "provisioning",
      "failed",
      "Provisioning did not reach a ready terminal state.",
      "status:" + sessionStatus
    );
  } else {
    updateModuleState(
      "provisioning",
      "active",
      "Provisioning in progress. Awaiting terminal backend status.",
      "status:" + (sessionStatus || "pending")
    );
  }

  if (!hasSession) {
    updateModuleState(
      "runtimecontrols",
      "locked",
      "Load a session before runtime controls become available.",
      "runtime_state pending"
    );
  } else if (runtimeState === "terminated") {
    updateModuleState(
      "runtimecontrols",
      "ready",
      "Runtime is terminated. Control actions are now mostly read-only.",
      "runtime_state:terminated"
    );
  } else if (runtimeState) {
    updateModuleState(
      "runtimecontrols",
      "ready",
      "Runtime controls available for pause/resume/terminate/key-rotation.",
      "runtime_state:" + runtimeState
    );
  } else {
    updateModuleState(
      "runtimecontrols",
      "active",
      "Runtime control surface active; waiting for first state response.",
      "runtime_state pending"
    );
  }

  if (!hasSession) {
    updateModuleState(
      "evidence",
      "locked",
      "Session evidence surfaces unlock after launch.",
      "typed evidence pending"
    );
  } else if (typedLoadedCount === 0 && typedErrorCount > 0) {
    updateModuleState(
      "evidence",
      "failed",
      "Typed evidence endpoints failed. Inspect error surfaces.",
      "errors:" + String(typedErrorCount)
    );
  } else if (typedLoadedCount > 0) {
    updateModuleState(
      "evidence",
      "ready",
      "Typed evidence loaded across " + String(typedLoadedCount) + " surfaces.",
      "timeline+verification+todos+funding"
    );
  } else {
    updateModuleState(
      "evidence",
      "active",
      "Evidence surfaces are available; load typed session APIs.",
      "typed evidence loading"
    );
  }

  if (el.moduleStateNote) {
    if (!hasWallet) {
      el.moduleStateNote.textContent =
        "Identity module blocked. Connect wallet to unlock policy and launch path.";
    } else if (!hasSession) {
      el.moduleStateNote.textContent =
        "Identity complete. Configure policy and launch signature flow to open provisioning.";
    } else if (sessionStatus === "ready") {
      el.moduleStateNote.textContent =
        "Session ready. Runtime controls and typed evidence surfaces are available.";
    } else if (sessionStatus === "failed" || sessionStatus === "expired") {
      el.moduleStateNote.textContent =
        "Session reached a failed terminal state. Inspect evidence surfaces for remediation.";
    } else {
      el.moduleStateNote.textContent =
        "Session in progress. Module states follow backend session transitions only.";
    }
  }

  const frontdoorEnabled = !!(state.bootstrap && state.bootstrap.enabled);
  const provisioningReady =
    !state.bootstrap || state.bootstrap.provisioning_backend !== "unconfigured";
  const nextAction = deriveDeterministicNextAction({
    frontdoorEnabled,
    provisioningReady,
    hasWallet,
    hasPolicyDraft,
    hasSession,
    selectedIdentityPath: state.identityEntryPath,
    walletBindingVerified: state.walletBindingVerified,
    sessionStatus,
    typedErrorCount,
  });
  if (el.moduleNextActionNote) {
    el.moduleNextActionNote.textContent =
      "Next action: " +
      nextAction.action +
      " Operator hint: " +
      nextAction.operatorHint;
  }
  syncPrimaryActionsState({
    hasWallet,
    hasPolicyDraft,
    hasSession,
    selectedIdentityPath: state.identityEntryPath,
    walletBindingVerified: state.walletBindingVerified,
  });
  renderModulePopover();
}

function syncPrimaryActionsState(model) {
  const hasWallet = !!model.hasWallet;
  const hasPolicyDraft = !!model.hasPolicyDraft;
  const hasSession = !!model.hasSession;
  const selectedIdentityPath = String(model.selectedIdentityPath || "wallet");
  const walletBindingVerified = !!model.walletBindingVerified;

  const frontdoorEnabled = !!(state.bootstrap && state.bootstrap.enabled);
  const provisioningReady =
    !state.bootstrap || state.bootstrap.provisioning_backend !== "unconfigured";

  if (el.suggestConfigBtn) {
    const suggestBlocked = !frontdoorEnabled || !hasWallet;
    el.suggestConfigBtn.disabled = suggestBlocked;
    el.suggestConfigBtn.title = suggestBlocked
      ? "Connect wallet before requesting a config suggestion."
      : "";
  }

  if (el.launchSessionBtn) {
    let launchBlock = "";
    if (!frontdoorEnabled) {
      launchBlock = "Frontdoor is disabled.";
    } else if (!provisioningReady) {
      launchBlock = "Provisioning backend is unconfigured.";
    } else if (!hasWallet) {
      launchBlock = "Connect wallet before launching.";
    } else if (
      (selectedIdentityPath === "email" || selectedIdentityPath === "social") &&
      !walletBindingVerified
    ) {
      launchBlock =
        "Wallet binding is required after email/social login before launch.";
    } else if (!hasPolicyDraft) {
      launchBlock = "Capture objective and produce policy config before launch.";
    } else if (hasSession) {
      launchBlock = "Session already active. Load or complete current session first.";
    }

    if (launchBlock) {
      el.launchSessionBtn.disabled = true;
      el.launchSessionBtn.title = launchBlock;
    } else {
      el.launchSessionBtn.disabled = false;
      el.launchSessionBtn.title = "";
    }
  }
}

function deriveDeterministicNextAction(model) {
  const selectedIdentityPath = String(model.selectedIdentityPath || "wallet");
  const sessionStatus = String(model.sessionStatus || "").trim();
  const hasTypedErrors = Number(model.typedErrorCount || 0) > 0;

  if (!model.frontdoorEnabled) {
    return {
      action: "Enable frontdoor mode before launch.",
      operatorHint: "Set frontdoor enablement in gateway config and restart the service.",
    };
  }
  if (!model.provisioningReady) {
    return {
      action: "Configure a provisioning backend before launch.",
      operatorHint:
        "Set GATEWAY_FRONTDOOR_PROVISION_COMMAND, or explicitly enable default fallback URL.",
    };
  }
  if (!model.hasWallet) {
    return {
      action: "Connect an EVM wallet and bind identity.",
      operatorHint: "Use the Privy connect button and confirm account access in the wallet.",
    };
  }
  if (
    (selectedIdentityPath === "email" || selectedIdentityPath === "social") &&
    !model.walletBindingVerified
  ) {
    return {
      action: "Complete wallet binding after non-wallet identity entry.",
      operatorHint: "Run wallet connect again and verify a 0x wallet address is populated.",
    };
  }
  if (!model.hasPolicyDraft) {
    return {
      action: "Generate or apply a policy configuration before launch.",
      operatorHint:
        "Use Suggest Config or apply a policy template, then verify required fields are valid.",
    };
  }
  if (!model.hasSession) {
    return {
      action: "Submit launch to create challenge, complete onboarding confirmation, and sign.",
      operatorHint: "Launch executes challenge -> onboarding step-4 -> signature -> verify.",
    };
  }
  if (sessionStatus === "failed" || sessionStatus === "expired") {
    return {
      action: "Inspect typed evidence and remediate the failed session.",
      operatorHint:
        "Load timeline, verification explanation, and funding checks; then relaunch with corrected config.",
    };
  }
  if (sessionStatus === "ready") {
    return {
      action: "Operate the running enclave or inspect evidence surfaces.",
      operatorHint: hasTypedErrors
        ? "Typed evidence has errors; reload typed session APIs to restore full proof posture."
        : "Use runtime controls, TODO feed, and verification explanation for ongoing operations.",
    };
  }
  return {
    action: "Wait for provisioning to reach a terminal backend state.",
    operatorHint: "Session polling is active; monitor timeline and gateway TODO status.",
  };
}

function updateModuleState(moduleId, status, summary, artifactValue) {
  const refs = moduleDom[moduleId];
  if (!refs || !refs.card || !refs.state || !refs.summary || !refs.artifactValue) {
    return;
  }

  const normalized = String(status || "locked").toLowerCase();
  refs.state.textContent = normalized;
  refs.state.classList.remove(
    "state-ready",
    "state-active",
    "state-pending",
    "state-failed",
    "state-locked"
  );
  refs.card.classList.remove(
    "module-ready",
    "module-active",
    "module-failed",
    "module-locked",
    "module-deferred"
  );

  if (normalized === "ready") {
    refs.state.classList.add("state-ready");
    refs.card.classList.add("module-ready");
  } else if (normalized === "failed") {
    refs.state.classList.add("state-failed");
    refs.card.classList.add("module-failed");
  } else if (normalized === "active") {
    refs.state.classList.add("state-active");
    refs.card.classList.add("module-active");
  } else if (normalized === "pending") {
    refs.state.classList.add("state-pending");
    refs.card.classList.add("module-deferred");
  } else {
    refs.state.classList.add("state-locked");
    refs.card.classList.add("module-locked");
  }

  refs.summary.textContent = summary || "No summary available.";
  refs.artifactValue.textContent = artifactValue || "Artifact pending.";
}

function renderProofSpotlight() {
  const session = state.sessionSnapshot;
  const verification = state.typed.verification;
  const timeline = state.typed.timeline;
  const todos = state.typed.todos;
  const runtime = state.typed.runtimeControl;

  if (el.proofVerification) {
    if (verification) {
      el.proofVerification.textContent =
        verification.backend +
        " / " +
        verification.level +
        " | fallback=" +
        (verification.fallback_used ? "enabled" : "disabled") +
        " | latency=" +
        String(verification.latency_ms) +
        "ms";
    } else if (session) {
      el.proofVerification.textContent =
        String(session.verification_backend || "-") +
        " / " +
        String(session.verification_level || "-");
    } else {
      el.proofVerification.textContent = "No verification evidence loaded.";
    }
  }

  if (el.proofTimeline) {
    if (timeline) {
      el.proofTimeline.textContent =
        String(timeline.events.length) + " timeline events loaded.";
    } else {
      el.proofTimeline.textContent = "No session timeline loaded.";
    }
  }

  if (el.proofProvisioning) {
    if (session) {
      el.proofProvisioning.textContent =
        String(session.provisioning_source || "unknown") +
        " | dedicated=" +
        (session.dedicated_instance ? "yes" : "no") +
        " | eigencloud=" +
        (session.launched_on_eigencloud ? "yes" : "no");
    } else {
      el.proofProvisioning.textContent = "No provisioning source loaded.";
    }
  }

  if (el.proofFallback) {
    if (session) {
      el.proofFallback.textContent =
        "fallback_enabled=" +
        (session.verification_fallback_enabled ? "true" : "false") +
        " | signed_required=" +
        (session.verification_fallback_require_signed_receipts ? "true" : "false");
    } else {
      el.proofFallback.textContent = "No fallback posture loaded.";
    }
  }

  if (el.proofRuntime) {
    const runtimeState =
      (runtime && runtime.runtime_state) ||
      (session && session.runtime_state) ||
      null;
    if (runtimeState) {
      el.proofRuntime.textContent = "runtime_state=" + runtimeState;
    } else {
      el.proofRuntime.textContent = "No runtime state loaded.";
    }
  }

  if (el.proofTodo) {
    if (todos) {
      el.proofTodo.textContent =
        todos.todo_status_summary +
        " | required_open=" +
        String(todos.todo_open_required_count) +
        " | recommended_open=" +
        String(todos.todo_open_recommended_count);
    } else {
      el.proofTodo.textContent = "No TODO posture loaded.";
    }
  }

  if (el.proofError) {
    const errors = Object.values(state.typed.errors).filter(Boolean);
    el.proofError.textContent = errors.length
      ? "Typed surface errors: " + errors.join(" | ")
      : "";
  }
}

function renderRuntimeControlState() {
  if (!el.runtimeControlSummary) {
    return;
  }

  const sessionId = String(state.typedSessionId || state.sessionId || "").trim();
  if (!looksLikeSessionId(sessionId)) {
    el.runtimeControlSummary.textContent =
      "Load a session to inspect and apply runtime controls.";
    setRuntimeActionButtonsDisabled(true);
    return;
  }

  const runtimeResponse = state.typed.runtimeControl;
  const runtimeState = String(
    (runtimeResponse && runtimeResponse.runtime_state) ||
      (state.sessionSnapshot && state.sessionSnapshot.runtime_state) ||
      "unknown"
  );
  const responseStatus = runtimeResponse ? runtimeResponse.status : "pending";
  const responseDetail = runtimeResponse
    ? runtimeResponse.detail
    : "No runtime control action applied yet.";
  const updatedAt = runtimeResponse ? formatTimestamp(runtimeResponse.updated_at) : "-";

  el.runtimeControlSummary.textContent =
    "Runtime state: " +
    runtimeState +
    ". Last action status: " +
    responseStatus +
    ". Detail: " +
    responseDetail +
    ". Updated: " +
    updatedAt +
    ".";

  const pending = state.runtimeActionPending;
  const terminated = runtimeState === "terminated";
  const paused = runtimeState === "paused";
  const running = runtimeState === "running";
  if (el.runtimePauseBtn) {
    el.runtimePauseBtn.disabled = pending || terminated || paused;
  }
  if (el.runtimeResumeBtn) {
    el.runtimeResumeBtn.disabled = pending || terminated || running;
  }
  if (el.runtimeTerminateBtn) {
    el.runtimeTerminateBtn.disabled = pending || terminated;
  }
  if (el.runtimeRotateKeyBtn) {
    el.runtimeRotateKeyBtn.disabled = pending;
  }
}

function setRuntimeActionButtonsDisabled(disabled) {
  const flag = !!disabled;
  if (el.runtimePauseBtn) el.runtimePauseBtn.disabled = flag;
  if (el.runtimeResumeBtn) el.runtimeResumeBtn.disabled = flag;
  if (el.runtimeTerminateBtn) el.runtimeTerminateBtn.disabled = flag;
  if (el.runtimeRotateKeyBtn) el.runtimeRotateKeyBtn.disabled = flag;
}

async function applyRuntimeControlAction(action) {
  if (state.runtimeActionPending) {
    return;
  }
  const sessionId = String(state.typedSessionId || state.sessionId || "").trim();
  if (!looksLikeSessionId(sessionId)) {
    setTypedSessionError("Load a valid session before applying runtime controls.");
    return;
  }
  if (el.runtimeControlError) {
    el.runtimeControlError.textContent = "";
  }
  state.runtimeActionPending = true;
  renderRuntimeControlState();

  try {
    const payload = await fetchJson(
      "/api/frontdoor/session/" +
        encodeURIComponent(sessionId) +
        "/runtime-control",
      {
        method: "POST",
        body: {
          action,
          actor: "frontdoor_ui",
        },
      }
    );
    const model = parseRuntimeControlResponse(payload);
    state.typed.runtimeControl = model;
    state.typed.errors.runtimeControl = "";
    await refreshTypedApiSurfaces(sessionId, { skipOnboarding: true });
    if (model.status === "blocked" && el.runtimeControlError) {
      el.runtimeControlError.textContent = model.detail;
    }
  } catch (err) {
    state.typed.errors.runtimeControl = formatTypedError("", err);
    if (el.runtimeControlError) {
      setTypedFailureText(el.runtimeControlError, "Runtime control failed", err);
    }
  } finally {
    state.runtimeActionPending = false;
    renderRuntimeControlState();
    renderProofSpotlight();
    renderModuleStateMachine();
  }
}

function renderFundingPreflight(model) {
  if (!el.fundingPreflightSummary || !el.fundingPreflightChecks) {
    return;
  }
  const failureCategory = model.failure_category
    ? " Failure category: " + model.failure_category + "."
    : "";
  el.fundingPreflightSummary.textContent =
    "Status: " +
    model.status +
    "." +
    failureCategory +
    " Updated: " +
    formatTimestamp(model.updated_at) +
    ".";

  if (!model.checks.length) {
    el.fundingPreflightChecks.innerHTML =
      '<p class="typed-empty">No funding preflight checks returned.</p>';
    return;
  }

  const rows = model.checks.map((check) => {
    const statusClass = "typed-pill typed-pill-" + sanitizeToken(check.status);
    return (
      '<div class="typed-todo-item">' +
      '<div class="typed-item-head">' +
      '<p class="typed-item-title">' +
      escapeHtml(check.check_id) +
      "</p>" +
      '<span class="' +
      escapeHtml(statusClass) +
      '">' +
      escapeHtml(check.status) +
      "</span>" +
      "</div>" +
      '<p class="typed-item-body">' +
      escapeHtml(check.detail) +
      "</p>" +
      "</div>"
    );
  });
  el.fundingPreflightChecks.innerHTML = rows.join("");
}

function parsePolicyTemplateLibraryResponse(payload) {
  const root = expectObject(payload, "Policy template library response");
  const templates = expectArrayField(
    root,
    "templates",
    "Policy template library response"
  ).map((template, idx) => {
    const model = expectObject(
      template,
      "Policy template library response templates[" + String(idx) + "]"
    );
    const risk = expectObject(
      model.risk_profile,
      "Policy template templates[" + String(idx) + "].risk_profile"
    );
    const cfg = expectObject(
      model.config,
      "Policy template templates[" + String(idx) + "].config"
    );
    return {
      id: expectStringField(
        model,
        "template_id",
        "Policy template library response templates[" + String(idx) + "]"
      ),
      title: expectStringField(
        model,
        "title",
        "Policy template library response templates[" + String(idx) + "]"
      ),
      domain: expectStringField(
        model,
        "domain",
        "Policy template library response templates[" + String(idx) + "]"
      ),
      objective: expectStringField(
        model,
        "objective",
        "Policy template library response templates[" + String(idx) + "]"
      ),
      rationale: expectStringField(
        model,
        "rationale",
        "Policy template library response templates[" + String(idx) + "]"
      ),
      modulePlan: expectStringArrayField(
        model,
        "module_plan",
        "Policy template library response templates[" + String(idx) + "]"
      ),
      riskProfile: {
        posture: expectStringField(
          risk,
          "posture",
          "Policy template templates[" + String(idx) + "].risk_profile"
        ),
        maxPositionSizeUsd: expectNumberField(
          risk,
          "max_position_size_usd",
          "Policy template templates[" + String(idx) + "].risk_profile"
        ),
        maxLeverage: expectNumberField(
          risk,
          "max_leverage",
          "Policy template templates[" + String(idx) + "].risk_profile"
        ),
        maxSlippageBps: expectNumberField(
          risk,
          "max_slippage_bps",
          "Policy template templates[" + String(idx) + "].risk_profile"
        ),
      },
      config: {
        paperLivePolicy: expectStringField(
          cfg,
          "paper_live_policy",
          "Policy template templates[" + String(idx) + "].config"
        ),
        custodyMode: expectStringField(
          cfg,
          "custody_mode",
          "Policy template templates[" + String(idx) + "].config"
        ),
        verificationBackend: expectStringField(
          cfg,
          "verification_backend",
          "Policy template templates[" + String(idx) + "].config"
        ),
        verificationFallbackRequireSignedReceipts: expectBooleanField(
          cfg,
          "verification_fallback_require_signed_receipts",
          "Policy template templates[" + String(idx) + "].config"
        ),
        informationSharingScope: expectStringField(
          cfg,
          "information_sharing_scope",
          "Policy template templates[" + String(idx) + "].config"
        ),
      },
    };
  });
  return {
    generated_at: expectStringField(
      root,
      "generated_at",
      "Policy template library response"
    ),
    templates,
  };
}

function parseExperienceManifestResponse(payload) {
  const root = expectObject(payload, "Experience manifest response");
  const steps = expectArrayField(root, "steps", "Experience manifest response").map(
    (step, idx) => {
      const model = expectObject(
        step,
        "Experience manifest response steps[" + String(idx) + "]"
      );
      return {
        step_id: expectStringField(
          model,
          "step_id",
          "Experience manifest response steps[" + String(idx) + "]"
        ),
        title: expectStringField(
          model,
          "title",
          "Experience manifest response steps[" + String(idx) + "]"
        ),
        purpose_id: expectStringField(
          model,
          "purpose_id",
          "Experience manifest response steps[" + String(idx) + "]"
        ),
        backend_contract: expectStringField(
          model,
          "backend_contract",
          "Experience manifest response steps[" + String(idx) + "]"
        ),
        artifact_binding: expectStringField(
          model,
          "artifact_binding",
          "Experience manifest response steps[" + String(idx) + "]"
        ),
        success_state: expectStringField(
          model,
          "success_state",
          "Experience manifest response steps[" + String(idx) + "]"
        ),
        failure_state: expectStringField(
          model,
          "failure_state",
          "Experience manifest response steps[" + String(idx) + "]"
        ),
      };
    }
  );
  return {
    manifest_version: expectNumberField(
      root,
      "manifest_version",
      "Experience manifest response"
    ),
    steps,
  };
}

function parseConfigContractResponse(payload) {
  const root = expectObject(payload, "Config contract response");
  const defaults = expectObject(root.defaults, "Config contract response.defaults");
  return {
    current_config_version: expectNumberField(
      root,
      "current_config_version",
      "Config contract response"
    ),
    defaults: {
      profile_domain: expectStringField(
        defaults,
        "profile_domain",
        "Config contract response.defaults"
      ),
    },
  };
}

function parseSessionResponse(payload) {
  const root = expectObject(payload, "Session response");
  return {
    session_id: expectStringField(root, "session_id", "Session response"),
    wallet_address: expectStringField(root, "wallet_address", "Session response"),
    privy_user_id: optionalStringField(root, "privy_user_id", "Session response"),
    version: expectNumberField(root, "version", "Session response"),
    status: expectStringField(root, "status", "Session response"),
    detail: expectStringField(root, "detail", "Session response"),
    provisioning_source: expectStringField(
      root,
      "provisioning_source",
      "Session response"
    ),
    dedicated_instance: expectBooleanField(
      root,
      "dedicated_instance",
      "Session response"
    ),
    launched_on_eigencloud: expectBooleanField(
      root,
      "launched_on_eigencloud",
      "Session response"
    ),
    verification_backend: expectStringField(
      root,
      "verification_backend",
      "Session response"
    ),
    verification_level: expectStringField(root, "verification_level", "Session response"),
    verification_fallback_enabled: expectBooleanField(
      root,
      "verification_fallback_enabled",
      "Session response"
    ),
    verification_fallback_require_signed_receipts: expectBooleanField(
      root,
      "verification_fallback_require_signed_receipts",
      "Session response"
    ),
    instance_url: optionalStringField(root, "instance_url", "Session response"),
    verify_url: optionalStringField(root, "verify_url", "Session response"),
    eigen_app_id: optionalStringField(root, "eigen_app_id", "Session response"),
    error: optionalStringField(root, "error", "Session response"),
    created_at: expectStringField(root, "created_at", "Session response"),
    updated_at: expectStringField(root, "updated_at", "Session response"),
    expires_at: expectStringField(root, "expires_at", "Session response"),
    profile_name: optionalStringField(root, "profile_name", "Session response"),
    todo_open_required_count: expectNumberField(
      root,
      "todo_open_required_count",
      "Session response"
    ),
    todo_open_recommended_count: expectNumberField(
      root,
      "todo_open_recommended_count",
      "Session response"
    ),
    todo_status_summary: expectStringField(root, "todo_status_summary", "Session response"),
    runtime_state: expectStringField(root, "runtime_state", "Session response"),
    funding_preflight_status: expectStringField(
      root,
      "funding_preflight_status",
      "Session response"
    ),
    funding_preflight_failure_category: optionalStringField(
      root,
      "funding_preflight_failure_category",
      "Session response"
    ),
  };
}

function parseRuntimeControlResponse(payload) {
  const root = expectObject(payload, "Runtime control response");
  return {
    session_id: expectStringField(root, "session_id", "Runtime control response"),
    action: expectStringField(root, "action", "Runtime control response"),
    status: expectStringField(root, "status", "Runtime control response"),
    runtime_state: expectStringField(root, "runtime_state", "Runtime control response"),
    detail: expectStringField(root, "detail", "Runtime control response"),
    updated_at: expectStringField(root, "updated_at", "Runtime control response"),
  };
}

function parseFundingPreflightResponse(payload) {
  const root = expectObject(payload, "Funding preflight response");
  const checks = expectArrayField(root, "checks", "Funding preflight response").map(
    (check, idx) => {
      const model = expectObject(
        check,
        "Funding preflight response checks[" + String(idx) + "]"
      );
      return {
        check_id: expectStringField(
          model,
          "check_id",
          "Funding preflight response checks[" + String(idx) + "]"
        ),
        status: expectStringField(
          model,
          "status",
          "Funding preflight response checks[" + String(idx) + "]"
        ),
        detail: expectStringField(
          model,
          "detail",
          "Funding preflight response checks[" + String(idx) + "]"
        ),
      };
    }
  );
  return {
    session_id: expectStringField(root, "session_id", "Funding preflight response"),
    status: expectStringField(root, "status", "Funding preflight response"),
    failure_category: optionalStringField(
      root,
      "failure_category",
      "Funding preflight response"
    ),
    checks,
    updated_at: expectStringField(root, "updated_at", "Funding preflight response"),
  };
}

function parseOnboardingChatResponse(payload) {
  const root = expectObject(payload, "Onboarding chat response");
  return {
    session_id: expectStringField(root, "session_id", "Onboarding chat response"),
    assistant_message: expectStringField(
      root,
      "assistant_message",
      "Onboarding chat response"
    ),
    state: parseOnboardingStateResponse(root.state),
  };
}

function parseOnboardingStateResponse(payload) {
  const root = expectObject(payload, "Onboarding state response");
  let step4Payload = null;
  if (root.step4_payload !== undefined && root.step4_payload !== null) {
    const step4 = expectObject(root.step4_payload, "Onboarding state response.step4_payload");
    step4Payload = {
      ready_to_sign: expectBooleanField(
        step4,
        "ready_to_sign",
        "Onboarding state response.step4_payload"
      ),
      confirmation_required: expectBooleanField(
        step4,
        "confirmation_required",
        "Onboarding state response.step4_payload"
      ),
      unresolved_required_fields: expectStringArrayField(
        step4,
        "unresolved_required_fields",
        "Onboarding state response.step4_payload"
      ),
      signature_action: expectStringField(
        step4,
        "signature_action",
        "Onboarding state response.step4_payload"
      ),
    };
  }
  const transcript = expectArrayField(root, "transcript", "Onboarding state response").map(
    (turn, idx) => {
      const model = expectObject(turn, "Onboarding state transcript[" + String(idx) + "]");
      return {
        role: expectStringField(
          model,
          "role",
          "Onboarding state transcript[" + String(idx) + "]"
        ),
        message: expectStringField(
          model,
          "message",
          "Onboarding state transcript[" + String(idx) + "]"
        ),
        created_at: expectStringField(
          model,
          "created_at",
          "Onboarding state transcript[" + String(idx) + "]"
        ),
      };
    }
  );
  return {
    session_id: expectStringField(root, "session_id", "Onboarding state response"),
    current_step: expectStringField(root, "current_step", "Onboarding state response"),
    completed: expectBooleanField(root, "completed", "Onboarding state response"),
    objective: optionalStringField(root, "objective", "Onboarding state response"),
    missing_fields: expectStringArrayField(
      root,
      "missing_fields",
      "Onboarding state response"
    ),
    step4_payload: step4Payload,
    transcript,
    updated_at: expectStringField(root, "updated_at", "Onboarding state response"),
  };
}

function parseTimelineResponse(payload) {
  const root = expectObject(payload, "Timeline response");
  const events = expectArrayField(root, "events", "Timeline response").map((event, idx) => {
    const model = expectObject(event, "Timeline response events[" + String(idx) + "]");
    return {
      seq_id: expectNumberField(model, "seq_id", "Timeline response events[" + String(idx) + "]"),
      event_type: expectStringField(
        model,
        "event_type",
        "Timeline response events[" + String(idx) + "]"
      ),
      status: expectStringField(model, "status", "Timeline response events[" + String(idx) + "]"),
      detail: expectStringField(model, "detail", "Timeline response events[" + String(idx) + "]"),
      actor: expectStringField(model, "actor", "Timeline response events[" + String(idx) + "]"),
      created_at: expectStringField(
        model,
        "created_at",
        "Timeline response events[" + String(idx) + "]"
      ),
    };
  });
  return {
    session_id: expectStringField(root, "session_id", "Timeline response"),
    events,
  };
}

function parseVerificationExplanationResponse(payload) {
  const root = expectObject(payload, "Verification explanation response");
  return {
    session_id: expectStringField(root, "session_id", "Verification explanation response"),
    backend: expectStringField(root, "backend", "Verification explanation response"),
    level: expectStringField(root, "level", "Verification explanation response"),
    fallback_used: expectBooleanField(
      root,
      "fallback_used",
      "Verification explanation response"
    ),
    latency_ms: expectNumberField(root, "latency_ms", "Verification explanation response"),
    failure_reason: optionalStringField(
      root,
      "failure_reason",
      "Verification explanation response"
    ),
  };
}

function parseGatewayTodosResponse(payload) {
  const root = expectObject(payload, "Gateway TODO response");
  const todos = expectArrayField(root, "todos", "Gateway TODO response").map((todo, idx) => {
    const item = expectObject(todo, "Gateway TODO response todos[" + String(idx) + "]");
    const refs = expectObject(
      item.evidence_refs,
      "Gateway TODO response todos[" + String(idx) + "].evidence_refs"
    );
    return {
      todo_id: expectStringField(item, "todo_id", "Gateway TODO response todos[" + String(idx) + "]"),
      severity: expectStringField(
        item,
        "severity",
        "Gateway TODO response todos[" + String(idx) + "]"
      ),
      status: expectStringField(item, "status", "Gateway TODO response todos[" + String(idx) + "]"),
      owner: expectStringField(item, "owner", "Gateway TODO response todos[" + String(idx) + "]"),
      action: expectStringField(item, "action", "Gateway TODO response todos[" + String(idx) + "]"),
      evidence_refs: {
        session_id: expectStringField(
          refs,
          "session_id",
          "Gateway TODO response todos[" + String(idx) + "].evidence_refs"
        ),
        provisioning_source: expectStringField(
          refs,
          "provisioning_source",
          "Gateway TODO response todos[" + String(idx) + "].evidence_refs"
        ),
        verification_level: expectStringField(
          refs,
          "verification_level",
          "Gateway TODO response todos[" + String(idx) + "].evidence_refs"
        ),
        module_state: expectStringField(
          refs,
          "module_state",
          "Gateway TODO response todos[" + String(idx) + "].evidence_refs"
        ),
        control_state: expectStringField(
          refs,
          "control_state",
          "Gateway TODO response todos[" + String(idx) + "].evidence_refs"
        ),
      },
    };
  });

  return {
    session_id: expectStringField(root, "session_id", "Gateway TODO response"),
    todo_open_required_count: expectNumberField(
      root,
      "todo_open_required_count",
      "Gateway TODO response"
    ),
    todo_open_recommended_count: expectNumberField(
      root,
      "todo_open_recommended_count",
      "Gateway TODO response"
    ),
    todo_status_summary: expectStringField(root, "todo_status_summary", "Gateway TODO response"),
    todos,
  };
}

function expectObject(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(label + " must be an object.");
  }
  return value;
}

function expectStringField(model, key, label) {
  if (typeof model[key] !== "string") {
    throw new Error(label + "." + key + " must be a string.");
  }
  return model[key];
}

function optionalStringField(model, key, label) {
  const value = model[key];
  if (value === undefined || value === null) {
    return null;
  }
  if (typeof value !== "string") {
    throw new Error(label + "." + key + " must be a string when provided.");
  }
  return value;
}

function expectBooleanField(model, key, label) {
  if (typeof model[key] !== "boolean") {
    throw new Error(label + "." + key + " must be a boolean.");
  }
  return model[key];
}

function expectNumberField(model, key, label) {
  const value = model[key];
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(label + "." + key + " must be a finite number.");
  }
  return value;
}

function expectArrayField(model, key, label) {
  const value = model[key];
  if (!Array.isArray(value)) {
    throw new Error(label + "." + key + " must be an array.");
  }
  return value;
}

function expectStringArrayField(model, key, label) {
  const value = expectArrayField(model, key, label);
  for (let i = 0; i < value.length; i += 1) {
    if (typeof value[i] !== "string") {
      throw new Error(label + "." + key + "[" + String(i) + "] must be a string.");
    }
  }
  return value;
}

function sanitizeToken(value) {
  const token = String(value || "unknown").toLowerCase();
  return token.replace(/[^a-z0-9_-]/g, "_");
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

function normalizeFrontdoorApiFailure(status, payload, text, url) {
  const body =
    payload && typeof payload === "object" && !Array.isArray(payload) ? payload : null;
  const endpoint = (() => {
    try {
      return new URL(String(url || ""), window.location.origin).pathname;
    } catch (_) {
      return String(url || "");
    }
  })();
  const detail =
    (body && (body.error || body.message || body.detail)) ||
    text ||
    (status >= 500 ? "Gateway internal error." : "Gateway request failed.");
  const lower = String(detail).toLowerCase();

  let code = "FRONTDOOR_REQUEST_FAILED";
  let operatorHint =
    "Inspect the request payload and gateway logs, then retry with the same session.";

  if (status === 400 && lower.includes("wallet_address query parameter is required")) {
    code = "FRONTDOOR_WALLET_FILTER_REQUIRED";
    operatorHint = "Provide wallet_address=0x... query parameter for monitor endpoints.";
  } else if (lower.includes("invalid session id")) {
    code = "FRONTDOOR_INVALID_SESSION_ID";
    operatorHint = "Use a valid UUID session_id from challenge or session monitor payload.";
  } else if (lower.includes("session not found")) {
    code = "FRONTDOOR_SESSION_NOT_FOUND";
    operatorHint = "Reload active sessions and use a non-expired session identifier.";
  } else if (lower.includes("challenge expired")) {
    code = "FRONTDOOR_CHALLENGE_EXPIRED";
    operatorHint = "Start launch again to request a fresh challenge and signature.";
  } else if (lower.includes("wallet_address does not match challenge session")) {
    code = "FRONTDOOR_CHALLENGE_WALLET_MISMATCH";
    operatorHint = "Reconnect the wallet used for challenge creation before signing.";
  } else if (lower.includes("signed message does not match challenge")) {
    code = "FRONTDOOR_SIGNATURE_MESSAGE_MISMATCH";
    operatorHint = "Sign the exact challenge message returned by the gateway without edits.";
  } else if (lower.includes("funding preflight failed")) {
    code = "FRONTDOOR_PREFLIGHT_FAILED";
    operatorHint =
      "Inspect funding preflight checks and satisfy wallet/auth/policy/gas/fee requirements.";
  } else if (
    lower.includes("no valid provisioning command configured") ||
    lower.includes("provisioning backend is unconfigured")
  ) {
    code = "FRONTDOOR_PROVISIONING_UNCONFIGURED";
    operatorHint =
      "Configure GATEWAY_FRONTDOOR_PROVISION_COMMAND or explicitly opt-in fallback URL.";
  } else if (lower.includes("privy app id is required")) {
    code = "FRONTDOOR_PRIVY_APP_ID_MISSING";
    operatorHint = "Set GATEWAY_FRONTDOOR_PRIVY_APP_ID (or canonical aliases) in environment.";
  } else if (status >= 500) {
    code = "FRONTDOOR_INTERNAL_ERROR";
    operatorHint = "Check server logs for endpoint failure and retry after backend recovery.";
  }

  if (body && typeof body.error_code === "string" && body.error_code.trim()) {
    code = body.error_code.trim();
  }
  if (body && typeof body.operator_hint === "string" && body.operator_hint.trim()) {
    operatorHint = body.operator_hint.trim();
  }

  return {
    message: String(detail || "Gateway request failed."),
    code,
    operatorHint,
    status,
    endpoint,
  };
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
    const normalized = normalizeFrontdoorApiFailure(
      res.status,
      payload,
      text,
      url
    );
    throw Object.assign(new Error(normalized.message), normalized);
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

if (!handlePrivyOauthPopupCallbackWindow()) {
  main();
}
