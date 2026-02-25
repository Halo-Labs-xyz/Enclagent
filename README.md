<p align="center">
  <img src="enclagent.png" alt="Enclagent" width="200"/>
</p>

<h1 align="center">Enclagent</h1>

<p align="center">
  <strong>Verifiable agent runtime with embedded gateway, frontdoor, and modular addons.</strong>
</p>

## Overview

Enclagent is a single-product Rust runtime.

- One binary (`enclagent`)
- One embedded web gateway (`/api/*`)
- One frontdoor contract (`/api/frontdoor/*`)
- Deterministic intent/receipt/verification lineage
- Core-8 module suite with addons (i.e. Hyperliquid, EigenDA)
- Explicit policy gates for live trading actions when trading addon is enabled

No separate Next.js applications are part of this repository.

## Repository Layout

- `src/` runtime, gateway, frontdoor, agent, tools, safety, secrets
- `tests/` integration tests
- `migrations/` database schema migrations
- `channels-src/` WASM channel sources
- `tools-src/` WASM tool sources
- `deploy/` deployment and service assets
- `scripts/` local verification and operational scripts
- `docs/` product, architecture, and operational docs

## Quick Start

Prerequisites:

- Rust 1.92+
- `wasm32-wasip2` target

Bootstrap dev environment:

```bash
./scripts/dev-setup.sh
```

Run onboarding:

```bash
cargo +1.92.0 run -- onboard
```

Run runtime:

```bash
cargo +1.92.0 run
```

Run startup doctor:

```bash
cargo +1.92.0 run -- doctor startup
```

## Frontdoor Contract

The embedded gateway serves the canonical frontdoor endpoints:

- `GET /api/frontdoor/bootstrap`
- `GET /api/frontdoor/config-contract`
- `POST /api/frontdoor/challenge`
- `POST /api/frontdoor/suggest-config`
- `POST /api/frontdoor/verify`
- `GET /api/frontdoor/session/{session_id}`
- `GET /api/frontdoor/sessions?wallet_address=<0x...>&limit=<n>`

Static frontdoor assets are under `src/channels/web/static/`.

## Validation

Fast checks:

```bash
cargo fmt --all --check
cargo check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features -- --nocapture
```

Full local gate:

```bash
./scripts/verify-local.sh
```

## Documentation Index

- Product directive: `docs/ENCLAGENT_VERIFIABLE_HL_AGENT_KIT_PRD.md`
- Execution checklist: `docs/ENCLAGENT_E2E_TODO_PRD.md`
- Architecture: `docs/ENCLAGENT_ARCHITECTURE.md`
- Fork status: `docs/ENCLAGENT_FORK_STATUS.md`
- Frontdoor flow: `docs/FRONTDOOR_ENCLAVE_FLOW.md`
- Local verification: `docs/LOCAL_VERIFICATION.md`
- Channel builds: `docs/BUILDING_CHANNELS.md`
- Telegram setup: `docs/TELEGRAM_SETUP.md`
- Refactor guide: `docs/REFACTOR_GUIDE.md`

## License

Licensed under either:

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT (`LICENSE-MIT`)
