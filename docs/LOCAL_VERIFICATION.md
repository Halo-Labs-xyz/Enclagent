# Local Verification

Run from repository root.

## Full gate

```bash
./scripts/verify-local.sh
```

## Individual commands

```bash
cargo +1.92.0 fmt --all --check
cargo +1.92.0 check
cargo +1.92.0 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.92.0 test --workspace --all-features -- --nocapture --test-threads=1
```

## Rebrand policy

No legacy product/runtime names are allowed in active runtime code.
