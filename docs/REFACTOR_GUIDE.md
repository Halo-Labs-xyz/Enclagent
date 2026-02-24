# Refactor Guide — Enclagent-Only Repo

Context and refactor guide for agent sessions. Goal: **one product (Enclagent), one runtime (Rust), one gateway (embedded).** Remove hyperClaw, ironclaw, frontdoor-next, and all unnecessary folders so the repo contains only what is required for a minimal, flawless Enclagent launchpad.

Paste this document or §9 One-Paragraph Brief into another agent session when executing the cleanup.

---

## 1. Executive Summary

**Vision: Secure Verifiable Agent Launchpad.**  
Single product — Enclagent: Hyperliquid-first agent with enclave provisioning, frontdoor (wallet + challenge + config + verify + session), and deterministic intent/receipt/verification lineage. **No separate Next.js apps.** The Rust binary is the runtime and the gateway; it serves the frontdoor API and static UI.

**Target capabilities (all in-repo, no hyperClaw/ironclaw):**
- **Generalized config** — FrontdoorUserConfig in Rust (`src/channels/web/types.rs`); single source of truth; placeholders for provisioning.
- **Multi-modal entry** — REPL, web gateway (HTTP/SSE/WS), HTTP webhook, WASM channels; frontdoor at `/api/frontdoor/*` (bootstrap, challenge, suggest-config, verify, session) with static UI in `src/channels/web/static/` (frontdoor.html, frontdoor.js, app.js).
- **Verifiable execution** — IntentEnvelope → ExecutionReceipt → VerificationRecord in Rust; EigenCloud primary + signed fallback; paper default, policy-gated live.

---

## 2. Target Codebase Structure (After Cleanup)

**Keep only:**

| Path | Role |
|------|------|
| **Repository root** | Enclagent Rust crate: `Cargo.toml` (name `enclagent`), `src/` (agent, channels, tools, workspace, db, frontdoor, gateway, intent/receipt/verification). Single binary: `enclagent`. |
| **benchmarks/** | Rust benchmarks (optional; keep if used). |
| **tests/** | Integration tests. |
| **migrations/** | DB migrations (Postgres + libSQL). |
| **wit/** | WIT definitions for WASM tools/channels. |
| **tools-src/** | WASM tool crates (telegram, slack, github, etc.). |
| **channels-src/** | WASM channel crates. |
| **deploy/** | Systemd, Docker, ecloud provisioning scripts. |
| **scripts/** | verify-local, provision-user-ecloud, etc. |
| **docs/** | Pruned: keep REFACTOR_GUIDE, FRONTDOOR_ENCLAVE_FLOW, architecture/PRD (rename LIQUIDCLAW_* → ENCLAGENT_* when touching). Remove or archive hyperClaw/ironclaw-specific docs. |
| **.env.example, .gitignore, wix/** | Config and installer. |
| **sdk/** | **Optional.** Keep only if you need `npx enclagent init` / npm-based onboard; otherwise remove. |

**Remove entirely:**

| Path | Reason |
|------|--------|
| **hyperClaw/** | Next.js app (launchpad, A2A, Supermemory, bridge routes). All required behavior lives in the Rust gateway and static assets. Removing it eliminates a whole stack and duplicate frontdoor/API surface. |
| **hyperClaw/cli** | CLI that talks to hyperClaw; obsolete when hyperClaw is gone. |
| **frontdoor-next/** | Standalone Next.js frontdoor. Redundant: Rust gateway already serves `/api/frontdoor/*` and static frontdoor UI. |
| **ironclaw/** | If present: upstream/reference snapshot. Not needed for Enclagent; already removed per MONOREPO_WORKSPACE. Delete any remaining references. |

**Root `package.json`:**  
Remove workspaces that point at removed trees. After cleanup, either (a) delete root `package.json` if no JS workspaces remain, or (b) keep only `sdk` in workspaces if you retain sdk.

---

## 3. Architecture (Single-Product)

```
User entry points (REPL, Web Gateway, HTTP Webhook, WASM channels)
    → Enclagent binary (one process)
        → Web gateway (Axum): /api/frontdoor/*, /api/chat/*, /api/memory/*, /api/jobs/*, …
        → Static UI: /, /gateway, /frontdoor.js, /app.js, frontdoor.html
    → Frontdoor flow (bootstrap → challenge → suggest-config → verify → session)
    → Provisioning (GATEWAY_FRONTDOOR_PROVISION_COMMAND; placeholders via env)
    → Per-user enclave (separate Enclagent instance + config)
    → Intent/receipt/verification (in-process; types in src/agent/intent.rs, src/tools/hyperliquid.rs)
```

There is no separate “dashboard” app. The gateway’s static pages and optional future minimal SPA (under `src/channels/web/static/`) are the UI. Bridge-style HTTP routes (intents, execute, verify, runs) and A2A/Supermemory currently live only in hyperClaw; if needed later, add them as routes in the Rust gateway; do not reintroduce a Next.js app.

---

## 4. Key Data Structures (Unchanged)

All of these remain in the Rust codebase; no dependency on hyperClaw or frontdoor-next.

- **FrontdoorUserConfig** — `src/channels/web/types.rs` (and re-exported from frontdoor.rs). Fields: config_version, profile_domain, domain_overrides, inference_*, profile_name, hyperliquid_network, paper_live_policy, custody_mode, *_wallet_address, vault_address, gateway_auth_key, eigencloud_auth_key, verification_*, accept_terms, risk/API/symbol/kill_switch/memory options.
- **Frontdoor API (Rust gateway)** — All under `/api/frontdoor/`:
  - `GET /api/frontdoor/bootstrap`
  - `GET /api/frontdoor/config-contract`
  - `POST /api/frontdoor/challenge`
  - `POST /api/frontdoor/suggest-config`
  - `POST /api/frontdoor/verify`
  - `GET /api/frontdoor/session/{session_id}`
- **Provisioning placeholders** — Injected as env vars into `GATEWAY_FRONTDOOR_PROVISION_COMMAND`. Full list and semantics: `docs/FRONTDOOR_ENCLAVE_FLOW.md`.
- **Intent/receipt/verification types** — `src/agent/intent.rs`, `src/tools/hyperliquid.rs`; used in-process. No HTTP contract dependency on hyperClaw.

---

## 5. What We Are Not Pursuing (After Cleanup)

- **No HyperClaw** — No Next.js execution engine, no separate launchpad/A2A/Supermemory app.
- **No ironclaw directory or references** — No upstream snapshot; no scripts/docs that assume ironclaw.
- **No frontdoor-next** — No second frontdoor app; the gateway’s frontdoor is the only one.
- **No duplicate frontdoor API** — Canonical frontdoor is `/api/frontdoor/*` on the Enclagent gateway. Do not reintroduce `/api/enclagent/frontdoor/*` or `/api/liquidclaw/*` unless you are adding a thin proxy in Rust.

Optional future (in-repo only): TUI frontdoor (CLI that calls `/api/frontdoor/*`), inference-driven config generation, or minimal gateway routes for intent/execute/verify/runs if a tiny dashboard needs them — all without adding back hyperClaw or frontdoor-next.

---

## 6. Step-by-Step Removal and Cleanup

Execute in order. Verify build and tests after each phase.

**Phase 1 — Delete trees**

1. Remove directory `hyperClaw/` (entire tree, including `hyperClaw/cli`).
2. Remove directory `frontdoor-next/`.
3. If `ironclaw/` exists, remove `ironclaw/`.

**Phase 2 — Root package.json and workspaces**

4. Open root `package.json`. Set `workspaces` to `[]` or remove the `workspaces` key; or, if keeping sdk only, set `workspaces` to `["sdk"]`. Remove any script that references `hyperClaw`, `frontdoor-next`, or `ironclaw`.
5. If no workspaces remain and you do not need root npm at all, you may delete root `package.json` and `package-lock.json`; otherwise keep them and run `npm install` at root to refresh lockfile.

**Phase 3 — References in repo**

6. **Scripts:** In `scripts/`, remove or rewrite any script that invokes hyperClaw, frontdoor-next, or ironclaw (e.g. `verify-local.sh`, `release-demo-readiness.sh`). Replace with Enclagent-only checks (e.g. `cargo build`, `cargo test`, gateway health).
7. **Docs:** In `docs/`, remove or archive hyperClaw/ironclaw-specific files (e.g. LIQUIDCLAW_FRONTDOOR_ONBOARDING that points at hyperClaw, IRONCLAW_*, HCLAW_*, UNIBASE_*, PRODUCTION_DEMO_24H if tied to Next app). Keep REFACTOR_GUIDE.md, FRONTDOOR_ENCLAVE_FLOW.md, and PRD/architecture docs; when editing PRDs, rename LIQUIDCLAW_* to ENCLAGENT_*.
8. **README / AGENTS / CLAUDE / MONOREPO:** Update to describe a single-product repo (Enclagent only). Remove instructions that start hyperClaw or frontdoor-next. Update `docs/MONOREPO_WORKSPACE.md` to state that the monorepo is deprecated or that only `sdk` remains if kept; do not list hyperClaw or frontdoor-next.

**Phase 4 — Naming and contracts**

9. Ensure no remaining code or docs reference `liquidclaw` or `LiquidClaw` for the product; use `enclagent` / `Enclagent`. Gateway API stays at `/api/frontdoor/*` (no `/api/liquidclaw/` or `/api/enclagent/` prefix required for frontdoor).
10. Grep for `hyperClaw`, `ironclaw`, `frontdoor-next`, `liquidclaw` (case-insensitive) across the repo and fix or remove remaining references (comments, doc links, env examples).

**Phase 5 — Validation**

11. `cargo build` and `cargo test` at repo root. Fix any breakage (e.g. tests that referenced hyperClaw or frontdoor-next).
12. If you kept sdk, run `npm install` and any sdk build/lint; ensure sdk does not depend on hyperClaw or frontdoor-next.

---

## 7. File Map (Post-Cleanup)

| Concern | Path | Notes |
|---------|------|--------|
| Agent runtime | `src/`, `Cargo.toml` | Single crate `enclagent`. |
| Web gateway | `src/channels/web/server.rs` | All API and static routes. |
| Frontdoor API + service | `src/channels/web/frontdoor.rs`, `server.rs` | Routes under `/api/frontdoor/*`. |
| Frontdoor types | `src/channels/web/types.rs` | FrontdoorUserConfig, etc. |
| Static UI | `src/channels/web/static/` | frontdoor.html, frontdoor.js, frontdoor.css, app.js, style.css, index. |
| Intent/receipt/verification | `src/agent/intent.rs`, `src/tools/hyperliquid.rs` | In-process; no HTTP dependency on removed apps. |
| Provisioning | `scripts/provision-user-ecloud.sh`, `deploy/ecloud-instance.env` | Placeholder injection unchanged. |
| Config | `src/channels/web/types.rs` (Rust) | Single source; no TS duplicate. |
| PRD / architecture | `docs/` (pruned) | Rename LIQUIDCLAW_* → ENCLAGENT_* when editing. |

---

## 8. Success Criteria

1. **No hyperClaw, no ironclaw, no frontdoor-next** — Directories removed; no workspaces or scripts referencing them.
2. **Single binary** — `enclagent` builds and runs; gateway and frontdoor are served from it.
3. **Frontdoor contract** — `/api/frontdoor/bootstrap`, `challenge`, `suggest-config`, `verify`, `session/{id}` work against the Rust gateway; static frontdoor UI loads from gateway.
4. **Placeholders** — Provisioning still uses env-injected placeholders; semantics documented in FRONTDOOR_ENCLAVE_FLOW.md.
5. **Verification** — Intent → receipt → verification lineage and paper default unchanged in Rust.
6. **Naming** — Repo and docs say Enclagent; no LiquidClaw/hyperClaw/ironclaw in new or updated code/docs.
7. **Build and test** — `cargo build`, `cargo test` pass; optional sdk (if kept) installs and builds.

---

## 9. One-Paragraph Brief

Enclagent is a single-product repo: one Rust runtime and embedded gateway with frontdoor at `/api/frontdoor/*` and static UI in `src/channels/web/static/`. Remove hyperClaw, frontdoor-next, and ironclaw (and all references); keep only the Enclagent crate, tools-src, channels-src, wit, deploy, scripts, migrations, tests, and pruned docs; optionally sdk. Root package.json workspaces must not list removed trees. Preserve Frontdoor API contract, placeholder-based provisioning, and intent/receipt/verification behavior in Rust. Use enclagent/Enclagent naming and ~/.enclagent. See docs/REFACTOR_GUIDE.md.

---

## 10. References

| Doc | Purpose |
|-----|---------|
| `docs/REFACTOR_GUIDE.md` | This file. |
| `docs/FRONTDOOR_ENCLAVE_FLOW.md` | Frontdoor flow, placeholders, provisioning. |
| `docs/LIQUIDCLAW_VERIFIABLE_HL_AGENT_KIT_PRD.md` | Product directive (rename to ENCLAGENT_* when editing). |
| `docs/LIQUIDCLAW_E2E_TODO_PRD.md` | Execution checklist (rename to ENCLAGENT_* when editing). |
| `docs/LIQUIDCLAW_ARCHITECTURE.md` | Mermaid diagram (update labels to Enclagent). |
| `AGENTS.md` | Branch strategy, decision filters. |
| `CLAUDE.md` | Build, structure, patterns. |

After cleanup, remove or archive any doc that only applied to hyperClaw/ironclaw/frontdoor-next.
