// Enclagent Web Gateway - Client

let token = '';
let eventSource = null;
let logEventSource = null;
let currentTab = 'chat';
let currentThreadId = null;
let assistantThreadId = null;
let hasMore = false;
let oldestTimestamp = null;
let loadingOlder = false;
let jobEvents = new Map(); // job_id -> Array of events
let jobListRefreshTimer = null;
const JOB_EVENTS_CAP = 500;
const MEMORY_SEARCH_QUERY_MAX_LENGTH = 100;
const OPS_REFRESH_INTERVAL_MS = 30000;
let opsRefreshTimer = null;
const QUICK_SURFACE_REFRESH_INTERVAL_MS = 15000;
const QUICK_LOG_LIMIT = 120;
let quickSurfaceRefreshTimer = null;
let settingsPanelVisible = false;
let recentLogs = [];
const quickSurfaceState = { logs: false, history: false, usage: false };
let gatewayRequiresAuth = true;
let frontdoorModeEnabled = false;
let appBootstrapped = false;

// --- Auth ---

function setLaunchpadVisibility(enabled) {
  const launchpadTabBtn = document.getElementById('launchpad-tab-btn');
  const launchpadPanel = document.getElementById('tab-launchpad');
  const launchpadFrame = document.getElementById('frontdoor-embed');
  const tabButtons = Array.from(document.querySelectorAll('.tab-bar button[data-tab]'));
  if (!launchpadTabBtn || !launchpadPanel) return;

  for (const button of tabButtons) {
    const tab = button.getAttribute('data-tab');
    if (tab !== 'launchpad') {
      button.classList.toggle('hidden-tab', !!enabled);
    }
  }

  launchpadTabBtn.classList.toggle('hidden-tab', !enabled);
  if (!enabled) {
    launchpadPanel.classList.remove('active');
    if (currentTab === 'launchpad') currentTab = 'chat';
    if (launchpadFrame) launchpadFrame.removeAttribute('src');
    return;
  }

  if (launchpadFrame && !launchpadFrame.getAttribute('src')) {
    launchpadFrame.setAttribute('src', '/frontdoor?embedded=1');
  }
}

function startGatewayApp(initialTab) {
  if (appBootstrapped) return;
  appBootstrapped = true;

  document.getElementById('auth-screen').style.display = 'none';
  document.getElementById('app').style.display = 'flex';
  stripTokenFromUrl();

  if (frontdoorModeEnabled) {
    initRuntimeInstanceLabel();
    if (initialTab) switchTab(initialTab);
    return;
  }

  connectSSE();
  connectLogSSE();
  initRuntimeInstanceLabel();
  startGatewayStatusPolling();
  loadThreads();
  loadMemoryTree();
  refreshPrivateStorageLink();
  loadJobs();
  loadSkills();
  loadOpsDashboard();
  startOpsRefresh();
  startQuickSurfaceRefresh();

  if (initialTab) switchTab(initialTab);
}

function handleAuthFailure(message) {
  stopOpsRefresh();
  stopQuickSurfaceRefresh();
  appBootstrapped = false;
  sessionStorage.removeItem('enclagent_token');
  document.getElementById('auth-screen').style.display = '';
  document.getElementById('app').style.display = 'none';
  document.getElementById('auth-error').textContent = message;
}

function authenticate() {
  if (gatewayRequiresAuth) {
    token = document.getElementById('token-input').value.trim();
    if (!token) {
      document.getElementById('auth-error').textContent = 'Token required';
      return;
    }
  } else {
    token = '';
  }

  // Test request before opening app surfaces.
  apiFetch('/api/chat/threads')
    .then(() => {
      if (gatewayRequiresAuth) {
        sessionStorage.setItem('enclagent_token', token);
      } else {
        sessionStorage.removeItem('enclagent_token');
      }
      const defaultTab = frontdoorModeEnabled ? 'launchpad' : 'chat';
      startGatewayApp(defaultTab);
    })
    .catch(() => {
      const message = gatewayRequiresAuth ? 'Invalid token' : 'Gateway bootstrap failed';
      handleAuthFailure(message);
    });
}

function stripTokenFromUrl() {
  // Strip token from URL so it's not visible in the address bar.
  const cleaned = new URL(window.location);
  cleaned.searchParams.delete('token');
  cleaned.hash = '';
  window.history.replaceState({}, '', cleaned.pathname + cleaned.search);
}

function tryTokenAutoAuth() {
  const params = new URLSearchParams(window.location.search);
  const urlToken = params.get('token') || tokenFromHash();
  if (urlToken) {
    document.getElementById('token-input').value = urlToken;
    authenticate();
    return;
  }
  const saved = sessionStorage.getItem('enclagent_token');
  if (saved) {
    document.getElementById('token-input').value = saved;
    // Hide auth screen immediately to prevent flash, authenticate() will
    // restore it if the token turns out to be invalid.
    document.getElementById('auth-screen').style.display = 'none';
    document.getElementById('app').style.display = 'flex';
    authenticate();
  }
}

function initializeGatewayMode() {
  fetch('/api/frontdoor/bootstrap')
    .then((res) => (res.ok ? res.json() : null))
    .then((bootstrap) => {
      frontdoorModeEnabled = !!(bootstrap && bootstrap.enabled);
      gatewayRequiresAuth = !frontdoorModeEnabled;
      setLaunchpadVisibility(frontdoorModeEnabled);
      if (frontdoorModeEnabled) {
        authenticate();
      } else {
        tryTokenAutoAuth();
      }
    })
    .catch(() => {
      frontdoorModeEnabled = false;
      gatewayRequiresAuth = true;
      setLaunchpadVisibility(false);
      tryTokenAutoAuth();
    });
}

document.getElementById('token-input').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') authenticate();
});

initializeGatewayMode();

function tokenFromHash() {
  const rawHash = String(window.location.hash || '');
  if (!rawHash) return null;
  const body = rawHash.startsWith('#') ? rawHash.slice(1) : rawHash;
  if (!body) return null;
  const query = body.includes('?') ? body.slice(body.indexOf('?') + 1) : body;
  const params = new URLSearchParams(query);
  const tokenFromFragment = params.get('token');
  return tokenFromFragment && tokenFromFragment.trim() ? tokenFromFragment.trim() : null;
}

function initRuntimeInstanceLabel() {
  const instanceEl = document.getElementById('runtime-instance');
  if (!instanceEl) return;
  const host = window.location.host || 'local';
  instanceEl.textContent = 'runtime@' + host;
}

function setSseConnectionState(open) {
  const dot = document.getElementById('sse-dot');
  const status = document.getElementById('sse-status');
  if (!dot || !status) return;
  if (open) {
    dot.classList.remove('disconnected');
    status.textContent = 'Open';
  } else {
    dot.classList.add('disconnected');
    status.textContent = 'Reconnecting';
  }
}

// --- API helper ---

function apiFetch(path, options) {
  const opts = options || {};
  opts.headers = opts.headers || {};
  if (token) {
    opts.headers['Authorization'] = 'Bearer ' + token;
  }
  if (opts.body && typeof opts.body === 'object') {
    opts.headers['Content-Type'] = 'application/json';
    opts.body = JSON.stringify(opts.body);
  }
  return fetch(path, opts).then((res) => {
    if (!res.ok) throw new Error(res.status + ' ' + res.statusText);
    return res.json();
  });
}

// --- SSE ---

function connectSSE() {
  if (eventSource) eventSource.close();

  const sseUrl = token
    ? '/api/chat/events?token=' + encodeURIComponent(token)
    : '/api/chat/events';
  eventSource = new EventSource(sseUrl);

  eventSource.onopen = () => {
    setSseConnectionState(true);
  };

  eventSource.onerror = () => {
    setSseConnectionState(false);
  };

  eventSource.addEventListener('response', (e) => {
    const data = JSON.parse(e.data);
    if (!isCurrentThread(data.thread_id)) return;
    addMessage('assistant', data.content);
    setStatus('');
    enableChatInput();
    // Refresh thread list so new titles appear after first message
    loadThreads();
  });

  eventSource.addEventListener('thinking', (e) => {
    const data = JSON.parse(e.data);
    if (!isCurrentThread(data.thread_id)) return;
    setStatus(data.message, true);
  });

  eventSource.addEventListener('tool_started', (e) => {
    const data = JSON.parse(e.data);
    if (!isCurrentThread(data.thread_id)) return;
    setStatus('Running tool: ' + data.name, true);
  });

  eventSource.addEventListener('tool_completed', (e) => {
    const data = JSON.parse(e.data);
    if (!isCurrentThread(data.thread_id)) return;
    const icon = data.success ? '\u2713' : '\u2717';
    setStatus('Tool ' + data.name + ' ' + icon);
  });

  eventSource.addEventListener('stream_chunk', (e) => {
    const data = JSON.parse(e.data);
    if (!isCurrentThread(data.thread_id)) return;
    appendToLastAssistant(data.content);
  });

  eventSource.addEventListener('status', (e) => {
    const data = JSON.parse(e.data);
    if (!isCurrentThread(data.thread_id)) return;
    setStatus(data.message);
    // "Done" and "Awaiting approval" are terminal signals from the agent:
    // the agentic loop finished, so re-enable input as a safety net in case
    // the response SSE event is empty or lost.
    if (data.message === 'Done' || data.message === 'Awaiting approval') {
      enableChatInput();
    }
  });

  eventSource.addEventListener('job_started', (e) => {
    const data = JSON.parse(e.data);
    showJobCard(data);
  });

  eventSource.addEventListener('approval_needed', (e) => {
    const data = JSON.parse(e.data);
    showApproval(data);
  });

  eventSource.addEventListener('auth_required', (e) => {
    const data = JSON.parse(e.data);
    showAuthCard(data);
  });

  eventSource.addEventListener('auth_completed', (e) => {
    const data = JSON.parse(e.data);
    removeAuthCard(data.extension_name);
    showToast(data.message, 'success');
    enableChatInput();
  });

  eventSource.addEventListener('error', (e) => {
    if (e.data) {
      const data = JSON.parse(e.data);
      if (!isCurrentThread(data.thread_id)) return;
      addMessage('system', 'Error: ' + data.message);
      enableChatInput();
    }
  });

  // Job event listeners (activity stream for all sandbox jobs)
  const jobEventTypes = [
    'job_message', 'job_tool_use', 'job_tool_result',
    'job_status', 'job_result'
  ];
  for (const evtType of jobEventTypes) {
    eventSource.addEventListener(evtType, (e) => {
      const data = JSON.parse(e.data);
      const jobId = data.job_id;
      if (!jobId) return;
      if (!jobEvents.has(jobId)) jobEvents.set(jobId, []);
      const events = jobEvents.get(jobId);
      events.push({ type: evtType, data: data, ts: Date.now() });
      // Cap per-job events to prevent memory leak
      while (events.length > JOB_EVENTS_CAP) events.shift();
      // If the Activity tab is currently visible for this job, refresh it
      refreshActivityTab(jobId);
      // Auto-refresh job list when on jobs tab (debounced)
      if ((evtType === 'job_result' || evtType === 'job_status') && currentTab === 'jobs' && !currentJobId) {
        clearTimeout(jobListRefreshTimer);
        jobListRefreshTimer = setTimeout(loadJobs, 200);
      }
      // Clean up finished job events after a viewing window
      if (evtType === 'job_result') {
        setTimeout(() => jobEvents.delete(jobId), 60000);
      }
    });
  }
}

// Check if an SSE event belongs to the currently viewed thread.
// Events without a thread_id (legacy) are always shown.
function isCurrentThread(threadId) {
  if (!threadId) return true;
  if (!currentThreadId) return true;
  return threadId === currentThreadId;
}

// --- Chat ---

function sendMessage() {
  const input = document.getElementById('chat-input');
  const sendBtn = document.getElementById('send-btn');
  const content = input.value.trim();
  if (!content) return;

  addMessage('user', content);
  input.value = '';
  autoResizeTextarea(input);
  setStatus('Sending...', true);

  sendBtn.disabled = true;
  input.disabled = true;

  apiFetch('/api/chat/send', {
    method: 'POST',
    body: { content, thread_id: currentThreadId || undefined },
  }).catch((err) => {
    addMessage('system', 'Failed to send: ' + err.message);
    setStatus('');
    enableChatInput();
  });
}

function enableChatInput() {
  const input = document.getElementById('chat-input');
  const sendBtn = document.getElementById('send-btn');
  sendBtn.disabled = false;
  input.disabled = false;
  input.focus();
}

function sendApprovalAction(requestId, action) {
  apiFetch('/api/chat/approval', {
    method: 'POST',
    body: { request_id: requestId, action: action, thread_id: currentThreadId },
  }).catch((err) => {
    addMessage('system', 'Failed to send approval: ' + err.message);
  });

  // Disable buttons and show confirmation on the card
  const card = document.querySelector('.approval-card[data-request-id="' + requestId + '"]');
  if (card) {
    const buttons = card.querySelectorAll('.approval-actions button');
    buttons.forEach((btn) => {
      btn.disabled = true;
    });
    const actions = card.querySelector('.approval-actions');
    const label = document.createElement('span');
    label.className = 'approval-resolved';
    const labelText = action === 'approve' ? 'Approved' : action === 'always' ? 'Always approved' : 'Denied';
    label.textContent = labelText;
    actions.appendChild(label);
  }
}

function renderMarkdown(text) {
  if (typeof marked !== 'undefined') {
    let html = marked.parse(text);
    // Sanitize HTML output to prevent XSS from tool output or LLM responses.
    html = sanitizeRenderedHtml(html);
    // Inject copy buttons into <pre> blocks
    html = html.replace(/<pre>/g, '<pre class="code-block-wrapper"><button class="copy-btn" onclick="copyCodeBlock(this)">Copy</button>');
    return html;
  }
  return escapeHtml(text);
}

// Strip dangerous HTML elements and attributes from rendered markdown.
// This prevents XSS from tool output or prompt injection in LLM responses.
function sanitizeRenderedHtml(html) {
  html = html.replace(/<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>/gi, '');
  html = html.replace(/<iframe\b[^>]*>[\s\S]*?<\/iframe>/gi, '');
  html = html.replace(/<object\b[^>]*>[\s\S]*?<\/object>/gi, '');
  html = html.replace(/<embed\b[^>]*\/?>/gi, '');
  html = html.replace(/<form\b[^>]*>[\s\S]*?<\/form>/gi, '');
  html = html.replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, '');
  html = html.replace(/<link\b[^>]*\/?>/gi, '');
  html = html.replace(/<base\b[^>]*\/?>/gi, '');
  html = html.replace(/<meta\b[^>]*\/?>/gi, '');
  // Remove event handler attributes (onclick, onerror, onload, etc.)
  html = html.replace(/\s+on\w+\s*=\s*"[^"]*"/gi, '');
  html = html.replace(/\s+on\w+\s*=\s*'[^']*'/gi, '');
  html = html.replace(/\s+on\w+\s*=\s*[^\s>]+/gi, '');
  // Remove javascript: and data: URLs in href/src attributes
  html = html.replace(/(href|src|action)\s*=\s*["']?\s*javascript\s*:/gi, '$1="');
  html = html.replace(/(href|src|action)\s*=\s*["']?\s*data\s*:/gi, '$1="');
  return html;
}

function copyCodeBlock(btn) {
  const pre = btn.parentElement;
  const code = pre.querySelector('code');
  const text = code ? code.textContent : pre.textContent;
  navigator.clipboard.writeText(text).then(() => {
    btn.textContent = 'Copied!';
    setTimeout(() => { btn.textContent = 'Copy'; }, 1500);
  });
}

function addMessage(role, content) {
  const container = document.getElementById('chat-messages');
  const div = document.createElement('div');
  div.className = 'message ' + role;
  if (role === 'user') {
    div.textContent = content;
  } else {
    div.setAttribute('data-raw', content);
    div.innerHTML = renderMarkdown(content);
  }
  container.appendChild(div);
  container.scrollTop = container.scrollHeight;
}

function appendToLastAssistant(chunk) {
  const container = document.getElementById('chat-messages');
  const messages = container.querySelectorAll('.message.assistant');
  if (messages.length > 0) {
    const last = messages[messages.length - 1];
    const raw = (last.getAttribute('data-raw') || '') + chunk;
    last.setAttribute('data-raw', raw);
    last.innerHTML = renderMarkdown(raw);
    container.scrollTop = container.scrollHeight;
  } else {
    addMessage('assistant', chunk);
  }
}

function setStatus(text, spinning) {
  const el = document.getElementById('chat-status');
  if (!text) {
    el.innerHTML = '';
    return;
  }
  el.innerHTML = (spinning ? '<div class="spinner"></div>' : '') + escapeHtml(text);
}

function showApproval(data) {
  const container = document.getElementById('chat-messages');
  const card = document.createElement('div');
  card.className = 'approval-card';
  card.setAttribute('data-request-id', data.request_id);

  const header = document.createElement('div');
  header.className = 'approval-header';
  header.textContent = 'Tool requires approval';
  card.appendChild(header);

  const toolName = document.createElement('div');
  toolName.className = 'approval-tool-name';
  toolName.textContent = data.tool_name;
  card.appendChild(toolName);

  if (data.description) {
    const desc = document.createElement('div');
    desc.className = 'approval-description';
    desc.textContent = data.description;
    card.appendChild(desc);
  }

  if (data.parameters) {
    const paramsToggle = document.createElement('button');
    paramsToggle.className = 'approval-params-toggle';
    paramsToggle.textContent = 'Show parameters';
    const paramsBlock = document.createElement('pre');
    paramsBlock.className = 'approval-params';
    paramsBlock.textContent = data.parameters;
    paramsBlock.style.display = 'none';
    paramsToggle.addEventListener('click', () => {
      const visible = paramsBlock.style.display !== 'none';
      paramsBlock.style.display = visible ? 'none' : 'block';
      paramsToggle.textContent = visible ? 'Show parameters' : 'Hide parameters';
    });
    card.appendChild(paramsToggle);
    card.appendChild(paramsBlock);
  }

  const actions = document.createElement('div');
  actions.className = 'approval-actions';

  const approveBtn = document.createElement('button');
  approveBtn.className = 'approve';
  approveBtn.textContent = 'Approve';
  approveBtn.addEventListener('click', () => sendApprovalAction(data.request_id, 'approve'));

  const alwaysBtn = document.createElement('button');
  alwaysBtn.className = 'always';
  alwaysBtn.textContent = 'Always';
  alwaysBtn.addEventListener('click', () => sendApprovalAction(data.request_id, 'always'));

  const denyBtn = document.createElement('button');
  denyBtn.className = 'deny';
  denyBtn.textContent = 'Deny';
  denyBtn.addEventListener('click', () => sendApprovalAction(data.request_id, 'deny'));

  actions.appendChild(approveBtn);
  actions.appendChild(alwaysBtn);
  actions.appendChild(denyBtn);
  card.appendChild(actions);

  container.appendChild(card);
  container.scrollTop = container.scrollHeight;
}

function showJobCard(data) {
  const container = document.getElementById('chat-messages');
  const card = document.createElement('div');
  card.className = 'job-card';

  const icon = document.createElement('span');
  icon.className = 'job-card-icon';
  icon.textContent = '\u2692';
  card.appendChild(icon);

  const info = document.createElement('div');
  info.className = 'job-card-info';

  const title = document.createElement('div');
  title.className = 'job-card-title';
  title.textContent = data.title || 'Sandbox Job';
  info.appendChild(title);

  const id = document.createElement('div');
  id.className = 'job-card-id';
  id.textContent = (data.job_id || '').substring(0, 8);
  info.appendChild(id);

  card.appendChild(info);

  const viewBtn = document.createElement('button');
  viewBtn.className = 'job-card-view';
  viewBtn.textContent = 'View Job';
  viewBtn.addEventListener('click', () => {
    switchTab('jobs');
    openJobDetail(data.job_id);
  });
  card.appendChild(viewBtn);

  if (data.browse_url) {
    const browseBtn = document.createElement('a');
    browseBtn.className = 'job-card-browse';
    browseBtn.href = data.browse_url;
    browseBtn.target = '_blank';
    browseBtn.textContent = 'Browse';
    card.appendChild(browseBtn);
  }

  container.appendChild(card);
  container.scrollTop = container.scrollHeight;
}

// --- Auth card ---

function showAuthCard(data) {
  // Remove any existing card for this extension first
  removeAuthCard(data.extension_name);

  const container = document.getElementById('chat-messages');
  const card = document.createElement('div');
  card.className = 'auth-card';
  card.setAttribute('data-extension-name', data.extension_name);

  const header = document.createElement('div');
  header.className = 'auth-header';
  header.textContent = 'Authentication required for ' + data.extension_name;
  card.appendChild(header);

  if (data.instructions) {
    const instr = document.createElement('div');
    instr.className = 'auth-instructions';
    instr.textContent = data.instructions;
    card.appendChild(instr);
  }

  const links = document.createElement('div');
  links.className = 'auth-links';

  if (data.auth_url) {
    const oauthBtn = document.createElement('button');
    oauthBtn.className = 'auth-oauth';
    oauthBtn.textContent = 'Authenticate with ' + data.extension_name;
    oauthBtn.addEventListener('click', () => {
      window.open(data.auth_url, '_blank', 'width=600,height=700');
    });
    links.appendChild(oauthBtn);
  }

  if (data.setup_url) {
    const setupLink = document.createElement('a');
    setupLink.href = data.setup_url;
    setupLink.target = '_blank';
    setupLink.textContent = 'Get your token';
    links.appendChild(setupLink);
  }

  if (links.children.length > 0) {
    card.appendChild(links);
  }

  // Token input
  const tokenRow = document.createElement('div');
  tokenRow.className = 'auth-token-input';

  const tokenInput = document.createElement('input');
  tokenInput.type = 'password';
  tokenInput.placeholder = 'Paste your API key or token';
  tokenInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') submitAuthToken(data.extension_name, tokenInput.value);
  });
  tokenRow.appendChild(tokenInput);
  card.appendChild(tokenRow);

  // Error display (hidden initially)
  const errorEl = document.createElement('div');
  errorEl.className = 'auth-error';
  errorEl.style.display = 'none';
  card.appendChild(errorEl);

  // Action buttons
  const actions = document.createElement('div');
  actions.className = 'auth-actions';

  const submitBtn = document.createElement('button');
  submitBtn.className = 'auth-submit';
  submitBtn.textContent = 'Submit';
  submitBtn.addEventListener('click', () => submitAuthToken(data.extension_name, tokenInput.value));

  const cancelBtn = document.createElement('button');
  cancelBtn.className = 'auth-cancel';
  cancelBtn.textContent = 'Cancel';
  cancelBtn.addEventListener('click', () => cancelAuth(data.extension_name));

  actions.appendChild(submitBtn);
  actions.appendChild(cancelBtn);
  card.appendChild(actions);

  container.appendChild(card);
  container.scrollTop = container.scrollHeight;
  tokenInput.focus();
}

function removeAuthCard(extensionName) {
  const card = document.querySelector('.auth-card[data-extension-name="' + extensionName + '"]');
  if (card) card.remove();
}

function submitAuthToken(extensionName, tokenValue) {
  if (!tokenValue || !tokenValue.trim()) return;

  // Disable submit button while in flight
  const card = document.querySelector('.auth-card[data-extension-name="' + extensionName + '"]');
  if (card) {
    const btns = card.querySelectorAll('button');
    btns.forEach((b) => { b.disabled = true; });
  }

  apiFetch('/api/chat/auth-token', {
    method: 'POST',
    body: { extension_name: extensionName, token: tokenValue.trim() },
  }).then((result) => {
    if (result.success) {
      removeAuthCard(extensionName);
      addMessage('system', result.message);
    } else {
      showAuthCardError(extensionName, result.message);
    }
  }).catch((err) => {
    showAuthCardError(extensionName, 'Failed: ' + err.message);
  });
}

function cancelAuth(extensionName) {
  apiFetch('/api/chat/auth-cancel', {
    method: 'POST',
    body: { extension_name: extensionName },
  }).catch(() => {});
  removeAuthCard(extensionName);
  enableChatInput();
}

function showAuthCardError(extensionName, message) {
  const card = document.querySelector('.auth-card[data-extension-name="' + extensionName + '"]');
  if (!card) return;
  // Re-enable buttons
  const btns = card.querySelectorAll('button');
  btns.forEach((b) => { b.disabled = false; });
  // Show error
  const errorEl = card.querySelector('.auth-error');
  if (errorEl) {
    errorEl.textContent = message;
    errorEl.style.display = 'block';
  }
}

function loadHistory(before) {
  let historyUrl = '/api/chat/history?limit=50';
  if (currentThreadId) {
    historyUrl += '&thread_id=' + encodeURIComponent(currentThreadId);
  }
  if (before) {
    historyUrl += '&before=' + encodeURIComponent(before);
  }

  const isPaginating = !!before;
  if (isPaginating) loadingOlder = true;

  apiFetch(historyUrl).then((data) => {
    const container = document.getElementById('chat-messages');

    if (!isPaginating) {
      // Fresh load: clear and render
      container.innerHTML = '';
      for (const turn of data.turns) {
        addMessage('user', turn.user_input);
        if (turn.response) {
          addMessage('assistant', turn.response);
        }
      }
    } else {
      // Pagination: prepend older messages
      const savedHeight = container.scrollHeight;
      const fragment = document.createDocumentFragment();
      for (const turn of data.turns) {
        const userDiv = createMessageElement('user', turn.user_input);
        fragment.appendChild(userDiv);
        if (turn.response) {
          const assistantDiv = createMessageElement('assistant', turn.response);
          fragment.appendChild(assistantDiv);
        }
      }
      container.insertBefore(fragment, container.firstChild);
      // Restore scroll position so the user doesn't jump
      container.scrollTop = container.scrollHeight - savedHeight;
    }

    hasMore = data.has_more || false;
    oldestTimestamp = data.oldest_timestamp || null;
  }).catch(() => {
    // No history or no active thread
  }).finally(() => {
    loadingOlder = false;
    removeScrollSpinner();
  });
}

// Create a message DOM element without appending it (for prepend operations)
function createMessageElement(role, content) {
  const div = document.createElement('div');
  div.className = 'message ' + role;
  if (role === 'user') {
    div.textContent = content;
  } else {
    div.setAttribute('data-raw', content);
    div.innerHTML = renderMarkdown(content);
  }
  return div;
}

function removeScrollSpinner() {
  const spinner = document.getElementById('scroll-load-spinner');
  if (spinner) spinner.remove();
}

// --- Threads ---

function loadThreads() {
  apiFetch('/api/chat/threads').then((data) => {
    // Pinned assistant thread
    if (data.assistant_thread) {
      assistantThreadId = data.assistant_thread.id;
      const el = document.getElementById('assistant-thread');
      const isActive = currentThreadId === assistantThreadId;
      el.className = 'assistant-item' + (isActive ? ' active' : '');
      const meta = document.getElementById('assistant-meta');
      const count = data.assistant_thread.turn_count || 0;
      meta.textContent = count > 0 ? count + ' turns' : '';
    }

    // Regular threads
    const list = document.getElementById('thread-list');
    list.innerHTML = '';
    const threads = data.threads || [];
    for (const thread of threads) {
      const item = document.createElement('div');
      item.className = 'thread-item' + (thread.id === currentThreadId ? ' active' : '');
      const label = document.createElement('span');
      label.className = 'thread-label';
      label.textContent = thread.title || thread.id.substring(0, 8);
      label.title = thread.title ? thread.title + ' (' + thread.id + ')' : thread.id;
      item.appendChild(label);
      const meta = document.createElement('span');
      meta.className = 'thread-meta';
      meta.textContent = (thread.turn_count || 0) + ' turns';
      item.appendChild(meta);
      item.addEventListener('click', () => switchThread(thread.id));
      list.appendChild(item);
    }

    // Default to assistant thread on first load if no thread selected
    if (!currentThreadId && assistantThreadId) {
      switchToAssistant();
    }
  }).catch(() => {});
}

function switchToAssistant() {
  if (!assistantThreadId) return;
  currentThreadId = assistantThreadId;
  hasMore = false;
  oldestTimestamp = null;
  loadHistory();
  loadThreads();
}

function switchThread(threadId) {
  currentThreadId = threadId;
  hasMore = false;
  oldestTimestamp = null;
  loadHistory();
  loadThreads();
}

function createNewThread() {
  apiFetch('/api/chat/thread/new', { method: 'POST' }).then((data) => {
    currentThreadId = data.id || null;
    document.getElementById('chat-messages').innerHTML = '';
    setStatus('');
    loadThreads();
  }).catch((err) => {
    showToast('Failed to create thread: ' + err.message, 'error');
  });
}

function toggleThreadSidebar() {
  const sidebar = document.getElementById('thread-sidebar');
  sidebar.classList.toggle('collapsed');
  const btn = document.getElementById('thread-toggle-btn');
  btn.innerHTML = sidebar.classList.contains('collapsed') ? '&raquo;' : '&laquo;';
}

// Chat input auto-resize and keyboard handling
const chatInput = document.getElementById('chat-input');
chatInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault();
    sendMessage();
  }
});
chatInput.addEventListener('input', () => autoResizeTextarea(chatInput));

const skillsSearchQueryInput = document.getElementById('skills-search-query');
if (skillsSearchQueryInput) {
  skillsSearchQueryInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      searchSkills();
    }
  });
}

// Infinite scroll: load older messages when scrolled near the top
document.getElementById('chat-messages').addEventListener('scroll', function () {
  if (this.scrollTop < 100 && hasMore && !loadingOlder) {
    loadingOlder = true;
    // Show spinner at top
    const spinner = document.createElement('div');
    spinner.id = 'scroll-load-spinner';
    spinner.className = 'scroll-load-spinner';
    spinner.innerHTML = '<div class="spinner"></div> Loading older messages...';
    this.insertBefore(spinner, this.firstChild);
    loadHistory(oldestTimestamp);
  }
});

function autoResizeTextarea(el) {
  el.style.height = 'auto';
  el.style.height = Math.min(el.scrollHeight, 120) + 'px';
}

// --- Tabs ---

document.querySelectorAll('.tab-bar button[data-tab]').forEach((btn) => {
  btn.addEventListener('click', () => {
    const tab = btn.getAttribute('data-tab');
    switchTab(tab);
  });
});

document.querySelectorAll('.surface-toggle[data-surface-toggle]').forEach((btn) => {
  btn.addEventListener('click', () => {
    const surface = btn.getAttribute('data-surface-toggle');
    toggleQuickSurface(surface);
  });
});

renderQuickSurfaceLayout();

function switchTab(tab) {
  currentTab = tab;
  document.querySelectorAll('.tab-bar button[data-tab]').forEach((b) => {
    b.classList.toggle('active', b.getAttribute('data-tab') === tab);
  });
  document.querySelectorAll('.tab-panel').forEach((p) => {
    p.classList.toggle('active', p.id === 'tab-' + tab);
  });

  if (tab === 'skills') loadSkills();
  if (tab === 'ops') loadOpsDashboard();
  if (tab === 'memory') loadMemoryTree();
  if (tab === 'jobs') loadJobs();
  if (tab === 'routines') loadRoutines();
  if (tab === 'logs') applyLogFilters();
  if (tab === 'extensions') loadExtensions();
}

// --- Hyperliquid Ops dashboard ---

function startOpsRefresh() {
  stopOpsRefresh();
  opsRefreshTimer = setInterval(loadOpsDashboard, OPS_REFRESH_INTERVAL_MS);
}

function stopOpsRefresh() {
  if (opsRefreshTimer) {
    clearInterval(opsRefreshTimer);
    opsRefreshTimer = null;
  }
}

function startQuickSurfaceRefresh() {
  stopQuickSurfaceRefresh();
  quickSurfaceRefreshTimer = setInterval(refreshQuickSurfaces, QUICK_SURFACE_REFRESH_INTERVAL_MS);
}

function stopQuickSurfaceRefresh() {
  if (quickSurfaceRefreshTimer) {
    clearInterval(quickSurfaceRefreshTimer);
    quickSurfaceRefreshTimer = null;
  }
}

function toggleQuickSurface(surface) {
  if (!Object.prototype.hasOwnProperty.call(quickSurfaceState, surface)) return;
  quickSurfaceState[surface] = !quickSurfaceState[surface];
  renderQuickSurfaceLayout();
  if (quickSurfaceState[surface]) refreshQuickSurface(surface);
}

function renderQuickSurfaceLayout() {
  const panel = document.getElementById('quick-surfaces');
  if (!panel) return;
  const anyOpen = Object.values(quickSurfaceState).some(Boolean);
  panel.classList.toggle('visible', anyOpen);

  for (const surface of Object.keys(quickSurfaceState)) {
    const button = document.querySelector('.surface-toggle[data-surface-toggle="' + surface + '"]');
    const section = document.getElementById('quick-surface-' + surface);
    const open = !!quickSurfaceState[surface];
    if (button) {
      button.classList.toggle('active', open);
      button.setAttribute('aria-pressed', open ? 'true' : 'false');
    }
    if (section) {
      section.classList.toggle('visible', open);
    }
  }
}

function refreshQuickSurfaces() {
  for (const surface of Object.keys(quickSurfaceState)) {
    if (quickSurfaceState[surface]) refreshQuickSurface(surface);
  }
}

function refreshQuickSurface(surface) {
  if (surface === 'logs') renderQuickLogsSurface();
  if (surface === 'history') refreshQuickHistorySurface();
  if (surface === 'usage') refreshQuickUsageSurface();
}

function renderQuickLogsSurface() {
  const output = document.getElementById('quick-logs-output');
  if (!output) return;
  if (!recentLogs || recentLogs.length === 0) {
    output.innerHTML = '<div class="empty-state">No logs yet</div>';
    return;
  }

  const rows = recentLogs.slice(-50).reverse().map((entry) => {
    const ts = entry.timestamp ? String(entry.timestamp).substring(11, 23) : '--:--:--.---';
    const level = entry.level || 'INFO';
    const target = entry.target || '-';
    const message = entry.message || '';
    return '<div class="quick-log-row level-' + escapeHtml(level) + '">'
      + '<span class="quick-log-ts">' + escapeHtml(ts) + '</span>'
      + '<span class="quick-log-level">' + escapeHtml(level) + '</span>'
      + '<span class="quick-log-target">' + escapeHtml(target) + '</span>'
      + '<span class="quick-log-msg">' + escapeHtml(message) + '</span>'
      + '</div>';
  });
  output.innerHTML = rows.join('');
}

function refreshQuickHistorySurface() {
  const output = document.getElementById('quick-history-output');
  if (!output) return;

  let historyPath = '/api/chat/history?limit=8';
  if (currentThreadId) {
    historyPath += '&thread_id=' + encodeURIComponent(currentThreadId);
  }

  Promise.all([
    apiFetch('/api/chat/threads').catch(() => ({ threads: [], assistant_thread: null })),
    apiFetch('/api/jobs').catch(() => ({ jobs: [] })),
    apiFetch(historyPath).catch(() => ({ turns: [] })),
  ]).then(([threadsData, jobsData, historyData]) => {
    const assistantTurns = (threadsData.assistant_thread && threadsData.assistant_thread.turn_count) || 0;
    const conversations = (threadsData.threads || []).length;
    const jobs = (jobsData.jobs || []).slice(0, 4);
    const turns = (historyData.turns || []).slice(-4).reverse();
    const liveEvents = Array.from(jobEvents.values()).reduce((sum, events) => sum + events.length, 0);

    const turnRows = turns.length > 0
      ? turns.map((turn) => {
        const input = turn.user_input ? shortenSurfaceText(turn.user_input, 100) : '';
        const response = turn.response ? shortenSurfaceText(turn.response, 100) : '';
        return '<div class="quick-history-row">'
          + '<div class="quick-history-role">User</div>'
          + '<div class="quick-history-content">' + escapeHtml(input) + '</div>'
          + '<div class="quick-history-role">Assistant</div>'
          + '<div class="quick-history-content">' + escapeHtml(response) + '</div>'
          + '</div>';
      }).join('')
      : '<div class="empty-state">No thread turns yet</div>';

    const jobRows = jobs.length > 0
      ? jobs.map((job) => '<div class="quick-job-row">'
        + '<span>' + escapeHtml((job.title || 'Job').slice(0, 36)) + '</span>'
        + '<span class="badge ' + escapeHtml(String(job.state || 'pending').replace(' ', '_')) + '">'
        + escapeHtml(job.state || 'pending') + '</span>'
        + '</div>').join('')
      : '<div class="empty-state">No recent jobs</div>';

    output.innerHTML = ''
      + '<div class="quick-metric-grid">'
      + '<div class="quick-metric"><span>Assistant Turns</span><strong>' + escapeHtml(String(assistantTurns)) + '</strong></div>'
      + '<div class="quick-metric"><span>Conversations</span><strong>' + escapeHtml(String(conversations)) + '</strong></div>'
      + '<div class="quick-metric"><span>Live Events</span><strong>' + escapeHtml(String(liveEvents)) + '</strong></div>'
      + '</div>'
      + '<div class="quick-block-title">Recent Thread Turns</div>'
      + '<div class="quick-history-list">' + turnRows + '</div>'
      + '<div class="quick-block-title">Recent Jobs</div>'
      + '<div class="quick-job-list">' + jobRows + '</div>';
  }).catch((err) => {
    output.innerHTML = '<div class="empty-state">Failed to load action history: ' + escapeHtml(err.message) + '</div>';
  });
}

function refreshQuickUsageSurface() {
  const output = document.getElementById('quick-usage-output');
  if (!output) return;

  Promise.all([
    apiFetch('/api/jobs/summary').catch(() => null),
    apiFetch('/api/chat/threads').catch(() => null),
    apiFetch('/api/routines/summary').catch(() => null),
    apiFetch('/api/gateway/status').catch(() => null),
  ]).then(([jobs, threads, routines, gateway]) => {
    const threadCount = threads ? ((threads.threads || []).length + ((threads.assistant_thread && threads.assistant_thread.id) ? 1 : 0)) : 0;
    const turnCount = threads
      ? ((threads.assistant_thread && threads.assistant_thread.turn_count) || 0)
      + (threads.threads || []).reduce((sum, t) => sum + (t.turn_count || 0), 0)
      : 0;
    const inProgressJobs = jobs ? jobs.in_progress : 0;
    const failedJobs = jobs ? jobs.failed : 0;
    const routinesEnabled = routines ? routines.enabled : 0;
    const runsToday = routines ? routines.runs_today : 0;
    const sseClients = gateway ? gateway.sse_connections : 0;
    const wsClients = gateway ? gateway.ws_connections : 0;
    const runtimeStatus = gateway ? String(gateway.channel_status || 'unknown') : 'unknown';

    output.innerHTML = ''
      + '<div class="quick-metric-grid">'
      + '<div class="quick-metric"><span>Threads</span><strong>' + escapeHtml(String(threadCount)) + '</strong></div>'
      + '<div class="quick-metric"><span>Total Turns</span><strong>' + escapeHtml(String(turnCount)) + '</strong></div>'
      + '<div class="quick-metric"><span>Jobs In Progress</span><strong>' + escapeHtml(String(inProgressJobs)) + '</strong></div>'
      + '<div class="quick-metric"><span>Jobs Failed</span><strong>' + escapeHtml(String(failedJobs)) + '</strong></div>'
      + '<div class="quick-metric"><span>Automations Enabled</span><strong>' + escapeHtml(String(routinesEnabled)) + '</strong></div>'
      + '<div class="quick-metric"><span>Runs Today</span><strong>' + escapeHtml(String(runsToday)) + '</strong></div>'
      + '<div class="quick-metric"><span>SSE/WS Clients</span><strong>' + escapeHtml(String(sseClients)) + ' / ' + escapeHtml(String(wsClients)) + '</strong></div>'
      + '<div class="quick-metric"><span>Runtime Status</span><strong>' + escapeHtml(runtimeStatus) + '</strong></div>'
      + '</div>';
  }).catch((err) => {
    output.innerHTML = '<div class="empty-state">Failed to load usage: ' + escapeHtml(err.message) + '</div>';
  });
}

function shortenSurfaceText(text, maxLen) {
  const raw = String(text || '');
  if (raw.length <= maxLen) return raw;
  return raw.slice(0, maxLen) + '...';
}

function readSetting(settings, key, fallback) {
  if (!settings || !Object.prototype.hasOwnProperty.call(settings, key)) return fallback;
  const value = settings[key];
  return value === null || value === undefined ? fallback : value;
}

function toBoolean(value, fallback) {
  if (value === null || value === undefined) return fallback;
  if (typeof value === 'boolean') return value;
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase();
    if (normalized === 'true') return true;
    if (normalized === 'false') return false;
  }
  return fallback;
}

function boolLabel(value) {
  return value ? 'Enabled' : 'Disabled';
}

function maskAddress(address) {
  if (!address || typeof address !== 'string') return 'Not configured';
  if (address.length < 12) return address;
  return address.slice(0, 6) + '...' + address.slice(-4);
}

function opsPill(text, tone) {
  return '<span class="ops-pill ops-pill-' + tone + '">' + escapeHtml(String(text)) + '</span>';
}

function loadOpsDashboard() {
  const root = document.getElementById('ops-dashboard');
  if (!root) return;
  root.classList.add('loading');

  Promise.all([
    apiFetch('/api/settings/export').catch(() => ({ settings: {} })),
    apiFetch('/api/jobs/summary').catch(() => null),
    apiFetch('/api/routines/summary').catch(() => null),
    apiFetch('/api/chat/threads').catch(() => null),
    apiFetch('/api/memory/tree').catch(() => ({ entries: [] })),
    apiFetch('/api/gateway/status').catch(() => null),
  ]).then(([settingsData, jobs, routines, threads, memoryTree, gateway]) => {
    const settings = (settingsData && settingsData.settings) || {};

    const hyperliquidNetwork = String(readSetting(settings, 'hyperliquid_runtime.network', 'testnet'));
    const paperLivePolicy = String(readSetting(settings, 'hyperliquid_runtime.paper_live_policy', 'paper_first'));
    const hyperliquidApi = String(readSetting(settings, 'hyperliquid_runtime.api_base_url', 'not configured'));
    const hyperliquidWs = String(readSetting(settings, 'hyperliquid_runtime.ws_url', 'not configured'));
    const timeoutMs = readSetting(settings, 'hyperliquid_runtime.timeout_ms', 10000);
    const retryMax = readSetting(settings, 'hyperliquid_runtime.max_retries', 3);
    const retryBackoff = readSetting(settings, 'hyperliquid_runtime.retry_backoff_ms', 500);

    const custodyMode = String(readSetting(settings, 'wallet_vault_policy.custody_mode', 'operator_wallet'));
    const operatorWallet = readSetting(settings, 'wallet_vault_policy.operator_wallet_address', null);
    const userWallet = readSetting(settings, 'wallet_vault_policy.user_wallet_address', null);
    const vaultAddress = readSetting(settings, 'wallet_vault_policy.vault_address', null);
    const maxPosition = readSetting(settings, 'wallet_vault_policy.max_position_size_usd', 1000);
    const leverageCap = readSetting(settings, 'wallet_vault_policy.leverage_cap', 2);
    const killSwitchEnabled = toBoolean(readSetting(settings, 'wallet_vault_policy.kill_switch_enabled', true), true);
    const killSwitchBehavior = String(readSetting(settings, 'wallet_vault_policy.kill_switch_behavior', 'pause_agent'));

    const verificationBackend = String(readSetting(settings, 'verification_backend.backend', 'eigencloud_primary'));
    const eigenEndpoint = readSetting(settings, 'verification_backend.eigencloud_endpoint', null);
    const eigenAuthScheme = String(readSetting(settings, 'verification_backend.eigencloud_auth_scheme', 'bearer'));
    const eigenTokenConfigured = !!readSetting(settings, 'verification_backend.eigencloud_auth_token', null);
    const eigenTimeoutMs = readSetting(settings, 'verification_backend.eigencloud_timeout_ms', 5000);
    const fallbackEnabled = toBoolean(readSetting(settings, 'verification_backend.fallback_enabled', true), true);
    const requireSignedReceipts = toBoolean(readSetting(settings, 'verification_backend.fallback_require_signed_receipts', true), true);

    const embeddingsEnabled = toBoolean(readSetting(settings, 'embeddings.enabled', false), false);
    const embeddingsProvider = String(readSetting(settings, 'embeddings.provider', 'nearai'));
    const embeddingsModel = String(readSetting(settings, 'embeddings.model', 'text-embedding-3-small'));
    const llmBackend = String(readSetting(settings, 'llm_backend', 'nearai'));
    const selectedModel = String(readSetting(settings, 'selected_model', 'not selected'));

    const memoryEntries = (memoryTree && memoryTree.entries) || [];
    const memoryFiles = memoryEntries.filter((e) => e && !e.is_dir).length;
    const memoryDirs = memoryEntries.filter((e) => e && e.is_dir).length;

    const hasOperatorWallet = !!operatorWallet;
    const hasUserWallet = !!userWallet;
    const hasVaultAddress = !!vaultAddress;
    const attestationReady = hasUserWallet && verificationBackend === 'eigencloud_primary' && requireSignedReceipts;
    const verifyOpsReady = fallbackEnabled || !!eigenEndpoint;

    const totalJobs = jobs ? jobs.total : 0;
    const activeJobs = jobs ? jobs.in_progress : 0;
    const failedJobs = jobs ? jobs.failed : 0;
    const stuckJobs = jobs ? jobs.stuck : 0;
    const enabledRoutines = routines ? routines.enabled : 0;
    const runsToday = routines ? routines.runs_today : 0;
    const totalThreads = threads ? ((threads.threads || []).length + ((threads.assistant_thread && threads.assistant_thread.id) ? 1 : 0)) : 0;
    const gatewaySse = gateway ? (gateway.sse_connections || 0) : 0;
    const gatewayWs = gateway ? (gateway.ws_connections || 0) : 0;

    root.innerHTML =
      '<section class="ops-hero">'
      + '<div>'
      + '<h2>Hyperliquid Control Surface</h2>'
      + '<p>Runtime, custody policy, verification, attestation readiness, strategy workflows, and autonomous memory in one operator view.</p>'
      + '</div>'
      + '<div class="ops-hero-pills">'
      + opsPill('Network: ' + hyperliquidNetwork, hyperliquidNetwork === 'mainnet' ? 'warn' : 'info')
      + opsPill('Policy: ' + paperLivePolicy, paperLivePolicy === 'live_allowed' ? 'warn' : 'success')
      + opsPill('LLM: ' + llmBackend, 'neutral')
      + opsPill('Model: ' + selectedModel, 'neutral')
      + '</div>'
      + '</section>'

      + '<section class="ops-grid">'
      + '<article class="ops-card">'
      + '<h3>Hyperliquid Runtime</h3>'
      + '<div class="ops-kv"><span>API</span><span>' + escapeHtml(hyperliquidApi) + '</span></div>'
      + '<div class="ops-kv"><span>WebSocket</span><span>' + escapeHtml(hyperliquidWs) + '</span></div>'
      + '<div class="ops-kv"><span>Timeout</span><span>' + escapeHtml(String(timeoutMs)) + ' ms</span></div>'
      + '<div class="ops-kv"><span>Retries</span><span>' + escapeHtml(String(retryMax)) + ' @ ' + escapeHtml(String(retryBackoff)) + ' ms</span></div>'
      + '</article>'

      + '<article class="ops-card">'
      + '<h3>Wallet and Vault Policy</h3>'
      + '<div class="ops-kv"><span>Custody</span><span>' + escapeHtml(custodyMode) + '</span></div>'
      + '<div class="ops-kv"><span>Operator Wallet</span><span>' + escapeHtml(maskAddress(operatorWallet)) + '</span></div>'
      + '<div class="ops-kv"><span>User Wallet</span><span>' + escapeHtml(maskAddress(userWallet)) + '</span></div>'
      + '<div class="ops-kv"><span>Vault</span><span>' + escapeHtml(maskAddress(vaultAddress)) + '</span></div>'
      + '<div class="ops-kv"><span>Max Position</span><span>$' + escapeHtml(String(maxPosition)) + '</span></div>'
      + '<div class="ops-kv"><span>Leverage Cap</span><span>' + escapeHtml(String(leverageCap)) + 'x</span></div>'
      + '<div class="ops-kv"><span>Kill Switch</span><span>' + escapeHtml(boolLabel(killSwitchEnabled)) + ' (' + escapeHtml(killSwitchBehavior) + ')</span></div>'
      + '</article>'

      + '<article class="ops-card">'
      + '<h3>Verification and EigenCloud</h3>'
      + '<div class="ops-kv"><span>Backend</span><span>' + escapeHtml(verificationBackend) + '</span></div>'
      + '<div class="ops-kv"><span>Endpoint</span><span>' + escapeHtml(eigenEndpoint || 'Not configured') + '</span></div>'
      + '<div class="ops-kv"><span>Auth Scheme</span><span>' + escapeHtml(eigenAuthScheme) + '</span></div>'
      + '<div class="ops-kv"><span>Auth Token</span><span>' + (eigenTokenConfigured ? 'Configured' : 'Not configured') + '</span></div>'
      + '<div class="ops-kv"><span>Timeout</span><span>' + escapeHtml(String(eigenTimeoutMs)) + ' ms</span></div>'
      + '<div class="ops-kv"><span>Fallback</span><span>' + escapeHtml(boolLabel(fallbackEnabled)) + '</span></div>'
      + '<div class="ops-kv"><span>Signed Receipts</span><span>' + escapeHtml(boolLabel(requireSignedReceipts)) + '</span></div>'
      + '</article>'

      + '<article class="ops-card">'
      + '<h3>Autonomous Memory (Supermemory)</h3>'
      + '<div class="ops-kv"><span>Embeddings</span><span>' + escapeHtml(boolLabel(embeddingsEnabled)) + '</span></div>'
      + '<div class="ops-kv"><span>Provider</span><span>' + escapeHtml(embeddingsProvider) + '</span></div>'
      + '<div class="ops-kv"><span>Model</span><span>' + escapeHtml(embeddingsModel) + '</span></div>'
      + '<div class="ops-kv"><span>Workspace Files</span><span>' + escapeHtml(String(memoryFiles)) + '</span></div>'
      + '<div class="ops-kv"><span>Workspace Dirs</span><span>' + escapeHtml(String(memoryDirs)) + '</span></div>'
      + '<div class="ops-kv"><span>Enabled Routines</span><span>' + escapeHtml(String(enabledRoutines)) + '</span></div>'
      + '<div class="ops-kv"><span>Runs Today</span><span>' + escapeHtml(String(runsToday)) + '</span></div>'
      + '</article>'
      + '</section>'

      + '<section class="ops-grid ops-grid-secondary">'
      + '<article class="ops-card">'
      + '<h3>Agent and Gateway Activity</h3>'
      + '<div class="ops-stat-grid">'
      + '<div class="ops-stat"><span class="ops-stat-label">Threads</span><span class="ops-stat-value">' + escapeHtml(String(totalThreads)) + '</span></div>'
      + '<div class="ops-stat"><span class="ops-stat-label">Jobs</span><span class="ops-stat-value">' + escapeHtml(String(totalJobs)) + '</span></div>'
      + '<div class="ops-stat"><span class="ops-stat-label">Active Jobs</span><span class="ops-stat-value">' + escapeHtml(String(activeJobs)) + '</span></div>'
      + '<div class="ops-stat"><span class="ops-stat-label">Failed Jobs</span><span class="ops-stat-value">' + escapeHtml(String(failedJobs)) + '</span></div>'
      + '<div class="ops-stat"><span class="ops-stat-label">Stuck Jobs</span><span class="ops-stat-value">' + escapeHtml(String(stuckJobs)) + '</span></div>'
      + '<div class="ops-stat"><span class="ops-stat-label">SSE Clients</span><span class="ops-stat-value">' + escapeHtml(String(gatewaySse)) + '</span></div>'
      + '<div class="ops-stat"><span class="ops-stat-label">WS Clients</span><span class="ops-stat-value">' + escapeHtml(String(gatewayWs)) + '</span></div>'
      + '</div>'
      + '</article>'

      + '<article class="ops-card">'
      + '<h3>Attestation and Verifiability Checklist</h3>'
      + '<div class="ops-checklist">'
      + '<div class="ops-check-row"><span>Operator wallet mapped</span>' + opsPill(hasOperatorWallet ? 'Ready' : 'Missing', hasOperatorWallet ? 'success' : 'danger') + '</div>'
      + '<div class="ops-check-row"><span>User wallet mapped</span>' + opsPill(hasUserWallet ? 'Ready' : 'Missing', hasUserWallet ? 'success' : 'warn') + '</div>'
      + '<div class="ops-check-row"><span>Vault mapped</span>' + opsPill(hasVaultAddress ? 'Ready' : 'Optional', hasVaultAddress ? 'success' : 'neutral') + '</div>'
      + '<div class="ops-check-row"><span>Verification pipeline</span>' + opsPill(verifyOpsReady ? 'Ready' : 'Blocked', verifyOpsReady ? 'success' : 'danger') + '</div>'
      + '<div class="ops-check-row"><span>Signed receipt policy</span>' + opsPill(requireSignedReceipts ? 'Strict' : 'Relaxed', requireSignedReceipts ? 'success' : 'warn') + '</div>'
      + '<div class="ops-check-row"><span>Attestation readiness</span>' + opsPill(attestationReady ? 'Ready for validation flow' : 'Needs wallet + strict verify config', attestationReady ? 'success' : 'warn') + '</div>'
      + '</div>'
      + '</article>'
      + '</section>'

      + '<section class="ops-actions-wrap">'
      + '<article class="ops-card">'
      + '<h3>Quick Hyperliquid Actions</h3>'
      + '<div class="ops-actions">'
      + '<button data-command="/positions">/positions</button>'
      + '<button data-command="/exposure">/exposure</button>'
      + '<button data-command="/risk">/risk</button>'
      + '<button data-command="/funding">/funding</button>'
      + '<button data-command="/vault">/vault</button>'
      + '<button data-command="/verify latest">/verify latest</button>'
      + '<button data-command="/receipts agent-main">/receipts agent-main</button>'
      + '</div>'
      + '</article>'

      + '<article class="ops-card">'
      + '<h3>Strategy and Backtesting Playbooks</h3>'
      + '<div class="ops-actions">'
      + '<button data-fill="Build a Hyperliquid momentum strategy plan with strict risk controls and define a 30-day backtest protocol with metrics, failure modes, and execution guardrails.">Momentum Strategy Plan</button>'
      + '<button data-fill="Design a mean-reversion perpetual strategy for Hyperliquid with entry/exit criteria, max drawdown limits, and a reproducible backtesting checklist.">Mean Reversion Plan</button>'
      + '<button data-fill="Create a funding-rate carry strategy assessment across supported pairs and include scenario analysis for regime shifts and liquidity shocks.">Funding Carry Assessment</button>'
      + '<button data-fill="Run a verification-focused post-trade audit template for Hyperliquid actions including receipts, backend evidence, and attestation checkpoints.">Verification Audit Template</button>'
      + '<button data-fill="Create a supermemory routine plan to persist strategy decisions, outcomes, and risk incidents, then propose retrieval prompts for autonomous adaptation.">Supermemory Routine Plan</button>'
      + '</div>'
      + '</article>'
      + '</section>';

    root.classList.remove('loading');
  }).catch((err) => {
    root.classList.remove('loading');
    root.innerHTML = '<div class="ops-error">Failed to load Hyperliquid context: ' + escapeHtml(err.message) + '</div>';
  });
}

function stageOpsInput(text, autoSend) {
  switchTab('chat');
  const input = document.getElementById('chat-input');
  if (!input) return;
  input.value = text;
  autoResizeTextarea(input);
  input.focus();
  if (autoSend) sendMessage();
}

const opsDashboard = document.getElementById('ops-dashboard');
if (opsDashboard) {
  opsDashboard.addEventListener('click', (event) => {
    const button = event.target.closest('button[data-command], button[data-fill]');
    if (!button) return;
    const command = button.getAttribute('data-command');
    const fill = button.getAttribute('data-fill');
    if (command) {
      stageOpsInput(command, true);
      return;
    }
    if (fill) {
      stageOpsInput(fill, false);
    }
  });
}

function openPrivateStorageTree() {
  switchTab('memory');
}

function refreshPrivateStorageLink() {
  const meta = document.getElementById('private-storage-meta');
  if (!meta) return;
  apiFetch('/api/memory/tree').then((data) => {
    const entries = (data && data.entries) || [];
    const files = entries.filter((e) => e && !e.is_dir).length;
    const dirs = entries.filter((e) => e && e.is_dir).length;
    meta.textContent = files + ' files, ' + dirs + ' directories';
  }).catch(() => {
    meta.textContent = 'Private storage unavailable';
  });
}

// --- Memory (filesystem tree) ---

let memorySearchTimeout = null;
let currentMemoryPath = null;
let currentMemoryContent = null;
// Tree state: nested nodes persisted across renders
// { name, path, is_dir, children: [] | null, expanded: bool, loaded: bool }
let memoryTreeState = null;

document.getElementById('memory-search').addEventListener('input', (e) => {
  clearTimeout(memorySearchTimeout);
  const query = e.target.value.trim();
  if (!query) {
    loadMemoryTree();
    return;
  }
  memorySearchTimeout = setTimeout(() => searchMemory(query), 300);
});

function loadMemoryTree() {
  // Only load top-level on first load (or refresh)
  apiFetch('/api/memory/list?path=').then((data) => {
    memoryTreeState = data.entries.map((e) => ({
      name: e.name,
      path: e.path,
      is_dir: e.is_dir,
      children: e.is_dir ? null : undefined,
      expanded: false,
      loaded: false,
    }));
    renderTree();
    refreshPrivateStorageLink();
  }).catch(() => {});
}

function renderTree() {
  const container = document.getElementById('memory-tree');
  container.innerHTML = '';
  if (!memoryTreeState || memoryTreeState.length === 0) {
    container.innerHTML = '<div class="tree-item" style="color:var(--text-secondary)">No files in workspace</div>';
    return;
  }
  renderNodes(memoryTreeState, container, 0);
}

function renderNodes(nodes, container, depth) {
  for (const node of nodes) {
    const row = document.createElement('div');
    row.className = 'tree-row';
    row.style.paddingLeft = (depth * 16 + 8) + 'px';

    if (node.is_dir) {
      const arrow = document.createElement('span');
      arrow.className = 'expand-arrow' + (node.expanded ? ' expanded' : '');
      arrow.textContent = '\u25B6';
      arrow.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleExpand(node);
      });
      row.appendChild(arrow);

      const label = document.createElement('span');
      label.className = 'tree-label dir';
      label.textContent = node.name;
      label.addEventListener('click', () => toggleExpand(node));
      row.appendChild(label);
    } else {
      const spacer = document.createElement('span');
      spacer.className = 'expand-arrow-spacer';
      row.appendChild(spacer);

      const label = document.createElement('span');
      label.className = 'tree-label file';
      label.textContent = node.name;
      label.addEventListener('click', () => readMemoryFile(node.path));
      row.appendChild(label);
    }

    container.appendChild(row);

    if (node.is_dir && node.expanded && node.children) {
      const childContainer = document.createElement('div');
      childContainer.className = 'tree-children';
      renderNodes(node.children, childContainer, depth + 1);
      container.appendChild(childContainer);
    }
  }
}

function toggleExpand(node) {
  if (node.expanded) {
    node.expanded = false;
    renderTree();
    return;
  }

  if (node.loaded) {
    node.expanded = true;
    renderTree();
    return;
  }

  // Lazy-load children
  apiFetch('/api/memory/list?path=' + encodeURIComponent(node.path)).then((data) => {
    node.children = data.entries.map((e) => ({
      name: e.name,
      path: e.path,
      is_dir: e.is_dir,
      children: e.is_dir ? null : undefined,
      expanded: false,
      loaded: false,
    }));
    node.loaded = true;
    node.expanded = true;
    renderTree();
  }).catch(() => {});
}

function readMemoryFile(path) {
  currentMemoryPath = path;
  // Update breadcrumb
  document.getElementById('memory-breadcrumb-path').innerHTML = buildBreadcrumb(path);
  document.getElementById('memory-edit-btn').style.display = 'inline-block';

  // Exit edit mode if active
  cancelMemoryEdit();

  apiFetch('/api/memory/read?path=' + encodeURIComponent(path)).then((data) => {
    currentMemoryContent = data.content;
    const viewer = document.getElementById('memory-viewer');
    // Render markdown if it's a .md file
    if (path.endsWith('.md')) {
      viewer.innerHTML = '<div class="memory-rendered">' + renderMarkdown(data.content) + '</div>';
      viewer.classList.add('rendered');
    } else {
      viewer.textContent = data.content;
      viewer.classList.remove('rendered');
    }
  }).catch((err) => {
    currentMemoryContent = null;
    document.getElementById('memory-viewer').innerHTML = '<div class="empty">Error: ' + escapeHtml(err.message) + '</div>';
  });
}

function startMemoryEdit() {
  if (!currentMemoryPath || currentMemoryContent === null) return;
  document.getElementById('memory-viewer').style.display = 'none';
  const editor = document.getElementById('memory-editor');
  editor.style.display = 'flex';
  const textarea = document.getElementById('memory-edit-textarea');
  textarea.value = currentMemoryContent;
  textarea.focus();
}

function cancelMemoryEdit() {
  document.getElementById('memory-viewer').style.display = '';
  document.getElementById('memory-editor').style.display = 'none';
}

function saveMemoryEdit() {
  if (!currentMemoryPath) return;
  const content = document.getElementById('memory-edit-textarea').value;
  apiFetch('/api/memory/write', {
    method: 'POST',
    body: { path: currentMemoryPath, content: content },
  }).then(() => {
    showToast('Saved ' + currentMemoryPath, 'success');
    cancelMemoryEdit();
    readMemoryFile(currentMemoryPath);
  }).catch((err) => {
    showToast('Save failed: ' + err.message, 'error');
  });
}

function buildBreadcrumb(path) {
  const parts = path.split('/');
  let html = '<a onclick="loadMemoryTree()">workspace</a>';
  let current = '';
  for (const part of parts) {
    current += (current ? '/' : '') + part;
    html += ' / <a onclick="readMemoryFile(\'' + escapeHtml(current) + '\')">' + escapeHtml(part) + '</a>';
  }
  return html;
}

function searchMemory(query) {
  const normalizedQuery = normalizeSearchQuery(query);
  if (!normalizedQuery) return;

  apiFetch('/api/memory/search', {
    method: 'POST',
    body: { query: normalizedQuery, limit: 20 },
  }).then((data) => {
    const tree = document.getElementById('memory-tree');
    tree.innerHTML = '';
    if (data.results.length === 0) {
      tree.innerHTML = '<div class="tree-item" style="color:var(--text-secondary)">No results</div>';
      return;
    }
    for (const result of data.results) {
      const item = document.createElement('div');
      item.className = 'search-result';
      const snippet = snippetAround(result.content, normalizedQuery, 120);
      item.innerHTML = '<div class="path">' + escapeHtml(result.path) + '</div>'
        + '<div class="snippet">' + highlightQuery(snippet, normalizedQuery) + '</div>';
      item.addEventListener('click', () => readMemoryFile(result.path));
      tree.appendChild(item);
    }
  }).catch(() => {});
}

function normalizeSearchQuery(query) {
  return (typeof query === 'string' ? query : '').slice(0, MEMORY_SEARCH_QUERY_MAX_LENGTH);
}

function snippetAround(text, query, len) {
  const normalizedQuery = normalizeSearchQuery(query);
  const lower = text.toLowerCase();
  const idx = lower.indexOf(normalizedQuery.toLowerCase());
  if (idx < 0) return text.substring(0, len);
  const start = Math.max(0, idx - Math.floor(len / 2));
  const end = Math.min(text.length, start + len);
  let s = text.substring(start, end);
  if (start > 0) s = '...' + s;
  if (end < text.length) s = s + '...';
  return s;
}

function highlightQuery(text, query) {
  if (!query) return escapeHtml(text);
  const escaped = escapeHtml(text);
  const normalizedQuery = normalizeSearchQuery(query);
  const queryEscaped = normalizedQuery.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const re = new RegExp('(' + queryEscaped + ')', 'gi');
  return escaped.replace(re, '<mark>$1</mark>');
}
// --- Logs ---

const LOG_MAX_ENTRIES = 2000;
let logsPaused = false;
let logBuffer = []; // buffer while paused

function connectLogSSE() {
  if (logEventSource) logEventSource.close();

  const logsUrl = token
    ? '/api/logs/events?token=' + encodeURIComponent(token)
    : '/api/logs/events';
  logEventSource = new EventSource(logsUrl);

  logEventSource.addEventListener('log', (e) => {
    const entry = JSON.parse(e.data);
    recentLogs.push(entry);
    while (recentLogs.length > QUICK_LOG_LIMIT) recentLogs.shift();
    if (quickSurfaceState.logs) renderQuickLogsSurface();
    if (logsPaused) {
      logBuffer.push(entry);
      return;
    }
    appendLogEntry(entry);
  });

  logEventSource.onerror = () => {
    // Silent reconnect
  };
}

function appendLogEntry(entry) {
  const output = document.getElementById('logs-output');

  // Level filter
  const levelFilter = document.getElementById('logs-level-filter').value;
  const targetFilter = document.getElementById('logs-target-filter').value.trim().toLowerCase();

  const div = document.createElement('div');
  div.className = 'log-entry level-' + entry.level;
  div.setAttribute('data-level', entry.level);
  div.setAttribute('data-target', entry.target);

  const ts = document.createElement('span');
  ts.className = 'log-ts';
  ts.textContent = entry.timestamp.substring(11, 23);
  div.appendChild(ts);

  const lvl = document.createElement('span');
  lvl.className = 'log-level';
  lvl.textContent = entry.level.padEnd(5);
  div.appendChild(lvl);

  const tgt = document.createElement('span');
  tgt.className = 'log-target';
  tgt.textContent = entry.target;
  div.appendChild(tgt);

  const msg = document.createElement('span');
  msg.className = 'log-msg';
  msg.textContent = entry.message;
  div.appendChild(msg);

  div.addEventListener('click', () => div.classList.toggle('expanded'));

  // Apply current filters as visibility
  const matchesLevel = levelFilter === 'all' || entry.level === levelFilter;
  const matchesTarget = !targetFilter || entry.target.toLowerCase().includes(targetFilter);
  if (!matchesLevel || !matchesTarget) {
    div.style.display = 'none';
  }

  output.appendChild(div);

  // Cap entries
  while (output.children.length > LOG_MAX_ENTRIES) {
    output.removeChild(output.firstChild);
  }

  // Auto-scroll
  if (document.getElementById('logs-autoscroll').checked) {
    output.scrollTop = output.scrollHeight;
  }
}

function toggleLogsPause() {
  logsPaused = !logsPaused;
  const btn = document.getElementById('logs-pause-btn');
  btn.textContent = logsPaused ? 'Resume' : 'Pause';

  if (!logsPaused) {
    // Flush buffer
    for (const entry of logBuffer) {
      appendLogEntry(entry);
    }
    logBuffer = [];
  }
}

function clearLogs() {
  if (!confirm('Clear all logs?')) return;
  document.getElementById('logs-output').innerHTML = '';
  logBuffer = [];
}

// Re-apply filters when level or target changes
document.getElementById('logs-level-filter').addEventListener('change', applyLogFilters);
document.getElementById('logs-target-filter').addEventListener('input', applyLogFilters);

function applyLogFilters() {
  const levelFilter = document.getElementById('logs-level-filter').value;
  const targetFilter = document.getElementById('logs-target-filter').value.trim().toLowerCase();
  const entries = document.querySelectorAll('#logs-output .log-entry');
  for (const el of entries) {
    const matchesLevel = levelFilter === 'all' || el.getAttribute('data-level') === levelFilter;
    const matchesTarget = !targetFilter || el.getAttribute('data-target').toLowerCase().includes(targetFilter);
    el.style.display = (matchesLevel && matchesTarget) ? '' : 'none';
  }
}

// --- Skills ---

function loadSkills() {
  const installedList = document.getElementById('skills-installed-list');
  const summary = document.getElementById('skills-summary');
  if (!installedList || !summary) return;

  installedList.innerHTML = '<div class="empty-state">Loading installed skills...</div>';

  apiFetch('/api/skills').then((data) => {
    const skills = (data && data.skills) || [];
    renderSkillsSummary(skills);
    renderInstalledSkills(skills);
  }).catch((err) => {
    summary.innerHTML = '<div class="summary-card failed"><div class="count">0</div><div class="label">Skills Unavailable</div></div>';
    installedList.innerHTML = '<div class="empty-state">Failed to load skills: ' + escapeHtml(err.message) + '</div>';
  });
}

function renderSkillsSummary(skills) {
  const summary = document.getElementById('skills-summary');
  if (!summary) return;
  const trusted = skills.filter((s) => String(s.trust).toLowerCase() === 'trusted').length;
  const installed = skills.filter((s) => String(s.trust).toLowerCase() === 'installed').length;
  summary.innerHTML = ''
    + summaryCard('Total', skills.length, '')
    + summaryCard('Trusted', trusted, 'completed')
    + summaryCard('Installed', installed, 'active');
}

function renderInstalledSkills(skills) {
  const installedList = document.getElementById('skills-installed-list');
  if (!installedList) return;
  if (!skills || skills.length === 0) {
    installedList.innerHTML = '<div class="empty-state">No skills installed</div>';
    return;
  }

  installedList.innerHTML = '';
  for (const skill of skills) {
    const card = document.createElement('div');
    card.className = 'skill-card';
    card.innerHTML = '<div class="skill-card-header">'
      + '<span class="skill-name">' + escapeHtml(skill.name) + '</span>'
      + '<span class="badge ' + (String(skill.trust).toLowerCase() === 'trusted' ? 'completed' : 'in_progress') + '">'
      + escapeHtml(skill.trust) + '</span>'
      + '</div>'
      + '<div class="skill-description">' + escapeHtml(skill.description || 'No description') + '</div>'
      + '<div class="skill-metadata">'
      + '<span>v' + escapeHtml(skill.version || '-') + '</span>'
      + '<span>' + escapeHtml(skill.source || '-') + '</span>'
      + '</div>';

    if (skill.keywords && skill.keywords.length > 0) {
      const keywords = document.createElement('div');
      keywords.className = 'skill-keywords';
      keywords.innerHTML = skill.keywords
        .slice(0, 8)
        .map((k) => '<span class="skill-keyword">' + escapeHtml(k) + '</span>')
        .join('');
      card.appendChild(keywords);
    }

    const actions = document.createElement('div');
    actions.className = 'skill-actions';
    const removeBtn = document.createElement('button');
    removeBtn.className = 'btn-cancel';
    removeBtn.textContent = 'Remove';
    removeBtn.addEventListener('click', () => removeSkill(skill.name));
    actions.appendChild(removeBtn);
    card.appendChild(actions);
    installedList.appendChild(card);
  }
}

function searchSkills() {
  const queryInput = document.getElementById('skills-search-query');
  const catalogList = document.getElementById('skills-catalog-list');
  if (!queryInput || !catalogList) return;
  const query = queryInput.value.trim();
  if (!query) {
    catalogList.innerHTML = '<div class="empty-state">Enter a search term</div>';
    return;
  }

  catalogList.innerHTML = '<div class="empty-state">Searching catalog...</div>';
  apiFetch('/api/skills/search', {
    method: 'POST',
    body: { query },
  }).then((data) => {
    const results = (data && data.catalog) || [];
    renderCatalogSkills(results);
  }).catch((err) => {
    catalogList.innerHTML = '<div class="empty-state">Search failed: ' + escapeHtml(err.message) + '</div>';
  });
}

function renderCatalogSkills(results) {
  const catalogList = document.getElementById('skills-catalog-list');
  if (!catalogList) return;
  if (!results || results.length === 0) {
    catalogList.innerHTML = '<div class="empty-state">No catalog results</div>';
    return;
  }

  catalogList.innerHTML = '';
  for (const item of results) {
    const slug = item.slug || item.name;
    const card = document.createElement('div');
    card.className = 'skill-card catalog-card';
    card.innerHTML = '<div class="skill-card-header">'
      + '<span class="skill-name">' + escapeHtml(item.name || slug) + '</span>'
      + '<span class="badge in_progress">score ' + escapeHtml(String(item.score || 0)) + '</span>'
      + '</div>'
      + '<div class="skill-description">' + escapeHtml(item.description || 'No description') + '</div>'
      + '<div class="skill-metadata">'
      + '<span>v' + escapeHtml(item.version || '-') + '</span>'
      + '<span>' + escapeHtml(slug || '-') + '</span>'
      + '</div>';

    const actions = document.createElement('div');
    actions.className = 'skill-actions';
    const installBtn = document.createElement('button');
    installBtn.className = 'btn-restart';
    installBtn.textContent = 'Install';
    installBtn.addEventListener('click', () => installSkill(slug));
    actions.appendChild(installBtn);
    card.appendChild(actions);
    catalogList.appendChild(card);
  }
}

function installSkill(name) {
  if (!name) return;
  apiFetch('/api/skills/install', {
    method: 'POST',
    headers: { 'X-Confirm-Action': 'true' },
    body: { name },
  }).then((res) => {
    if (res && res.success) {
      showToast(res.message || ('Installed ' + name), 'success');
      loadSkills();
      return;
    }
    showToast((res && res.message) || ('Install failed for ' + name), 'error');
  }).catch((err) => {
    showToast('Install failed: ' + err.message, 'error');
  });
}

function removeSkill(name) {
  if (!name) return;
  if (!confirm('Remove skill "' + name + '"?')) return;
  apiFetch('/api/skills/' + encodeURIComponent(name), {
    method: 'DELETE',
    headers: { 'X-Confirm-Action': 'true' },
  }).then((res) => {
    if (res && res.success) {
      showToast(res.message || ('Removed ' + name), 'success');
      loadSkills();
      return;
    }
    showToast((res && res.message) || ('Removal failed for ' + name), 'error');
  }).catch((err) => {
    showToast('Remove failed: ' + err.message, 'error');
  });
}

// --- Extensions ---

function loadExtensions() {
  const extList = document.getElementById('extensions-list');
  const toolsTbody = document.getElementById('tools-tbody');
  const toolsEmpty = document.getElementById('tools-empty');

  // Fetch both in parallel
  Promise.all([
    apiFetch('/api/extensions').catch(() => ({ extensions: [] })),
    apiFetch('/api/extensions/tools').catch(() => ({ tools: [] })),
  ]).then(([extData, toolData]) => {
    // Render extensions
    if (extData.extensions.length === 0) {
      extList.innerHTML = '<div class="empty-state">No extensions installed</div>';
    } else {
      extList.innerHTML = '';
      for (const ext of extData.extensions) {
        extList.appendChild(renderExtensionCard(ext));
      }
    }

    // Render tools
    if (toolData.tools.length === 0) {
      toolsTbody.innerHTML = '';
      toolsEmpty.style.display = 'block';
    } else {
      toolsEmpty.style.display = 'none';
      toolsTbody.innerHTML = toolData.tools.map((t) =>
        '<tr><td>' + escapeHtml(t.name) + '</td><td>' + escapeHtml(t.description) + '</td></tr>'
      ).join('');
    }
  });
}

function renderExtensionCard(ext) {
  const card = document.createElement('div');
  card.className = 'ext-card';

  const header = document.createElement('div');
  header.className = 'ext-header';

  const name = document.createElement('span');
  name.className = 'ext-name';
  name.textContent = ext.name;
  header.appendChild(name);

  const kind = document.createElement('span');
  kind.className = 'ext-kind kind-' + ext.kind;
  kind.textContent = ext.kind;
  header.appendChild(kind);

  const authDot = document.createElement('span');
  authDot.className = 'ext-auth-dot ' + (ext.authenticated ? 'authed' : 'unauthed');
  authDot.title = ext.authenticated ? 'Authenticated' : 'Not authenticated';
  header.appendChild(authDot);

  card.appendChild(header);

  if (ext.description) {
    const desc = document.createElement('div');
    desc.className = 'ext-desc';
    desc.textContent = ext.description;
    card.appendChild(desc);
  }

  if (ext.url) {
    const url = document.createElement('div');
    url.className = 'ext-url';
    url.textContent = ext.url;
    url.title = ext.url;
    card.appendChild(url);
  }

  if (ext.tools.length > 0) {
    const tools = document.createElement('div');
    tools.className = 'ext-tools';
    tools.textContent = 'Tools: ' + ext.tools.join(', ');
    card.appendChild(tools);
  }

  const actions = document.createElement('div');
  actions.className = 'ext-actions';

  if (!ext.active) {
    const activateBtn = document.createElement('button');
    activateBtn.className = 'btn-ext activate';
    activateBtn.textContent = 'Activate';
    activateBtn.addEventListener('click', () => activateExtension(ext.name));
    actions.appendChild(activateBtn);
  } else {
    const activeLabel = document.createElement('span');
    activeLabel.className = 'ext-active-label';
    activeLabel.textContent = 'Active';
    actions.appendChild(activeLabel);
  }

  const removeBtn = document.createElement('button');
  removeBtn.className = 'btn-ext remove';
  removeBtn.textContent = 'Remove';
  removeBtn.addEventListener('click', () => removeExtension(ext.name));
  actions.appendChild(removeBtn);

  card.appendChild(actions);
  return card;
}

function activateExtension(name) {
  apiFetch('/api/extensions/' + encodeURIComponent(name) + '/activate', { method: 'POST' })
    .then((res) => {
      if (res.success) {
        loadExtensions();
        return;
      }

      if (res.auth_url) {
        showToast('Opening authentication for ' + name, 'info');
        window.open(res.auth_url, '_blank');
      } else if (res.awaiting_token) {
        showToast(res.instructions || 'Please provide an API token for ' + name, 'info');
      } else {
        showToast('Activate failed: ' + res.message, 'error');
      }
      loadExtensions();
    })
    .catch((err) => showToast('Activate failed: ' + err.message, 'error'));
}

function removeExtension(name) {
  if (!confirm('Remove extension "' + name + '"?')) return;
  apiFetch('/api/extensions/' + encodeURIComponent(name) + '/remove', { method: 'POST' })
    .then((res) => {
      if (!res.success) {
        showToast('Remove failed: ' + res.message, 'error');
      } else {
        showToast('Removed ' + name, 'success');
      }
      loadExtensions();
    })
    .catch((err) => showToast('Remove failed: ' + err.message, 'error'));
}

// --- Jobs ---

let currentJobId = null;
let currentJobSubTab = 'overview';
let jobFilesTreeState = null;

function loadJobs() {
  currentJobId = null;
  jobFilesTreeState = null;

  // Rebuild DOM if renderJobDetail() destroyed it (it wipes .jobs-container innerHTML).
  const container = document.querySelector('.jobs-container');
  if (!document.getElementById('jobs-summary')) {
    container.innerHTML =
      '<div class="jobs-summary" id="jobs-summary"></div>'
      + '<table class="jobs-table" id="jobs-table"><thead><tr>'
      + '<th>ID</th><th>Title</th><th>Status</th><th>Created</th><th>Actions</th>'
      + '</tr></thead><tbody id="jobs-tbody"></tbody></table>'
      + '<div class="empty-state" id="jobs-empty" style="display:none">No jobs found</div>';
  }

  Promise.all([
    apiFetch('/api/jobs/summary'),
    apiFetch('/api/jobs'),
  ]).then(([summary, jobList]) => {
    renderJobsSummary(summary);
    renderJobsList(jobList.jobs);
  }).catch(() => {});
}

function renderJobsSummary(s) {
  document.getElementById('jobs-summary').innerHTML = ''
    + summaryCard('Total', s.total, '')
    + summaryCard('In Progress', s.in_progress, 'active')
    + summaryCard('Completed', s.completed, 'completed')
    + summaryCard('Failed', s.failed, 'failed')
    + summaryCard('Stuck', s.stuck, 'stuck');
}

function summaryCard(label, count, cls) {
  return '<div class="summary-card ' + cls + '">'
    + '<div class="count">' + count + '</div>'
    + '<div class="label">' + label + '</div>'
    + '</div>';
}

function renderJobsList(jobs) {
  const tbody = document.getElementById('jobs-tbody');
  const empty = document.getElementById('jobs-empty');

  if (jobs.length === 0) {
    tbody.innerHTML = '';
    empty.style.display = 'block';
    return;
  }

  empty.style.display = 'none';
  tbody.innerHTML = jobs.map((job) => {
    const shortId = job.id.substring(0, 8);
    const stateClass = job.state.replace(' ', '_');

    let actionBtns = '';
    if (job.state === 'pending' || job.state === 'in_progress') {
      actionBtns = '<button class="btn-cancel" onclick="event.stopPropagation(); cancelJob(\'' + job.id + '\')">Cancel</button>';
    } else if (job.state === 'failed' || job.state === 'interrupted') {
      actionBtns = '<button class="btn-restart" onclick="event.stopPropagation(); restartJob(\'' + job.id + '\')">Restart</button>';
    }

    return '<tr class="job-row" onclick="openJobDetail(\'' + job.id + '\')">'
      + '<td title="' + escapeHtml(job.id) + '">' + shortId + '</td>'
      + '<td>' + escapeHtml(job.title) + '</td>'
      + '<td><span class="badge ' + stateClass + '">' + escapeHtml(job.state) + '</span></td>'
      + '<td>' + formatDate(job.created_at) + '</td>'
      + '<td>' + actionBtns + '</td>'
      + '</tr>';
  }).join('');
}

function cancelJob(jobId) {
  if (!confirm('Cancel this job?')) return;
  apiFetch('/api/jobs/' + jobId + '/cancel', { method: 'POST' })
    .then(() => {
      showToast('Job cancelled', 'success');
      if (currentJobId) openJobDetail(currentJobId);
      else loadJobs();
    })
    .catch((err) => {
      showToast('Failed to cancel job: ' + err.message, 'error');
    });
}

function restartJob(jobId) {
  apiFetch('/api/jobs/' + jobId + '/restart', { method: 'POST' })
    .then((res) => {
      showToast('Job restarted as ' + (res.new_job_id || '').substring(0, 8), 'success');
      loadJobs();
    })
    .catch((err) => {
      showToast('Failed to restart job: ' + err.message, 'error');
    });
}

function openJobDetail(jobId) {
  currentJobId = jobId;
  currentJobSubTab = 'activity';
  apiFetch('/api/jobs/' + jobId).then((job) => {
    renderJobDetail(job);
  }).catch((err) => {
    addMessage('system', 'Failed to load job: ' + err.message);
    closeJobDetail();
  });
}

function closeJobDetail() {
  currentJobId = null;
  jobFilesTreeState = null;
  loadJobs();
}

function renderJobDetail(job) {
  const container = document.querySelector('.jobs-container');
  const stateClass = job.state.replace(' ', '_');

  container.innerHTML = '';

  // Header
  const header = document.createElement('div');
  header.className = 'job-detail-header';

  let headerHtml = '<button class="btn-back" onclick="closeJobDetail()">&larr; Back</button>'
    + '<h2>' + escapeHtml(job.title) + '</h2>'
    + '<span class="badge ' + stateClass + '">' + escapeHtml(job.state) + '</span>';

  if (job.state === 'failed' || job.state === 'interrupted') {
    headerHtml += '<button class="btn-restart" onclick="restartJob(\'' + job.id + '\')">Restart</button>';
  }
  if (job.browse_url) {
    headerHtml += '<a class="btn-browse" href="' + escapeHtml(job.browse_url) + '" target="_blank">Browse Files</a>';
  }

  header.innerHTML = headerHtml;
  container.appendChild(header);

  // Sub-tab bar
  const tabs = document.createElement('div');
  tabs.className = 'job-detail-tabs';
  const subtabs = ['overview', 'activity', 'files'];
  for (const st of subtabs) {
    const btn = document.createElement('button');
    btn.textContent = st.charAt(0).toUpperCase() + st.slice(1);
    btn.className = st === currentJobSubTab ? 'active' : '';
    btn.addEventListener('click', () => {
      currentJobSubTab = st;
      renderJobDetail(job);
    });
    tabs.appendChild(btn);
  }
  container.appendChild(tabs);

  // Content
  const content = document.createElement('div');
  content.className = 'job-detail-content';
  container.appendChild(content);

  switch (currentJobSubTab) {
    case 'overview': renderJobOverview(content, job); break;
    case 'files': renderJobFiles(content, job); break;
    case 'activity': renderJobActivity(content, job); break;
  }
}

function metaItem(label, value) {
  return '<div class="meta-item"><div class="meta-label">' + escapeHtml(label)
    + '</div><div class="meta-value">' + escapeHtml(String(value != null ? value : '-'))
    + '</div></div>';
}

function formatDuration(secs) {
  if (secs == null) return '-';
  if (secs < 60) return secs + 's';
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  if (m < 60) return m + 'm ' + s + 's';
  const h = Math.floor(m / 60);
  return h + 'h ' + (m % 60) + 'm';
}

function renderJobOverview(container, job) {
  // Metadata grid
  const grid = document.createElement('div');
  grid.className = 'job-meta-grid';
  grid.innerHTML = metaItem('Job ID', job.id)
    + metaItem('State', job.state)
    + metaItem('Created', formatDate(job.created_at))
    + metaItem('Started', formatDate(job.started_at))
    + metaItem('Completed', formatDate(job.completed_at))
    + metaItem('Duration', formatDuration(job.elapsed_secs))
    + (job.job_mode ? metaItem('Mode', job.job_mode) : '');
  container.appendChild(grid);

  // Description
  if (job.description) {
    const descSection = document.createElement('div');
    descSection.className = 'job-description';
    const descHeader = document.createElement('h3');
    descHeader.textContent = 'Description';
    descSection.appendChild(descHeader);
    const descBody = document.createElement('div');
    descBody.className = 'job-description-body';
    descBody.innerHTML = renderMarkdown(job.description);
    descSection.appendChild(descBody);
    container.appendChild(descSection);
  }

  // State transitions timeline
  if (job.transitions.length > 0) {
    const timelineSection = document.createElement('div');
    timelineSection.className = 'job-timeline-section';
    const tlHeader = document.createElement('h3');
    tlHeader.textContent = 'State Transitions';
    timelineSection.appendChild(tlHeader);

    const timeline = document.createElement('div');
    timeline.className = 'timeline';
    for (const t of job.transitions) {
      const entry = document.createElement('div');
      entry.className = 'timeline-entry';
      const dot = document.createElement('div');
      dot.className = 'timeline-dot';
      entry.appendChild(dot);
      const info = document.createElement('div');
      info.className = 'timeline-info';
      info.innerHTML = '<span class="badge ' + t.from.replace(' ', '_') + '">' + escapeHtml(t.from) + '</span>'
        + ' &rarr; '
        + '<span class="badge ' + t.to.replace(' ', '_') + '">' + escapeHtml(t.to) + '</span>'
        + '<span class="timeline-time">' + formatDate(t.timestamp) + '</span>'
        + (t.reason ? '<div class="timeline-reason">' + escapeHtml(t.reason) + '</div>' : '');
      entry.appendChild(info);
      timeline.appendChild(entry);
    }
    timelineSection.appendChild(timeline);
    container.appendChild(timelineSection);
  }
}

function renderJobFiles(container, job) {
  container.innerHTML = '<div class="job-files">'
    + '<div class="job-files-sidebar"><div class="job-files-tree"></div></div>'
    + '<div class="job-files-viewer"><div class="empty-state">Select a file to view</div></div>'
    + '</div>';

  container._jobId = job ? job.id : null;

  apiFetch('/api/jobs/' + job.id + '/files/list?path=').then((data) => {
    jobFilesTreeState = data.entries.map((e) => ({
      name: e.name,
      path: e.path,
      is_dir: e.is_dir,
      children: e.is_dir ? null : undefined,
      expanded: false,
      loaded: false,
    }));
    renderJobFilesTree();
  }).catch(() => {
    const treeContainer = document.querySelector('.job-files-tree');
    if (treeContainer) {
      treeContainer.innerHTML = '<div class="tree-item" style="color:var(--text-secondary)">No project files</div>';
    }
  });
}

function renderJobFilesTree() {
  const treeContainer = document.querySelector('.job-files-tree');
  if (!treeContainer) return;
  treeContainer.innerHTML = '';
  if (!jobFilesTreeState || jobFilesTreeState.length === 0) {
    treeContainer.innerHTML = '<div class="tree-item" style="color:var(--text-secondary)">No files in workspace</div>';
    return;
  }
  renderJobFileNodes(jobFilesTreeState, treeContainer, 0);
}

function renderJobFileNodes(nodes, container, depth) {
  for (const node of nodes) {
    const row = document.createElement('div');
    row.className = 'tree-row';
    row.style.paddingLeft = (depth * 16 + 8) + 'px';

    if (node.is_dir) {
      const arrow = document.createElement('span');
      arrow.className = 'expand-arrow' + (node.expanded ? ' expanded' : '');
      arrow.textContent = '\u25B6';
      arrow.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleJobFileExpand(node);
      });
      row.appendChild(arrow);

      const label = document.createElement('span');
      label.className = 'tree-label dir';
      label.textContent = node.name;
      label.addEventListener('click', () => toggleJobFileExpand(node));
      row.appendChild(label);
    } else {
      const spacer = document.createElement('span');
      spacer.className = 'expand-arrow-spacer';
      row.appendChild(spacer);

      const label = document.createElement('span');
      label.className = 'tree-label file';
      label.textContent = node.name;
      label.addEventListener('click', () => readJobFile(node.path));
      row.appendChild(label);
    }

    container.appendChild(row);

    if (node.is_dir && node.expanded && node.children) {
      const childContainer = document.createElement('div');
      childContainer.className = 'tree-children';
      renderJobFileNodes(node.children, childContainer, depth + 1);
      container.appendChild(childContainer);
    }
  }
}

function getJobId() {
  const container = document.querySelector('.job-detail-content');
  return (container && container._jobId) || null;
}

function toggleJobFileExpand(node) {
  if (node.expanded) {
    node.expanded = false;
    renderJobFilesTree();
    return;
  }
  if (node.loaded) {
    node.expanded = true;
    renderJobFilesTree();
    return;
  }
  const jobId = getJobId();
  apiFetch('/api/jobs/' + jobId + '/files/list?path=' + encodeURIComponent(node.path)).then((data) => {
    node.children = data.entries.map((e) => ({
      name: e.name,
      path: e.path,
      is_dir: e.is_dir,
      children: e.is_dir ? null : undefined,
      expanded: false,
      loaded: false,
    }));
    node.loaded = true;
    node.expanded = true;
    renderJobFilesTree();
  }).catch(() => {});
}

function readJobFile(path) {
  const viewer = document.querySelector('.job-files-viewer');
  if (!viewer) return;
  const jobId = getJobId();
  apiFetch('/api/jobs/' + jobId + '/files/read?path=' + encodeURIComponent(path)).then((data) => {
    viewer.innerHTML = '<div class="job-files-path">' + escapeHtml(path) + '</div>'
      + '<pre class="job-files-content">' + escapeHtml(data.content) + '</pre>';
  }).catch((err) => {
    viewer.innerHTML = '<div class="empty-state">Error: ' + escapeHtml(err.message) + '</div>';
  });
}

// --- Activity tab (unified for all sandbox jobs) ---

let activityCurrentJobId = null;
// Track how many live SSE events we've already rendered so refreshActivityTab
// only appends new ones (avoids duplicates on each SSE tick).
let activityRenderedLiveIndex = 0;

function renderJobActivity(container, job) {
  activityCurrentJobId = job ? job.id : null;
  activityRenderedLiveIndex = 0;

  container.innerHTML = '<div class="activity-toolbar">'
    + '<select id="activity-type-filter">'
    + '<option value="all">All Events</option>'
    + '<option value="message">Messages</option>'
    + '<option value="tool_use">Tool Calls</option>'
    + '<option value="tool_result">Results</option>'
    + '</select>'
    + '<label class="logs-checkbox"><input type="checkbox" id="activity-autoscroll" checked> Auto-scroll</label>'
    + '</div>'
    + '<div class="activity-terminal" id="activity-terminal"></div>'
    + '<div class="activity-input-bar" id="activity-input-bar">'
    + '<input type="text" id="activity-prompt-input" placeholder="Send follow-up prompt..." />'
    + '<button id="activity-send-btn">Send</button>'
    + '<button id="activity-done-btn" title="Signal done">Done</button>'
    + '</div>';

  document.getElementById('activity-type-filter').addEventListener('change', applyActivityFilter);

  const terminal = document.getElementById('activity-terminal');
  const input = document.getElementById('activity-prompt-input');
  const sendBtn = document.getElementById('activity-send-btn');
  const doneBtn = document.getElementById('activity-done-btn');

  sendBtn.addEventListener('click', () => sendJobPrompt(job.id, false));
  doneBtn.addEventListener('click', () => sendJobPrompt(job.id, true));
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') sendJobPrompt(job.id, false);
  });

  // Load persisted events from DB, then catch up with any live SSE events
  apiFetch('/api/jobs/' + job.id + '/events').then((data) => {
    if (data.events && data.events.length > 0) {
      for (const evt of data.events) {
        appendActivityEvent(terminal, evt.event_type, evt.data);
      }
    }
    appendNewLiveEvents(terminal, job.id);
  }).catch(() => {
    appendNewLiveEvents(terminal, job.id);
  });
}

function appendNewLiveEvents(terminal, jobId) {
  const live = jobEvents.get(jobId) || [];
  for (let i = activityRenderedLiveIndex; i < live.length; i++) {
    const evt = live[i];
    appendActivityEvent(terminal, evt.type.replace('job_', ''), evt.data);
  }
  activityRenderedLiveIndex = live.length;
  const autoScroll = document.getElementById('activity-autoscroll');
  if (!autoScroll || autoScroll.checked) {
    terminal.scrollTop = terminal.scrollHeight;
  }
}

function applyActivityFilter() {
  const filter = document.getElementById('activity-type-filter').value;
  const events = document.querySelectorAll('#activity-terminal .activity-event');
  for (const el of events) {
    if (filter === 'all') {
      el.style.display = '';
    } else {
      el.style.display = el.getAttribute('data-event-type') === filter ? '' : 'none';
    }
  }
}

function appendActivityEvent(terminal, eventType, data) {
  if (!terminal) return;
  const el = document.createElement('div');
  el.className = 'activity-event activity-event-' + eventType;
  el.setAttribute('data-event-type', eventType);

  // Respect current filter
  const filterEl = document.getElementById('activity-type-filter');
  if (filterEl && filterEl.value !== 'all' && filterEl.value !== eventType) {
    el.style.display = 'none';
  }

  switch (eventType) {
    case 'message':
      el.innerHTML = '<span class="activity-role">' + escapeHtml(data.role || 'assistant') + '</span> '
        + '<span class="activity-content">' + escapeHtml(data.content || '') + '</span>';
      break;
    case 'tool_use':
      el.innerHTML = '<details class="activity-tool-block"><summary>'
        + '<span class="activity-tool-icon">&#9881;</span> '
        + escapeHtml(data.tool_name || 'tool')
        + '</summary><pre class="activity-tool-input">'
        + escapeHtml(typeof data.input === 'string' ? data.input : JSON.stringify(data.input, null, 2))
        + '</pre></details>';
      break;
    case 'tool_result':
      el.innerHTML = '<details class="activity-tool-block activity-tool-result"><summary>'
        + '<span class="activity-tool-icon">&#10003;</span> '
        + escapeHtml(data.tool_name || 'result')
        + '</summary><pre class="activity-tool-output">'
        + escapeHtml(data.output || '')
        + '</pre></details>';
      break;
    case 'status':
      el.innerHTML = '<span class="activity-status">' + escapeHtml(data.message || '') + '</span>';
      break;
    case 'result':
      el.className += ' activity-final';
      const success = data.success !== false;
      el.innerHTML = '<span class="activity-result-status" data-success="' + success + '">'
        + escapeHtml(data.message || data.status || 'done') + '</span>';
      if (data.session_id) {
        el.innerHTML += ' <span class="activity-session-id">session: ' + escapeHtml(data.session_id) + '</span>';
      }
      break;
    default:
      el.innerHTML = '<span class="activity-status">' + escapeHtml(JSON.stringify(data)) + '</span>';
  }

  terminal.appendChild(el);
}

function refreshActivityTab(jobId) {
  if (activityCurrentJobId !== jobId) return;
  if (currentJobSubTab !== 'activity') return;
  const terminal = document.getElementById('activity-terminal');
  if (!terminal) return;
  appendNewLiveEvents(terminal, jobId);
}

function sendJobPrompt(jobId, done) {
  const input = document.getElementById('activity-prompt-input');
  const content = input ? input.value.trim() : '';
  if (!content && !done) return;

  apiFetch('/api/jobs/' + jobId + '/prompt', {
    method: 'POST',
    body: { content: content || '(done)', done: done },
  }).then(() => {
    if (input) input.value = '';
    if (done) {
      const bar = document.getElementById('activity-input-bar');
      if (bar) bar.innerHTML = '<span class="activity-status">Done signal sent</span>';
    }
  }).catch((err) => {
    const terminal = document.getElementById('activity-terminal');
    if (terminal) {
      appendActivityEvent(terminal, 'status', { message: 'Failed to send: ' + err.message });
    }
  });
}

// --- Routines ---

let currentRoutineId = null;

function loadRoutines() {
  currentRoutineId = null;

  // Restore list view if detail was open
  const detail = document.getElementById('routine-detail');
  if (detail) detail.style.display = 'none';
  const table = document.getElementById('routines-table');
  if (table) table.style.display = '';

  Promise.all([
    apiFetch('/api/routines/summary'),
    apiFetch('/api/routines'),
  ]).then(([summary, listData]) => {
    renderRoutinesSummary(summary);
    renderRoutinesList(listData.routines);
  }).catch(() => {});
}

function renderRoutinesSummary(s) {
  document.getElementById('routines-summary').innerHTML = ''
    + summaryCard('Total', s.total, '')
    + summaryCard('Enabled', s.enabled, 'active')
    + summaryCard('Disabled', s.disabled, '')
    + summaryCard('Failing', s.failing, 'failed')
    + summaryCard('Runs Today', s.runs_today, 'completed');
}

function renderRoutinesList(routines) {
  const tbody = document.getElementById('routines-tbody');
  const empty = document.getElementById('routines-empty');

  if (!routines || routines.length === 0) {
    tbody.innerHTML = '';
    empty.style.display = 'block';
    return;
  }

  empty.style.display = 'none';
  tbody.innerHTML = routines.map((r) => {
    const statusClass = r.status === 'active' ? 'completed'
      : r.status === 'failing' ? 'failed'
      : 'pending';

    const toggleLabel = r.enabled ? 'Disable' : 'Enable';
    const toggleClass = r.enabled ? 'btn-cancel' : 'btn-restart';

    return '<tr class="routine-row" onclick="openRoutineDetail(\'' + r.id + '\')">'
      + '<td>' + escapeHtml(r.name) + '</td>'
      + '<td>' + escapeHtml(r.trigger_summary) + '</td>'
      + '<td>' + escapeHtml(r.action_type) + '</td>'
      + '<td>' + formatRelativeTime(r.last_run_at) + '</td>'
      + '<td>' + formatRelativeTime(r.next_fire_at) + '</td>'
      + '<td>' + r.run_count + '</td>'
      + '<td><span class="badge ' + statusClass + '">' + escapeHtml(r.status) + '</span></td>'
      + '<td>'
      + '<button class="' + toggleClass + '" onclick="event.stopPropagation(); toggleRoutine(\'' + r.id + '\')">' + toggleLabel + '</button> '
      + '<button class="btn-restart" onclick="event.stopPropagation(); triggerRoutine(\'' + r.id + '\')">Run</button> '
      + '<button class="btn-cancel" onclick="event.stopPropagation(); deleteRoutine(\'' + r.id + '\', \'' + escapeHtml(r.name) + '\')">Delete</button>'
      + '</td>'
      + '</tr>';
  }).join('');
}

function openRoutineDetail(id) {
  currentRoutineId = id;
  apiFetch('/api/routines/' + id).then((routine) => {
    renderRoutineDetail(routine);
  }).catch((err) => {
    showToast('Failed to load routine: ' + err.message, 'error');
  });
}

function closeRoutineDetail() {
  currentRoutineId = null;
  loadRoutines();
}

function renderRoutineDetail(routine) {
  const table = document.getElementById('routines-table');
  if (table) table.style.display = 'none';
  document.getElementById('routines-empty').style.display = 'none';

  const detail = document.getElementById('routine-detail');
  detail.style.display = 'block';

  const statusClass = !routine.enabled ? 'pending'
    : routine.consecutive_failures > 0 ? 'failed'
    : 'completed';
  const statusLabel = !routine.enabled ? 'disabled'
    : routine.consecutive_failures > 0 ? 'failing'
    : 'active';

  let html = '<div class="job-detail-header">'
    + '<button class="btn-back" onclick="closeRoutineDetail()">&larr; Back</button>'
    + '<h2>' + escapeHtml(routine.name) + '</h2>'
    + '<span class="badge ' + statusClass + '">' + escapeHtml(statusLabel) + '</span>'
    + '</div>';

  // Metadata grid
  html += '<div class="job-meta-grid">'
    + metaItem('Routine ID', routine.id)
    + metaItem('Enabled', routine.enabled ? 'Yes' : 'No')
    + metaItem('Run Count', routine.run_count)
    + metaItem('Failures', routine.consecutive_failures)
    + metaItem('Last Run', formatDate(routine.last_run_at))
    + metaItem('Next Fire', formatDate(routine.next_fire_at))
    + metaItem('Created', formatDate(routine.created_at))
    + '</div>';

  // Description
  if (routine.description) {
    html += '<div class="job-description"><h3>Description</h3>'
      + '<div class="job-description-body">' + escapeHtml(routine.description) + '</div></div>';
  }

  // Trigger config
  html += '<div class="job-description"><h3>Trigger</h3>'
    + '<pre class="action-json">' + escapeHtml(JSON.stringify(routine.trigger, null, 2)) + '</pre></div>';

  // Action config
  html += '<div class="job-description"><h3>Action</h3>'
    + '<pre class="action-json">' + escapeHtml(JSON.stringify(routine.action, null, 2)) + '</pre></div>';

  // Recent runs
  if (routine.recent_runs && routine.recent_runs.length > 0) {
    html += '<div class="job-timeline-section"><h3>Recent Runs</h3>'
      + '<table class="routines-table"><thead><tr>'
      + '<th>Trigger</th><th>Started</th><th>Completed</th><th>Status</th><th>Summary</th><th>Tokens</th>'
      + '</tr></thead><tbody>';
    for (const run of routine.recent_runs) {
      const runStatusClass = run.status === 'Ok' ? 'completed'
        : run.status === 'Failed' ? 'failed'
        : run.status === 'Attention' ? 'stuck'
        : 'in_progress';
      html += '<tr>'
        + '<td>' + escapeHtml(run.trigger_type) + '</td>'
        + '<td>' + formatDate(run.started_at) + '</td>'
        + '<td>' + formatDate(run.completed_at) + '</td>'
        + '<td><span class="badge ' + runStatusClass + '">' + escapeHtml(run.status) + '</span></td>'
        + '<td>' + escapeHtml(run.result_summary || '-') + '</td>'
        + '<td>' + (run.tokens_used != null ? run.tokens_used : '-') + '</td>'
        + '</tr>';
    }
    html += '</tbody></table></div>';
  }

  detail.innerHTML = html;
}

function triggerRoutine(id) {
  apiFetch('/api/routines/' + id + '/trigger', { method: 'POST' })
    .then(() => showToast('Routine triggered', 'success'))
    .catch((err) => showToast('Trigger failed: ' + err.message, 'error'));
}

function toggleRoutine(id) {
  apiFetch('/api/routines/' + id + '/toggle', { method: 'POST' })
    .then((res) => {
      showToast('Routine ' + (res.status || 'toggled'), 'success');
      if (currentRoutineId) openRoutineDetail(currentRoutineId);
      else loadRoutines();
    })
    .catch((err) => showToast('Toggle failed: ' + err.message, 'error'));
}

function deleteRoutine(id, name) {
  if (!confirm('Delete routine "' + name + '"?')) return;
  apiFetch('/api/routines/' + id, { method: 'DELETE' })
    .then(() => {
      showToast('Routine deleted', 'success');
      if (currentRoutineId === id) closeRoutineDetail();
      else loadRoutines();
    })
    .catch((err) => showToast('Delete failed: ' + err.message, 'error'));
}

function formatRelativeTime(isoString) {
  if (!isoString) return '-';
  const d = new Date(isoString);
  const now = Date.now();
  const diffMs = now - d.getTime();
  const absDiff = Math.abs(diffMs);
  const future = diffMs < 0;

  if (absDiff < 60000) return future ? 'in <1m' : '<1m ago';
  if (absDiff < 3600000) {
    const m = Math.floor(absDiff / 60000);
    return future ? 'in ' + m + 'm' : m + 'm ago';
  }
  if (absDiff < 86400000) {
    const h = Math.floor(absDiff / 3600000);
    return future ? 'in ' + h + 'h' : h + 'h ago';
  }
  const days = Math.floor(absDiff / 86400000);
  return future ? 'in ' + days + 'd' : days + 'd ago';
}

// --- Gateway status widget ---

let gatewayStatusInterval = null;

function startGatewayStatusPolling() {
  fetchGatewayStatus();
  gatewayStatusInterval = setInterval(fetchGatewayStatus, 30000);
}

function fetchGatewayStatus() {
  apiFetch('/api/gateway/status').then((data) => {
    const popover = document.getElementById('gateway-popover');
    popover.innerHTML = '<div class="gw-stat"><span>SSE clients</span><span>' + (data.sse_connections || 0) + '</span></div>'
      + '<div class="gw-stat"><span>WS clients</span><span>' + (data.ws_connections || 0) + '</span></div>'
      + '<div class="gw-stat"><span>Total</span><span>' + (data.total_connections || 0) + '</span></div>'
      + '<div class="gw-stat"><span>Channels</span><span>' + escapeHtml(String(data.channel_status || 'unknown')) + '</span></div>'
      + '<div class="gw-stat"><span>Verification</span><span>' + escapeHtml(String(data.verification_status || 'unknown')) + '</span></div>';
    if (data.routine_webhook_status) {
      popover.innerHTML += '<div class="gw-stat"><span>Automation Webhooks</span><span>'
        + escapeHtml(String(data.routine_webhook_status))
        + '</span></div>';
    }
    if (quickSurfaceState.usage) refreshQuickUsageSurface();
  }).catch(() => {});
}

function toggleSettingsPanel() {
  settingsPanelVisible = !settingsPanelVisible;
  const panel = document.getElementById('settings-panel');
  if (!panel) return;
  panel.classList.toggle('visible', settingsPanelVisible);
  if (settingsPanelVisible) loadSettingsPanel();
}

function loadSettingsPanel() {
  const body = document.getElementById('settings-panel-body');
  if (!body) return;

  Promise.all([
    apiFetch('/api/settings/export').catch(() => ({ settings: {} })),
    apiFetch('/api/gateway/status').catch(() => null),
  ]).then(([settingsData, gateway]) => {
    const settings = (settingsData && settingsData.settings) || {};

    body.innerHTML = ''
      + '<div class="setting-row"><span>Runtime Instance</span><strong>'
      + escapeHtml('runtime@' + (window.location.host || 'local'))
      + '</strong></div>'
      + '<div class="setting-row"><span>Gateway Channel Status</span><strong>'
      + escapeHtml(gateway ? String(gateway.channel_status || 'unknown') : 'unknown')
      + '</strong></div>'
      + '<div class="setting-row"><span>LLM Backend</span><strong>'
      + escapeHtml(String(readSetting(settings, 'llm_backend', 'not configured')))
      + '</strong></div>'
      + '<div class="setting-row"><span>Selected Model</span><strong>'
      + escapeHtml(String(readSetting(settings, 'selected_model', 'not selected')))
      + '</strong></div>'
      + '<div class="setting-row"><span>Hyperliquid Network</span><strong>'
      + escapeHtml(String(readSetting(settings, 'hyperliquid_runtime.network', 'testnet')))
      + '</strong></div>'
      + '<div class="setting-row"><span>Custody Mode</span><strong>'
      + escapeHtml(String(readSetting(settings, 'wallet_vault_policy.custody_mode', 'operator_wallet')))
      + '</strong></div>'
      + '<div class="setting-row"><span>Verification Backend</span><strong>'
      + escapeHtml(String(readSetting(settings, 'verification_backend.backend', 'not configured')))
      + '</strong></div>'
      + '<div class="setting-row"><span>Embeddings Enabled</span><strong>'
      + escapeHtml(boolLabel(toBoolean(readSetting(settings, 'embeddings.enabled', false), false)))
      + '</strong></div>';
  }).catch((err) => {
    body.innerHTML = '<div class="empty-state">Failed to load settings: ' + escapeHtml(err.message) + '</div>';
  });
}

// Show/hide popover on hover
document.getElementById('gateway-status-trigger').addEventListener('mouseenter', () => {
  document.getElementById('gateway-popover').classList.add('visible');
});
document.getElementById('gateway-status-trigger').addEventListener('mouseleave', () => {
  document.getElementById('gateway-popover').classList.remove('visible');
});

// --- Extension install ---

function installExtension() {
  const name = document.getElementById('ext-install-name').value.trim();
  if (!name) {
    showToast('Extension name is required', 'error');
    return;
  }
  const url = document.getElementById('ext-install-url').value.trim();
  const kind = document.getElementById('ext-install-kind').value;

  apiFetch('/api/extensions/install', {
    method: 'POST',
    body: { name, url: url || undefined, kind },
  }).then((res) => {
    if (res.success) {
      showToast('Installed ' + name, 'success');
      document.getElementById('ext-install-name').value = '';
      document.getElementById('ext-install-url').value = '';
      loadExtensions();
    } else {
      showToast('Install failed: ' + (res.message || 'unknown error'), 'error');
    }
  }).catch((err) => {
    showToast('Install failed: ' + err.message, 'error');
  });
}

// --- Keyboard shortcuts ---

document.addEventListener('keydown', (e) => {
  const mod = e.metaKey || e.ctrlKey;
  const tag = (e.target.tagName || '').toLowerCase();
  const inInput = tag === 'input' || tag === 'textarea';

  // Mod+1-9: switch tabs
  if (mod && /^[1-9]$/.test(e.key)) {
    e.preventDefault();
    const tabs = Array.from(document.querySelectorAll('.tab-bar button[data-tab]'))
      .map((button) => button.getAttribute('data-tab'));
    const idx = parseInt(e.key) - 1;
    if (tabs[idx]) switchTab(tabs[idx]);
    return;
  }

  // Mod+K: focus chat input or memory search
  if (mod && e.key === 'k') {
    e.preventDefault();
    if (currentTab === 'memory') {
      document.getElementById('memory-search').focus();
    } else {
      document.getElementById('chat-input').focus();
    }
    return;
  }

  // Mod+N: new thread
  if (mod && e.key === 'n' && currentTab === 'chat') {
    e.preventDefault();
    createNewThread();
    return;
  }

  // Escape: close job detail or blur input
  if (e.key === 'Escape') {
    if (currentJobId) {
      closeJobDetail();
    } else if (inInput) {
      e.target.blur();
    }
    return;
  }
});

// --- Toasts ---

function showToast(message, type) {
  const container = document.getElementById('toasts');
  const toast = document.createElement('div');
  toast.className = 'toast toast-' + (type || 'info');
  toast.textContent = message;
  container.appendChild(toast);
  // Trigger slide-in
  requestAnimationFrame(() => toast.classList.add('visible'));
  setTimeout(() => {
    toast.classList.remove('visible');
    toast.addEventListener('transitionend', () => toast.remove());
  }, 4000);
}

// --- Utilities ---

function escapeHtml(str) {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}

function formatDate(isoString) {
  if (!isoString) return '-';
  const d = new Date(isoString);
  return d.toLocaleString();
}

window.addEventListener('beforeunload', () => {
  stopOpsRefresh();
  stopQuickSurfaceRefresh();
  if (gatewayStatusInterval) clearInterval(gatewayStatusInterval);
});
