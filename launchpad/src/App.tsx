import React from "react";

const PrivyLaunchpad = React.lazy(() => import("./PrivyLaunchpad"));

function App() {
  const [bootstrap, setBootstrap] = React.useState<Bootstrap | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [identityFlowStarted, setIdentityFlowStarted] = React.useState(false);

  React.useEffect(() => {
    fetch("/api/frontdoor/bootstrap")
      .then((r) => r.json())
      .then(setBootstrap)
      .catch((e) => setError(String(e?.message || e)));
  }, []);

  if (error) {
    return (
      <div className="lp-shell" style={{ padding: 24 }}>
        <p style={{ color: "var(--danger)" }}>Failed to load: {error}</p>
      </div>
    );
  }

  if (!bootstrap) {
    return (
      <div className="lp-shell" style={{ padding: 24 }}>
        <p className="lp-eyebrow">Loading...</p>
      </div>
    );
  }

  if (!bootstrap.enabled) {
    return (
      <div className="lp-shell" style={{ padding: 24 }}>
        <p style={{ color: "var(--danger)" }}>
          Launchpad is disabled. Enable GATEWAY_FRONTDOOR_ENABLED=true.
        </p>
      </div>
    );
  }

  const appId = String(bootstrap.privy_app_id || "").trim();
  if (!appId) {
    return (
      <div className="lp-shell" style={{ padding: 24 }}>
        <p style={{ color: "var(--danger)" }}>
          Privy launch is required but no Privy App ID is configured.
        </p>
      </div>
    );
  }

  if (!identityFlowStarted) {
    return (
      <main className="lp-shell">
        <section className="lp-chat-card">
          <div className="lp-chat-head">
            <h2>Launchpad Chat</h2>
            <p>Step 1 required: secure identity via Privy.</p>
          </div>
          <div className="lp-chat-stream">
            <div className="lp-msg assistant">
              Welcome. First step: sign up or log in with Privy to provision your wallet identity before configuration.
            </div>
          </div>
          <div className="lp-chat-actions">
            <button
              type="button"
              className="lp-action-btn"
              onClick={() => setIdentityFlowStarted(true)}
            >
              Start Privy Sign Up
            </button>
          </div>
        </section>
      </main>
    );
  }

  return (
    <React.Suspense
      fallback={
        <div className="lp-shell" style={{ padding: 24 }}>
          <p className="lp-eyebrow">Initializing secure identity...</p>
        </div>
      }
    >
      <PrivyLaunchpad bootstrap={bootstrap} />
    </React.Suspense>
  );
}

export interface Bootstrap {
  enabled: boolean;
  require_privy?: boolean;
  privy_app_id?: string | null;
  privy_client_id?: string | null;
  poll_interval_ms?: number;
}

export default App;
