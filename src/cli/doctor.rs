//! `enclagent doctor` - active health diagnostics.
//!
//! Probes external dependencies and validates configuration to surface
//! problems before they bite during normal operation. Each check reports
//! pass/fail with actionable guidance on failures.

use std::path::PathBuf;
use std::time::Duration;

use clap::Subcommand;

/// Optional doctor subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum DoctorSubcommand {
    /// Validate startup prerequisites (working directory, ports, and MCP reachability).
    Startup,
}

/// Run diagnostic checks and print results.
pub async fn run_doctor_command(
    command: Option<DoctorSubcommand>,
    strict: bool,
) -> anyhow::Result<()> {
    match command {
        Some(DoctorSubcommand::Startup) => run_startup_checks(strict).await,
        None => run_full_doctor(strict).await,
    }
}

async fn run_full_doctor(strict: bool) -> anyhow::Result<()> {
    println!("Enclagent Doctor");
    println!("===============\n");

    let mut passed = 0u32;
    let mut failed = 0u32;
    let hyperliquid_context = load_hyperliquid_doctor_context();

    // ── Configuration checks ──────────────────────────────────

    check(
        "NEAR AI session",
        check_nearai_session().await,
        &mut passed,
        &mut failed,
    );

    check(
        "Database backend",
        check_database().await,
        &mut passed,
        &mut failed,
    );

    check(
        "Workspace directory",
        check_workspace_dir(),
        &mut passed,
        &mut failed,
    );

    check(
        "Startup working directory guardrail",
        check_startup_working_directory(),
        &mut passed,
        &mut failed,
    );

    check(
        "Hyperliquid credentials",
        check_hyperliquid_credentials(&hyperliquid_context),
        &mut passed,
        &mut failed,
    );

    check(
        "Hyperliquid API reachability",
        check_hyperliquid_api_reachability(&hyperliquid_context).await,
        &mut passed,
        &mut failed,
    );

    check(
        "MCP startup preflight",
        check_mcp_startup_preflight(None).await,
        &mut passed,
        &mut failed,
    );

    check(
        "Vault address presence",
        check_hyperliquid_vault_presence(&hyperliquid_context),
        &mut passed,
        &mut failed,
    );

    check(
        "Verification backend reachability",
        check_verification_backend_reachability(&hyperliquid_context).await,
        &mut passed,
        &mut failed,
    );

    // ── External binary checks ────────────────────────────────

    check(
        "Docker",
        check_binary("docker", &["--version"]),
        &mut passed,
        &mut failed,
    );

    check(
        "cloudflared",
        check_binary("cloudflared", &["--version"]),
        &mut passed,
        &mut failed,
    );

    check(
        "ngrok",
        check_binary("ngrok", &["version"]),
        &mut passed,
        &mut failed,
    );

    check(
        "tailscale",
        check_binary("tailscale", &["version"]),
        &mut passed,
        &mut failed,
    );

    // ── Summary ───────────────────────────────────────────────

    println!();
    println!("  {passed} passed, {failed} failed");

    if failed > 0 {
        println!("\n  Some checks failed. This is normal if you don't use those features.");
        if strict {
            anyhow::bail!("doctor strict mode failed with {failed} check(s)");
        }
    }

    Ok(())
}

async fn run_startup_checks(strict: bool) -> anyhow::Result<()> {
    println!("Enclagent Doctor (startup)");
    println!("===========================\n");

    let mut passed = 0u32;
    let mut failed = 0u32;

    check(
        "Startup working directory guardrail",
        check_startup_working_directory(),
        &mut passed,
        &mut failed,
    );

    check(
        "Database backend",
        check_database().await,
        &mut passed,
        &mut failed,
    );

    check(
        "HTTP gateway bind port",
        check_gateway_port_available(),
        &mut passed,
        &mut failed,
    );

    check(
        "MCP startup preflight",
        check_mcp_startup_preflight(Some(Duration::from_secs(3))).await,
        &mut passed,
        &mut failed,
    );

    println!();
    println!("  {passed} passed, {failed} failed");

    if failed > 0 {
        println!("\n  Startup preflight failed. Fix the listed checks before booting runtime.");
        if strict {
            anyhow::bail!("doctor startup strict mode failed with {failed} check(s)");
        }
    }

    Ok(())
}

// ── Individual checks ───────────────────────────────────────

fn check(name: &str, result: CheckResult, passed: &mut u32, failed: &mut u32) {
    match result {
        CheckResult::Pass(detail) => {
            *passed += 1;
            println!("  [pass] {name}: {detail}");
        }
        CheckResult::Fail(detail) => {
            *failed += 1;
            println!("  [FAIL] {name}: {detail}");
        }
        CheckResult::Skip(reason) => {
            println!("  [skip] {name}: {reason}");
        }
    }
}

enum CheckResult {
    Pass(String),
    Fail(String),
    Skip(String),
}

struct HyperliquidDoctorContext {
    runtime: Result<crate::config::HyperliquidRuntimeConfig, String>,
    wallet: Result<crate::config::WalletVaultPolicyConfig, String>,
    verification: Result<crate::config::VerificationBackendConfig, String>,
}

fn load_hyperliquid_doctor_context() -> HyperliquidDoctorContext {
    match load_settings_for_doctor() {
        Ok(settings) => HyperliquidDoctorContext {
            runtime: crate::config::HyperliquidRuntimeConfig::resolve(&settings)
                .map_err(|e| e.to_string()),
            wallet: crate::config::WalletVaultPolicyConfig::resolve(&settings)
                .map_err(|e| e.to_string()),
            verification: crate::config::VerificationBackendConfig::resolve(&settings)
                .map_err(|e| e.to_string()),
        },
        Err(e) => HyperliquidDoctorContext {
            runtime: Err(e.clone()),
            wallet: Err(e.clone()),
            verification: Err(e),
        },
    }
}

fn load_settings_for_doctor() -> Result<crate::settings::Settings, String> {
    let _ = dotenvy::dotenv();
    crate::bootstrap::load_enclagent_env();

    let mut settings = crate::settings::Settings::load();
    let toml_path = crate::settings::Settings::default_toml_path();

    match crate::settings::Settings::load_toml(&toml_path) {
        Ok(Some(toml_settings)) => settings.merge_from(&toml_settings),
        Ok(None) => {}
        Err(e) => {
            return Err(format!(
                "failed to load TOML config at {}: {e}",
                toml_path.display()
            ));
        }
    }

    Ok(settings)
}

fn check_startup_working_directory() -> CheckResult {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            return CheckResult::Fail(format!("cannot resolve current directory: {error}"));
        }
    };

    let cwd_has_enclagent_manifest = cwd.join("Cargo.toml").exists()
        && std::fs::read_to_string(cwd.join("Cargo.toml"))
            .ok()
            .is_some_and(|content| content.contains("name = \"enclagent\""));
    if cwd_has_enclagent_manifest {
        return CheckResult::Pass(format!("running from {}", cwd.display()));
    }

    let probable_workspace_root = cwd.join("enclagent").join("Cargo.toml").exists();

    if probable_workspace_root {
        return CheckResult::Fail(format!(
            "detected workspace root {}; run from enclagent repo:\n    cd enclagent && ~/.cargo/bin/cargo +1.92.0 run -- run",
            cwd.display()
        ));
    }

    CheckResult::Pass(format!(
        "cwd {} (non-workspace path; no guardrail violation)",
        cwd.display()
    ))
}

fn check_gateway_port_available() -> CheckResult {
    let settings = match load_settings_for_doctor() {
        Ok(settings) => settings,
        Err(error) => return CheckResult::Fail(error),
    };

    if !settings.channels.http_enabled {
        return CheckResult::Skip("HTTP gateway disabled".to_string());
    }

    let port = settings.channels.http_port.unwrap_or(3014);
    let bind_addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    match std::net::TcpListener::bind(bind_addr) {
        Ok(listener) => {
            drop(listener);
            CheckResult::Pass(format!("port {} is available", port))
        }
        Err(error) => CheckResult::Fail(format!(
            "port {} is unavailable ({}); free the port or change CHANNEL_HTTP_PORT",
            port, error
        )),
    }
}

async fn check_mcp_startup_preflight(timeout_override: Option<Duration>) -> CheckResult {
    let timeout = timeout_override.unwrap_or(Duration::from_secs(4));
    let servers = match crate::tools::mcp::config::load_mcp_servers().await {
        Ok(servers) => servers,
        Err(error) => {
            return CheckResult::Fail(format!("cannot load MCP server config: {error}"));
        }
    };

    let enabled: Vec<_> = servers.enabled_servers().cloned().collect();
    if enabled.is_empty() {
        return CheckResult::Skip("no enabled MCP servers configured".to_string());
    }

    let health =
        crate::tools::mcp::health::probe_enabled_servers(enabled.into_iter(), timeout).await;
    let failures: Vec<_> = health.iter().filter(|entry| !entry.is_healthy()).collect();
    let summary = health
        .iter()
        .map(|entry| format!("{}={}", entry.name, entry.state.as_str()))
        .collect::<Vec<_>>()
        .join(", ");

    if failures.is_empty() {
        CheckResult::Pass(summary)
    } else {
        let details = failures
            .iter()
            .map(|entry| format!("{}:{} ({})", entry.name, entry.state.as_str(), entry.detail))
            .collect::<Vec<_>>()
            .join("; ");
        CheckResult::Fail(details)
    }
}

fn check_hyperliquid_credentials(context: &HyperliquidDoctorContext) -> CheckResult {
    let wallet = match &context.wallet {
        Ok(wallet) => wallet,
        Err(e) => return CheckResult::Fail(format!("wallet/vault config invalid: {e}")),
    };

    let mut issues = Vec::new();
    let mut present = 0usize;

    let operator_required = matches!(
        wallet.custody_mode,
        crate::config::CustodyMode::OperatorWallet | crate::config::CustodyMode::DualMode
    );
    let user_required = matches!(
        wallet.custody_mode,
        crate::config::CustodyMode::UserWallet | crate::config::CustodyMode::DualMode
    );

    validate_wallet_credential(
        "operator wallet",
        wallet.operator_wallet_address.as_deref(),
        operator_required,
        &mut issues,
        &mut present,
    );
    validate_wallet_credential(
        "user wallet",
        wallet.user_wallet_address.as_deref(),
        user_required,
        &mut issues,
        &mut present,
    );

    match &context.verification {
        Ok(verification)
            if verification.backend
                == crate::config::VerificationBackendKind::EigenCloudPrimary =>
        {
            let token = verification
                .eigencloud
                .auth_token
                .as_deref()
                .map(str::trim)
                .unwrap_or("");
            if token.is_empty() {
                issues.push("missing EigenCloud auth token for eigencloud_primary backend".into());
            }
        }
        Ok(_) => {}
        Err(e) => issues.push(format!("verification backend config invalid: {e}")),
    }

    if issues.is_empty() {
        CheckResult::Pass(format!(
            "{} credentials ready ({present} wallet credential(s) present)",
            custody_mode_label(wallet.custody_mode)
        ))
    } else {
        CheckResult::Fail(issues.join("; "))
    }
}

fn check_hyperliquid_vault_presence(context: &HyperliquidDoctorContext) -> CheckResult {
    let wallet = match &context.wallet {
        Ok(wallet) => wallet,
        Err(e) => return CheckResult::Fail(format!("wallet/vault config invalid: {e}")),
    };

    let Some(vault_address) = wallet.vault_address.as_deref().map(str::trim) else {
        return CheckResult::Fail(
            "vault address not configured (set HYPERLIQUID_VAULT_ADDRESS)".into(),
        );
    };

    if !is_valid_wallet_address(vault_address) {
        return CheckResult::Fail(
            "vault address format invalid (expected 0x + 40 hex chars)".into(),
        );
    }

    CheckResult::Pass(format!(
        "vault configured ({})",
        mask_wallet_address(vault_address)
    ))
}

async fn check_hyperliquid_api_reachability(context: &HyperliquidDoctorContext) -> CheckResult {
    let runtime = match &context.runtime {
        Ok(runtime) => runtime,
        Err(e) => return CheckResult::Fail(format!("hyperliquid runtime config invalid: {e}")),
    };

    let timeout = Duration::from_millis(runtime.timeout_ms.clamp(1_000, 20_000));
    probe_http_endpoint("Hyperliquid API", &runtime.api_base_url, timeout, None).await
}

async fn check_verification_backend_reachability(
    context: &HyperliquidDoctorContext,
) -> CheckResult {
    let verification = match &context.verification {
        Ok(verification) => verification,
        Err(e) => return CheckResult::Fail(format!("verification backend config invalid: {e}")),
    };

    match verification.backend {
        crate::config::VerificationBackendKind::EigenCloudPrimary => {
            let endpoint = match verification.eigencloud.endpoint.as_deref().map(str::trim) {
                Some(endpoint) if !endpoint.is_empty() => endpoint,
                _ => {
                    return CheckResult::Fail(
                        "EIGENCLOUD_ENDPOINT is required when VERIFICATION_BACKEND=eigencloud_primary"
                            .into(),
                    );
                }
            };

            let timeout =
                Duration::from_millis(verification.eigencloud.timeout_ms.clamp(1_000, 20_000));
            let auth_header = match (
                verification.eigencloud.auth_scheme,
                verification.eigencloud.auth_token.as_deref().map(str::trim),
            ) {
                (_, Some("")) | (_, None) => None,
                (crate::config::EigenCloudAuthScheme::Bearer, Some(token)) => {
                    Some(("Authorization", format!("Bearer {token}")))
                }
                (crate::config::EigenCloudAuthScheme::ApiKey, Some(token)) => {
                    Some(("X-API-Key", token.to_string()))
                }
            };

            probe_http_endpoint("EigenCloud backend", endpoint, timeout, auth_header).await
        }
        crate::config::VerificationBackendKind::FallbackOnly => {
            check_fallback_chain_path(&verification.fallback.chain_path)
        }
    }
}

fn check_fallback_chain_path(chain_path: &std::path::Path) -> CheckResult {
    if let Some(parent) = chain_path.parent() {
        if parent.exists() && !parent.is_dir() {
            return CheckResult::Fail(format!(
                "fallback chain parent exists but is not a directory ({})",
                parent.display()
            ));
        }

        if parent.exists() {
            let writable = std::fs::metadata(parent)
                .map(|meta| !meta.permissions().readonly())
                .unwrap_or(true);

            if !writable {
                return CheckResult::Fail(format!(
                    "fallback chain parent is not writable ({})",
                    parent.display()
                ));
            }

            return CheckResult::Pass(format!(
                "fallback_only active; receipt chain path ready ({})",
                chain_path.display()
            ));
        }

        return CheckResult::Pass(format!(
            "fallback_only active; receipt chain parent will be created ({})",
            parent.display()
        ));
    }

    CheckResult::Fail(format!(
        "fallback chain path has no parent directory ({})",
        chain_path.display()
    ))
}

fn validate_wallet_credential(
    label: &str,
    value: Option<&str>,
    required: bool,
    issues: &mut Vec<String>,
    present: &mut usize,
) {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(address) => {
            if is_valid_wallet_address(address) {
                *present += 1;
            } else {
                issues.push(format!(
                    "{label} format invalid (expected 0x + 40 hex chars)"
                ));
            }
        }
        None if required => issues.push(format!("missing required {label}")),
        None => {}
    }
}

fn is_valid_wallet_address(address: &str) -> bool {
    let address = address.trim();
    if address.len() != 42 || !address.starts_with("0x") {
        return false;
    }
    address.as_bytes()[2..]
        .iter()
        .all(|b| b.is_ascii_hexdigit())
}

fn mask_wallet_address(address: &str) -> String {
    let address = address.trim();
    if address.len() < 10 {
        return address.to_string();
    }
    format!("{}...{}", &address[..6], &address[address.len() - 4..])
}

fn custody_mode_label(mode: crate::config::CustodyMode) -> &'static str {
    match mode {
        crate::config::CustodyMode::OperatorWallet => "operator_wallet",
        crate::config::CustodyMode::UserWallet => "user_wallet",
        crate::config::CustodyMode::DualMode => "dual_mode",
    }
}

async fn probe_http_endpoint(
    label: &str,
    endpoint: &str,
    timeout: Duration,
    auth_header: Option<(&'static str, String)>,
) -> CheckResult {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return CheckResult::Fail(format!("{label} endpoint is empty"));
    }

    let url = match reqwest::Url::parse(endpoint) {
        Ok(url) => url,
        Err(e) => {
            return CheckResult::Fail(format!(
                "{label} endpoint URL is invalid ({}): {e}",
                redact_url_for_display(endpoint)
            ));
        }
    };

    let client = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(client) => client,
        Err(e) => return CheckResult::Fail(format!("cannot construct HTTP client: {e}")),
    };

    let mut request = client.get(url.clone());
    if let Some((name, value)) = auth_header {
        request = request.header(name, value);
    }

    match request.send().await {
        Ok(response) => {
            let status = response.status();
            if status.is_server_error() {
                CheckResult::Fail(format!(
                    "{label} reachable but unhealthy ({} at {})",
                    status,
                    redact_url_for_display(url.as_str())
                ))
            } else {
                CheckResult::Pass(format!(
                    "{} ({status})",
                    redact_url_for_display(url.as_str())
                ))
            }
        }
        Err(e) => CheckResult::Fail(format!(
            "{label} unreachable ({}): {e}",
            redact_url_for_display(url.as_str())
        )),
    }
}

fn redact_url_for_display(raw: &str) -> String {
    match reqwest::Url::parse(raw) {
        Ok(mut url) => {
            if !url.username().is_empty() {
                let _ = url.set_username("redacted");
            }
            if url.password().is_some() {
                let _ = url.set_password(Some("redacted"));
            }
            url.to_string()
        }
        Err(_) => "<invalid-url>".to_string(),
    }
}

async fn check_nearai_session() -> CheckResult {
    let selected_backend = std::env::var("LLM_BACKEND")
        .ok()
        .and_then(|raw| crate::settings::normalize_llm_backend(&raw))
        .or_else(|| {
            load_settings_for_doctor()
                .ok()
                .and_then(|settings| settings.llm_backend)
                .and_then(|raw| crate::settings::normalize_llm_backend(&raw))
        })
        .unwrap_or_else(|| "nearai".to_string());

    if selected_backend != "nearai" {
        return CheckResult::Skip(format!(
            "LLM backend is {selected_backend}; NEAR AI session not required"
        ));
    }

    // Check if session file exists
    let session_path = crate::llm::session::default_session_path();
    if !session_path.exists() {
        // Check for API key mode
        if std::env::var("NEARAI_API_KEY").is_ok() {
            return CheckResult::Pass("API key configured".into());
        }
        return CheckResult::Fail(format!(
            "session file not found at {}. Run `enclagent onboard`",
            session_path.display()
        ));
    }

    // Verify the session file is readable and non-empty
    match std::fs::read_to_string(&session_path) {
        Ok(content) if content.trim().is_empty() => {
            CheckResult::Fail("session file is empty".into())
        }
        Ok(_) => CheckResult::Pass(format!("session found ({})", session_path.display())),
        Err(e) => CheckResult::Fail(format!("cannot read session file: {e}")),
    }
}

async fn check_database() -> CheckResult {
    let backend = std::env::var("DATABASE_BACKEND")
        .ok()
        .unwrap_or_else(|| "postgres".into());

    match backend.as_str() {
        "libsql" | "turso" | "sqlite" => {
            let path = std::env::var("LIBSQL_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| crate::config::default_libsql_path());

            if path.exists() {
                CheckResult::Pass(format!("libSQL database exists ({})", path.display()))
            } else {
                CheckResult::Pass(format!(
                    "libSQL database not found at {} (will be created on first run)",
                    path.display()
                ))
            }
        }
        _ => {
            if std::env::var("DATABASE_URL").is_ok() {
                // Try to connect
                match try_pg_connect().await {
                    Ok(()) => CheckResult::Pass("PostgreSQL connected".into()),
                    Err(e) => CheckResult::Fail(format!("PostgreSQL connection failed: {e}")),
                }
            } else {
                CheckResult::Fail("DATABASE_URL not set".into())
            }
        }
    }
}

#[cfg(feature = "postgres")]
async fn try_pg_connect() -> Result<(), String> {
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL not set".to_string())?;

    let config = deadpool_postgres::Config {
        url: Some(url),
        ..Default::default()
    };
    let pool = config
        .create_pool(
            Some(deadpool_postgres::Runtime::Tokio1),
            tokio_postgres::NoTls,
        )
        .map_err(|e| format!("pool error: {e}"))?;

    let client = tokio::time::timeout(std::time::Duration::from_secs(5), pool.get())
        .await
        .map_err(|_| "connection timeout (5s)".to_string())?
        .map_err(|e| format!("{e}"))?;

    client
        .execute("SELECT 1", &[])
        .await
        .map_err(|e| format!("{e}"))?;

    Ok(())
}

#[cfg(not(feature = "postgres"))]
async fn try_pg_connect() -> Result<(), String> {
    Err("postgres feature not compiled in".into())
}

fn check_workspace_dir() -> CheckResult {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".enclagent");

    if dir.exists() {
        if dir.is_dir() {
            CheckResult::Pass(format!("{}", dir.display()))
        } else {
            CheckResult::Fail(format!("{} exists but is not a directory", dir.display()))
        }
    } else {
        CheckResult::Pass(format!("{} will be created on first run", dir.display()))
    }
}

fn check_binary(name: &str, args: &[&str]) -> CheckResult {
    match std::process::Command::new(name)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version = version.trim();
            // Some tools print version to stderr
            let version = if version.is_empty() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                stderr.trim().lines().next().unwrap_or("").to_string()
            } else {
                version.lines().next().unwrap_or("").to_string()
            };

            if output.status.success() {
                CheckResult::Pass(version)
            } else {
                CheckResult::Fail(format!("exited with {}", output.status))
            }
        }
        Err(_) => CheckResult::Skip(format!("{name} not found in PATH")),
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::doctor::*;

    #[test]
    fn check_binary_finds_sh() {
        match check_binary("sh", &["-c", "echo ok"]) {
            CheckResult::Pass(_) => {}
            other => panic!("expected Pass for sh, got: {}", format_result(&other)),
        }
    }

    #[test]
    fn check_binary_skips_nonexistent() {
        match check_binary("__enclagent_nonexistent_binary__", &["--version"]) {
            CheckResult::Skip(_) => {}
            other => panic!(
                "expected Skip for nonexistent binary, got: {}",
                format_result(&other)
            ),
        }
    }

    #[test]
    fn check_workspace_dir_does_not_panic() {
        let result = check_workspace_dir();
        match result {
            CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
        }
    }

    #[tokio::test]
    async fn check_nearai_session_does_not_panic() {
        let result = check_nearai_session().await;
        match result {
            CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
        }
    }

    #[test]
    fn wallet_address_validation() {
        assert!(is_valid_wallet_address(
            "0x0123456789abcdef0123456789abcdef01234567"
        ));
        assert!(is_valid_wallet_address(
            "0x0123456789ABCDEF0123456789ABCDEF01234567"
        ));
        assert!(!is_valid_wallet_address("0x0123"));
        assert!(!is_valid_wallet_address(
            "0x0123456789abcdef0123456789abcdef0123456z"
        ));
        assert!(!is_valid_wallet_address(
            "0123456789abcdef0123456789abcdef01234567"
        ));
    }

    #[test]
    fn redact_url_hides_credentials() {
        let redacted = redact_url_for_display("https://user:pass@example.com/v1/check");
        assert!(redacted.contains("redacted:redacted@example.com"));
    }

    fn format_result(r: &CheckResult) -> String {
        match r {
            CheckResult::Pass(s) => format!("Pass({s})"),
            CheckResult::Fail(s) => format!("Fail({s})"),
            CheckResult::Skip(s) => format!("Skip({s})"),
        }
    }
}
