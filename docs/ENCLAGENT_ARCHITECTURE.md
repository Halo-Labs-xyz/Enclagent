# Enclagent Architecture

Canonical high-level architecture for the Enclagent runtime.

```mermaid
flowchart TB
  subgraph ingress["Ingress Surface"]
    repl["REPL Console"]
    gateway["Web Gateway (HTTP/SSE/WS)"]
    webhook["HTTP Webhook"]
    wasm_channels["WASM Channels"]
  end

  subgraph control["Control Plane"]
    router["Agent Loop + Router"]
    scheduler["Scheduler + Routine Engine"]
    policy["Policy Engine"]
  end

  subgraph execution["Execution + Verification"]
    hl_tool["Hyperliquid Tooling"]
    verify["Verification Backend + Signed Fallback"]
    artifacts["Intent / Receipt / Verification Artifacts"]
  end

  subgraph state["State"]
    memory["Workspace Memory"]
    db["Postgres or libSQL"]
    secrets["Encrypted Secrets Store"]
  end

  repl --> router
  gateway --> router
  webhook --> router
  wasm_channels --> router

  router --> scheduler
  scheduler --> router
  router --> policy
  policy --> hl_tool
  hl_tool --> artifacts
  verify --> artifacts

  router <--> memory
  memory <--> db
  router --> secrets
```

Core references:

- `src/channels/web/server.rs`
- `src/channels/web/frontdoor.rs`
- `src/agent/intent.rs`
- `src/tools/hyperliquid.rs`
