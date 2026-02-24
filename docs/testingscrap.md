cargo fmt --all
cargo check
cargo test -p enclagent platform::tests:: -- --nocapture
cargo test -p enclagent agent::submission::tests:: -- --nocapture
cargo test -p enclagent agent::commands::tests:: -- --nocapture
cargo test -p enclagent agent::dispatcher::tests:: -- --nocapture