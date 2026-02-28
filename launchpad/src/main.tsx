import React from "react";
import { createRoot } from "react-dom/client";

const rootEl = document.getElementById("root");
if (!rootEl) throw new Error("Root element not found");
const root = createRoot(rootEl);

root.render(
  <React.StrictMode>
    <div className="lp-shell" style={{ padding: 24 }}>
      <p className="lp-eyebrow">Loading launchpad...</p>
    </div>
  </React.StrictMode>
);

const mountApp = async () => {
  const { default: App } = await import("./App");
  root.render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  );
};

if ("requestIdleCallback" in window) {
  (window as Window & { requestIdleCallback: (cb: () => void, opts?: { timeout: number }) => number }).requestIdleCallback(
    () => {
      void mountApp();
    },
    { timeout: 250 }
  );
} else {
  setTimeout(() => {
    void mountApp();
  }, 0);
}
