import React from "react";
import { createRoot } from "react-dom/client";
import { Buffer } from "buffer";

const g = globalThis as unknown as { Buffer?: typeof Buffer; global?: unknown };
if (!g.global) {
  g.global = globalThis;
}
if (!g.Buffer) {
  g.Buffer = Buffer;
}

const rootEl = document.getElementById("root");
if (!rootEl) throw new Error("Root element not found");

void import("./App").then(({ default: App }) => {
  createRoot(rootEl).render(
    <App />
  );
});
