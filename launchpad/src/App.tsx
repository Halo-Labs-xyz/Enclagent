import React from "react";
import PrivyLaunchpad from "./PrivyLaunchpad";

function App() {
  const [bootstrap, setBootstrap] = React.useState<Bootstrap | null>(null);
  const [error, setError] = React.useState<string | null>(null);

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

  return <PrivyLaunchpad bootstrap={bootstrap} />;
}

export interface Bootstrap {
  enabled: boolean;
  require_privy?: boolean;
  privy_app_id?: string | null;
  privy_client_id?: string | null;
  poll_interval_ms?: number;
}

export default App;
