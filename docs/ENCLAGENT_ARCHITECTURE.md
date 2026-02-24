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
    frontdoor["Frontdoor + Provisioning Orchestrator"]
    org["Org Workspace + Membership APIs"]
    modules["Module Catalog + State APIs"]
    router["4-Layer Inference Router"]
    policy["Policy Guardrail Engine"]
  end

  subgraph execution["Execution + Verification"]
    core["General/Developer/Creative/Research/BizOps/Comms Modules"]
    addons["Optional Addons (Hyperliquid, EigenDA)"]
    verify["EigenCloud Primary + Signed Fallback"]
    artifacts["Intent / Receipt / Verification Artifacts"]
  end

  subgraph state["State"]
    memory["Workspace Memory"]
    db["Postgres or libSQL"]
    secrets["Encrypted Secrets Store"]
  end

  repl --> router
  gateway --> frontdoor
  gateway --> org
  gateway --> modules
  gateway --> router
  webhook --> router
  wasm_channels --> router

  frontdoor --> org
  modules --> policy
  router --> policy
  policy --> core
  policy --> addons
  core --> artifacts
  addons --> artifacts
  verify --> artifacts

  router <--> memory
  memory <--> db
  router --> secrets
```

Core references:

- `src/platform/mod.rs`
- `src/channels/web/server.rs`
- `src/channels/web/frontdoor.rs`
- `src/channels/web/types.rs`
- `src/agent/intent.rs`
