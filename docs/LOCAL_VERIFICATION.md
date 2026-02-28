# Local Verification

Run from repository root.

## Full gate

```bash
./scripts/verify-local.sh
```

## Individual commands

```bash
npm run build:launchpad
cargo +1.92.0 fmt --all --check
cargo +1.92.0 check
cargo +1.92.0 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.92.0 test --workspace --all-features -- --nocapture --test-threads=1
```

## Launchpad/frontdoor static asset behavior

- The gateway serves `src/channels/web/static/*` from filesystem first, with embedded fallback.
- After web UI changes, run `npm run build:launchpad` and restart the gateway process.
- A Rust rebuild is not required for static asset refresh in local/dev flows.
- Launchpad chunk files are hashed and gateway returns `Cache-Control: no-store` for static assets to avoid stale browser chunk mismatches.

## Rebrand policy

No legacy product/runtime names are allowed in active runtime code.
