//! Main setup wizard orchestration.
//!
//! The wizard guides users through:
//! 1. Database connection
//! 2. Security (secrets master key)
//! 3. Inference provider (NEAR AI, Anthropic, OpenAI, Ollama, OpenAI-compatible)
//! 4. Model selection
//! 5. Embeddings
//! 6. Channel configuration
//! 7. Heartbeat (background tasks)
//! 8. Hyperliquid runtime profile
//! 9. Wallet and vault policy
//! 10. Verification backend

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::Arc;

#[cfg(feature = "postgres")]
use deadpool_postgres::{Config as PoolConfig, Runtime};
use rand::RngCore;
use rand::rngs::OsRng;
use secrecy::{ExposeSecret, SecretString};
#[cfg(feature = "postgres")]
use tokio_postgres::NoTls;

use crate::channels::wasm::{
    ChannelCapabilitiesFile, available_channel_names, install_bundled_channel,
};
use crate::llm::{SessionConfig, SessionManager};
use crate::secrets::{SecretsCrypto, SecretsStore};
use crate::settings::{KeySource, Settings, normalize_llm_backend};
use crate::setup::channels::{
    SecretsContext, setup_http, setup_telegram, setup_tunnel, setup_wasm_channel,
};
use crate::setup::prompts::{
    confirm, input, optional_input, print_error, print_header, print_info, print_step,
    print_success, secret_input, select_many, select_one,
};

/// Setup wizard error.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Channel setup error: {0}")]
    Channel(String),

    #[error("User cancelled")]
    Cancelled,
}

impl From<crate::setup::channels::ChannelSetupError> for SetupError {
    fn from(e: crate::setup::channels::ChannelSetupError) -> Self {
        SetupError::Channel(e.to_string())
    }
}

/// Setup wizard configuration.
#[derive(Debug, Clone, Default)]
pub struct SetupConfig {
    /// Skip authentication step (use existing session).
    pub skip_auth: bool,
    /// Only reconfigure channels.
    pub channels_only: bool,
}

const WIZARD_PROVIDERS: [&str; 5] = [
    "nearai",
    "anthropic",
    "openai",
    "ollama",
    "openai_compatible",
];

#[cfg(feature = "postgres")]
const LOCAL_DOCKER_POSTGRES_URL: &str = "postgres://enclagent:enclagent@localhost:5432/enclagent";

fn normalize_wizard_provider(raw: &str) -> Option<String> {
    let normalized = normalize_llm_backend(raw)?;
    if WIZARD_PROVIDERS.contains(&normalized.as_str()) {
        Some(normalized)
    } else {
        None
    }
}

fn provider_display_name(provider: &str) -> &str {
    match provider {
        "nearai" => "NEAR AI",
        "anthropic" => "Anthropic (Claude)",
        "openai" => "OpenAI",
        "ollama" => "Ollama (local)",
        "openai_compatible" => "OpenAI-compatible endpoint",
        _ => provider,
    }
}

/// Interactive setup wizard for Enclagent.
pub struct SetupWizard {
    config: SetupConfig,
    settings: Settings,
    session_manager: Option<Arc<SessionManager>>,
    /// Database pool (created during setup, postgres only).
    #[cfg(feature = "postgres")]
    db_pool: Option<deadpool_postgres::Pool>,
    /// libSQL backend (created during setup, libsql only).
    #[cfg(feature = "libsql")]
    db_backend: Option<crate::db::libsql::LibSqlBackend>,
    /// Secrets crypto (created during setup).
    secrets_crypto: Option<Arc<SecretsCrypto>>,
    /// Cached API key from provider setup (used by model fetcher without env mutation).
    llm_api_key: Option<SecretString>,
}

impl SetupWizard {
    /// Return the ordered onboarding step titles for the current setup mode.
    pub fn onboarding_step_titles(config: &SetupConfig) -> Vec<&'static str> {
        if config.channels_only {
            vec!["Channel Configuration"]
        } else {
            vec![
                "Database Connection",
                "Security",
                "Inference Provider",
                "Model Selection",
                "Embeddings (Semantic Search)",
                "Channel Configuration",
                "Background Tasks",
                "Hyperliquid Runtime Profile",
                "Wallet and Vault Policy",
                "Verification Backend",
            ]
        }
    }

    /// Create a new setup wizard.
    pub fn new() -> Self {
        Self {
            config: SetupConfig::default(),
            settings: Settings::default(),
            session_manager: None,
            #[cfg(feature = "postgres")]
            db_pool: None,
            #[cfg(feature = "libsql")]
            db_backend: None,
            secrets_crypto: None,
            llm_api_key: None,
        }
    }

    /// Create a wizard with custom configuration.
    pub fn with_config(config: SetupConfig) -> Self {
        Self {
            config,
            settings: Settings::default(),
            session_manager: None,
            #[cfg(feature = "postgres")]
            db_pool: None,
            #[cfg(feature = "libsql")]
            db_backend: None,
            secrets_crypto: None,
            llm_api_key: None,
        }
    }

    /// Set the session manager (for reusing existing auth).
    pub fn with_session(mut self, session: Arc<SessionManager>) -> Self {
        self.session_manager = Some(session);
        self
    }

    /// Run the setup wizard.
    pub async fn run(&mut self) -> Result<(), SetupError> {
        print_header("Enclagent Setup Wizard");
        let step_titles = Self::onboarding_step_titles(&self.config);

        if self.config.channels_only {
            // Channels-only mode: just step 6
            print_step(1, step_titles.len(), step_titles[0]);
            self.step_channels().await?;
        } else {
            let total_steps = step_titles.len();

            // Step 1: Database
            print_step(1, total_steps, step_titles[0]);
            self.step_database().await?;
            self.load_existing_settings_from_connected_db().await?;

            // Step 2: Security
            print_step(2, total_steps, step_titles[1]);
            self.step_security().await?;

            // Step 3: Inference provider selection (unless skipped)
            if !self.config.skip_auth {
                print_step(3, total_steps, step_titles[2]);
                self.step_inference_provider().await?;
            } else {
                let provider = self.require_wizard_provider("model selection")?;
                print_info(&format!(
                    "Skipping inference provider setup (using configured provider: {})",
                    provider_display_name(&provider)
                ));
            }

            // Step 4: Model selection
            print_step(4, total_steps, step_titles[3]);
            self.step_model_selection().await?;

            // Step 5: Embeddings
            print_step(5, total_steps, step_titles[4]);
            self.step_embeddings()?;

            // Step 6: Channel configuration
            print_step(6, total_steps, step_titles[5]);
            self.step_channels().await?;

            // Step 7: Heartbeat
            print_step(7, total_steps, step_titles[6]);
            self.step_heartbeat()?;

            // Step 8: Hyperliquid runtime profile
            print_step(8, total_steps, step_titles[7]);
            self.step_hyperliquid_runtime_profile()?;

            // Step 9: Wallet and vault policy
            print_step(9, total_steps, step_titles[8]);
            self.step_wallet_vault_policy()?;

            // Step 10: Verification backend
            print_step(10, total_steps, step_titles[9]);
            self.step_verification_backend()?;
        }

        // Save settings and print summary
        self.save_and_summarize().await?;

        Ok(())
    }

    /// Step 1: Database connection.
    async fn step_database(&mut self) -> Result<(), SetupError> {
        // When both features are compiled, let the user choose.
        // If DATABASE_BACKEND is already set in the environment, respect it.
        #[cfg(all(feature = "postgres", feature = "libsql"))]
        {
            // Check if a backend is already pinned via env var
            let env_backend = std::env::var("DATABASE_BACKEND").ok();

            if let Some(ref backend) = env_backend {
                if backend == "libsql" || backend == "turso" || backend == "sqlite" {
                    return self.step_database_libsql().await;
                }
                if backend != "postgres" && backend != "postgresql" {
                    print_info(&format!(
                        "Unknown DATABASE_BACKEND '{}', defaulting to PostgreSQL",
                        backend
                    ));
                }
                return self.step_database_postgres().await;
            }

            // Interactive selection
            let pre_selected = self.settings.database_backend.as_deref().map(|b| match b {
                "libsql" | "turso" | "sqlite" => 1,
                _ => 0,
            });

            print_info("Which database backend would you like to use?");
            println!();

            let options = &[
                "PostgreSQL  - production-grade, requires a running server",
                "libSQL      - embedded SQLite, zero dependencies, optional Turso cloud sync",
            ];
            let choice =
                select_one("Select a database backend:", options).map_err(SetupError::Io)?;

            // If the user picked something different from what was pre-selected, clear
            // stale connection settings so the next step starts fresh.
            if let Some(prev) = pre_selected
                && prev != choice
            {
                self.settings.database_url = None;
                self.settings.libsql_path = None;
                self.settings.libsql_url = None;
            }

            match choice {
                1 => return self.step_database_libsql().await,
                _ => return self.step_database_postgres().await,
            }
        }

        #[cfg(all(feature = "postgres", not(feature = "libsql")))]
        {
            return self.step_database_postgres().await;
        }

        #[cfg(all(feature = "libsql", not(feature = "postgres")))]
        {
            return self.step_database_libsql().await;
        }
    }

    /// Step 1 (postgres): Database connection via PostgreSQL URL.
    #[cfg(feature = "postgres")]
    async fn step_database_postgres(&mut self) -> Result<(), SetupError> {
        self.settings.database_backend = Some("postgres".to_string());

        let existing_url = std::env::var("DATABASE_URL")
            .ok()
            .or_else(|| self.settings.database_url.clone());

        if let Some(ref url) = existing_url {
            let display_url = mask_password_in_url(url);
            print_info(&format!("Existing database URL: {}", display_url));

            if confirm("Use this database?", true).map_err(SetupError::Io)? {
                if let Err(e) = self.test_database_connection_postgres(url).await {
                    print_error(&format!("Connection failed: {}", e));
                    print_info("Let's configure a new database URL.");
                } else {
                    print_success("Database connection successful");
                    self.settings.database_url = Some(url.clone());
                    return Ok(());
                }
            }
        }

        if self.try_local_docker_postgres_bootstrap().await? {
            return Ok(());
        }

        println!();
        print_info("Enter your PostgreSQL connection URL.");
        print_info("Format: postgres://user:password@host:port/database");
        println!();

        loop {
            let url = input("Database URL").map_err(SetupError::Io)?;

            if url.is_empty() {
                print_error("Database URL is required.");
                continue;
            }

            print_info("Testing connection...");
            match self.test_database_connection_postgres(&url).await {
                Ok(()) => {
                    print_success("Database connection successful");

                    if confirm("Run database migrations?", true).map_err(SetupError::Io)? {
                        self.run_migrations_postgres().await?;
                    }

                    self.settings.database_url = Some(url);
                    return Ok(());
                }
                Err(e) => {
                    print_error(&format!("Connection failed: {}", e));
                    if !confirm("Try again?", true).map_err(SetupError::Io)? {
                        return Err(SetupError::Database(
                            "Database connection failed".to_string(),
                        ));
                    }
                }
            }
        }
    }

    /// Step 1 (libsql): Database connection via local file or Turso remote replica.
    #[cfg(feature = "libsql")]
    async fn step_database_libsql(&mut self) -> Result<(), SetupError> {
        self.settings.database_backend = Some("libsql".to_string());

        let default_path = crate::config::default_libsql_path();
        let default_path_str = default_path.to_string_lossy().to_string();

        // Check for existing configuration
        let existing_path = std::env::var("LIBSQL_PATH")
            .ok()
            .or_else(|| self.settings.libsql_path.clone());

        if let Some(ref path) = existing_path {
            print_info(&format!("Existing database path: {}", path));
            if confirm("Use this database?", true).map_err(SetupError::Io)? {
                let turso_url = std::env::var("LIBSQL_URL")
                    .ok()
                    .or_else(|| self.settings.libsql_url.clone());
                let turso_token = std::env::var("LIBSQL_AUTH_TOKEN").ok();

                match self
                    .test_database_connection_libsql(
                        path,
                        turso_url.as_deref(),
                        turso_token.as_deref(),
                    )
                    .await
                {
                    Ok(()) => {
                        print_success("Database connection successful");
                        self.settings.libsql_path = Some(path.clone());
                        if let Some(url) = turso_url {
                            self.settings.libsql_url = Some(url);
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        print_error(&format!("Connection failed: {}", e));
                        print_info("Let's configure a new database path.");
                    }
                }
            }
        }

        println!();
        print_info("Enclagent uses an embedded SQLite database (libSQL).");
        print_info("No external database server required.");
        println!();

        let path_input = optional_input(
            "Database file path",
            Some(&format!("default: {}", default_path_str)),
        )
        .map_err(SetupError::Io)?;

        let db_path = path_input.unwrap_or(default_path_str.clone());

        // Ask about Turso cloud sync
        println!();
        let use_turso =
            confirm("Enable Turso cloud sync (remote replica)?", false).map_err(SetupError::Io)?;

        let (turso_url, turso_token) = if use_turso {
            print_info("Enter your Turso database URL and auth token.");
            print_info("Format: libsql://your-db.turso.io");
            println!();

            let url = input("Turso URL").map_err(SetupError::Io)?;
            if url.is_empty() {
                print_error("Turso URL is required for cloud sync.");
                (None, None)
            } else {
                let token_secret = secret_input("Auth token").map_err(SetupError::Io)?;
                let token = token_secret.expose_secret().to_string();
                if token.is_empty() {
                    print_error("Auth token is required for cloud sync.");
                    (None, None)
                } else {
                    (Some(url), Some(token))
                }
            }
        } else {
            (None, None)
        };

        print_info("Testing connection...");
        match self
            .test_database_connection_libsql(&db_path, turso_url.as_deref(), turso_token.as_deref())
            .await
        {
            Ok(()) => {
                print_success("Database connection successful");

                // Always run migrations for libsql (they're idempotent)
                self.run_migrations_libsql().await?;

                self.settings.libsql_path = Some(db_path);
                if let Some(url) = turso_url {
                    self.settings.libsql_url = Some(url);
                }
                Ok(())
            }
            Err(e) => Err(SetupError::Database(format!("Connection failed: {}", e))),
        }
    }

    /// Test PostgreSQL connection and store the pool.
    #[cfg(feature = "postgres")]
    async fn test_database_connection_postgres(&mut self, url: &str) -> Result<(), SetupError> {
        let mut cfg = PoolConfig::new();
        cfg.url = Some(url.to_string());
        cfg.pool = Some(deadpool_postgres::PoolConfig {
            max_size: 5,
            ..Default::default()
        });

        let pool = cfg
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| SetupError::Database(format!("Failed to create pool: {}", e)))?;

        let _ = pool
            .get()
            .await
            .map_err(|e| SetupError::Database(format!("Failed to connect: {}", e)))?;

        self.db_pool = Some(pool);
        Ok(())
    }

    /// Test libSQL connection and store the backend.
    #[cfg(feature = "libsql")]
    async fn test_database_connection_libsql(
        &mut self,
        path: &str,
        turso_url: Option<&str>,
        turso_token: Option<&str>,
    ) -> Result<(), SetupError> {
        use crate::db::libsql::LibSqlBackend;
        use std::path::Path;

        let db_path = Path::new(path);

        let backend = if let (Some(url), Some(token)) = (turso_url, turso_token) {
            LibSqlBackend::new_remote_replica(db_path, url, token)
                .await
                .map_err(|e| SetupError::Database(format!("Failed to connect: {}", e)))?
        } else {
            LibSqlBackend::new_local(db_path)
                .await
                .map_err(|e| SetupError::Database(format!("Failed to open database: {}", e)))?
        };

        self.db_backend = Some(backend);
        Ok(())
    }

    /// Run PostgreSQL migrations.
    #[cfg(feature = "postgres")]
    async fn run_migrations_postgres(&self) -> Result<(), SetupError> {
        if let Some(ref pool) = self.db_pool {
            use refinery::embed_migrations;
            embed_migrations!("migrations");

            print_info("Running migrations...");

            let mut client = pool
                .get()
                .await
                .map_err(|e| SetupError::Database(format!("Pool error: {}", e)))?;

            migrations::runner()
                .run_async(&mut **client)
                .await
                .map_err(|e| SetupError::Database(format!("Migration failed: {}", e)))?;

            print_success("Migrations applied");
        }
        Ok(())
    }

    /// Run libSQL migrations.
    #[cfg(feature = "libsql")]
    async fn run_migrations_libsql(&self) -> Result<(), SetupError> {
        if let Some(ref backend) = self.db_backend {
            use crate::db::Database;

            print_info("Running migrations...");

            backend
                .run_migrations()
                .await
                .map_err(|e| SetupError::Database(format!("Migration failed: {}", e)))?;

            print_success("Migrations applied");
        }
        Ok(())
    }

    /// Load existing settings from the selected database so "Keep current ..."
    /// prompts reflect persisted values instead of defaults.
    async fn load_existing_settings_from_connected_db(&mut self) -> Result<(), SetupError> {
        let selected_backend = self.settings.database_backend.clone();
        let selected_database_url = self.settings.database_url.clone();
        let selected_libsql_path = self.settings.libsql_path.clone();
        let selected_libsql_url = self.settings.libsql_url.clone();

        let mut db_map: Option<std::collections::HashMap<String, serde_json::Value>> = None;

        #[cfg(feature = "postgres")]
        if db_map.is_none()
            && let Some(ref pool) = self.db_pool
        {
            let store = crate::history::Store::from_pool(pool.clone());
            match store.get_all_settings("default").await {
                Ok(map) if !map.is_empty() => db_map = Some(map),
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Failed to load existing settings from postgres: {}", e);
                }
            }
        }

        #[cfg(feature = "libsql")]
        if db_map.is_none()
            && let Some(ref backend) = self.db_backend
        {
            use crate::db::SettingsStore as _;
            match backend.get_all_settings("default").await {
                Ok(map) if !map.is_empty() => db_map = Some(map),
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Failed to load existing settings from libsql: {}", e);
                }
            }
        }

        if let Some(map) = db_map {
            let mut loaded = Settings::from_db_map(&map);

            // Preserve Step 1 bootstrap selections as authoritative for this run.
            loaded.database_backend = selected_backend;
            loaded.database_url = selected_database_url;
            loaded.libsql_path = selected_libsql_path;
            loaded.libsql_url = selected_libsql_url;

            self.settings = loaded;
            print_info("Loaded existing settings from database.");
        }

        Ok(())
    }

    /// Offer to spin up a local PostgreSQL instance in Docker for zero-friction setup.
    #[cfg(feature = "postgres")]
    async fn try_local_docker_postgres_bootstrap(&mut self) -> Result<bool, SetupError> {
        println!();
        if !confirm("Start local PostgreSQL in Docker for this setup?", true)
            .map_err(SetupError::Io)?
        {
            return Ok(false);
        }

        print_info("Starting local PostgreSQL via Docker...");
        let start_msg = match start_local_postgres_via_docker() {
            Ok(message) => message,
            Err(err) => {
                print_error(&format!("Could not start Docker PostgreSQL: {}", err));
                return Ok(false);
            }
        };
        print_success(&start_msg);

        let display_url = mask_password_in_url(LOCAL_DOCKER_POSTGRES_URL);
        print_info(&format!(
            "Waiting for database readiness at {}...",
            display_url
        ));

        let mut last_error: Option<String> = None;
        for _ in 0..30 {
            match self
                .test_database_connection_postgres(LOCAL_DOCKER_POSTGRES_URL)
                .await
            {
                Ok(()) => {
                    print_success("Database connection successful");
                    self.run_migrations_postgres().await?;
                    self.settings.database_url = Some(LOCAL_DOCKER_POSTGRES_URL.to_string());
                    print_success("Local Docker PostgreSQL configured");
                    return Ok(true);
                }
                Err(err) => {
                    last_error = Some(err.to_string());
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }

        if let Some(err) = last_error {
            print_error(&format!(
                "Docker PostgreSQL started but did not become ready in time: {}",
                err
            ));
        } else {
            print_error("Docker PostgreSQL started but did not become ready in time.");
        }

        Ok(false)
    }

    /// Step 2: Security (secrets master key).
    async fn step_security(&mut self) -> Result<(), SetupError> {
        // Check current configuration
        let env_key_exists = std::env::var("SECRETS_MASTER_KEY").is_ok();

        if env_key_exists {
            print_info("Secrets master key found in SECRETS_MASTER_KEY environment variable.");
            self.settings.secrets_master_key_source = KeySource::Env;
            print_success("Security configured (env var)");
            return Ok(());
        }

        // Try to retrieve existing key from keychain. We use get_master_key()
        // instead of has_master_key() so we can cache the key bytes and build
        // SecretsCrypto eagerly, avoiding redundant keychain accesses later
        // (each access triggers macOS system dialogs).
        print_info("Checking OS keychain for existing master key...");
        if let Ok(keychain_key_bytes) = crate::secrets::keychain::get_master_key().await {
            let key_hex: String = keychain_key_bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();
            self.secrets_crypto = Some(Arc::new(
                SecretsCrypto::new(SecretString::from(key_hex))
                    .map_err(|e| SetupError::Config(e.to_string()))?,
            ));

            print_info("Existing master key found in OS keychain.");
            if confirm("Use existing keychain key?", true).map_err(SetupError::Io)? {
                self.settings.secrets_master_key_source = KeySource::Keychain;
                print_success("Security configured (keychain)");
                return Ok(());
            }
            // User declined the existing key; clear the cached crypto so a fresh
            // key can be generated below.
            self.secrets_crypto = None;
        }

        // Offer options
        println!();
        print_info("The secrets master key encrypts sensitive data like API tokens.");
        print_info("Choose where to store it:");
        println!();

        let options = [
            "OS Keychain (recommended for local installs)",
            "Environment variable (for CI/Docker)",
            "Skip (disable secrets features)",
        ];

        let choice = select_one("Select storage method:", &options).map_err(SetupError::Io)?;

        match choice {
            0 => {
                // Generate and store in keychain
                print_info("Generating master key...");
                let key = crate::secrets::keychain::generate_master_key();

                crate::secrets::keychain::store_master_key(&key)
                    .await
                    .map_err(|e| {
                        SetupError::Config(format!("Failed to store in keychain: {}", e))
                    })?;

                // Also create crypto instance
                let key_hex: String = key.iter().map(|b| format!("{:02x}", b)).collect();
                self.secrets_crypto = Some(Arc::new(
                    SecretsCrypto::new(SecretString::from(key_hex))
                        .map_err(|e| SetupError::Config(e.to_string()))?,
                ));

                self.settings.secrets_master_key_source = KeySource::Keychain;
                print_success("Master key generated and stored in OS keychain");
            }
            1 => {
                // Env var mode
                print_info("Generate a key and add it to your environment:");
                let key_hex = crate::secrets::keychain::generate_master_key_hex();
                println!();
                println!("  export SECRETS_MASTER_KEY={}", key_hex);
                println!();
                print_info("Add this to your shell profile or .env file.");

                self.settings.secrets_master_key_source = KeySource::Env;
                print_success("Configured for environment variable");
            }
            _ => {
                self.settings.secrets_master_key_source = KeySource::None;
                print_info("Secrets features disabled. Channel tokens must be set via env vars.");
            }
        }

        Ok(())
    }

    fn set_llm_backend(&mut self, backend: &str) {
        let changed = self.settings.llm_backend.as_deref() != Some(backend);
        self.settings.llm_backend = Some(backend.to_string());
        if changed {
            self.settings.selected_model = None;
        }
    }

    fn configured_wizard_provider(&mut self) -> Result<Option<String>, SetupError> {
        if let Some(current) = self.settings.llm_backend.clone() {
            if let Some(normalized) = normalize_wizard_provider(&current) {
                if self.settings.llm_backend.as_deref() != Some(normalized.as_str()) {
                    self.settings.llm_backend = Some(normalized.clone());
                }
                return Ok(Some(normalized));
            }
            return Err(SetupError::Config(format!(
                "Configured provider '{}' is unsupported. Supported providers: NEAR AI, Anthropic, OpenAI, Ollama, OpenAI-compatible.",
                current
            )));
        }

        if let Ok(env_backend) = std::env::var("LLM_BACKEND") {
            if let Some(normalized) = normalize_wizard_provider(&env_backend) {
                self.settings.llm_backend = Some(normalized.clone());
                return Ok(Some(normalized));
            }
            return Err(SetupError::Config(format!(
                "LLM_BACKEND='{}' is unsupported. Supported providers: NEAR AI, Anthropic, OpenAI, Ollama, OpenAI-compatible.",
                env_backend
            )));
        }

        Ok(None)
    }

    fn require_wizard_provider(&mut self, context: &str) -> Result<String, SetupError> {
        match self.configured_wizard_provider()? {
            Some(provider) => Ok(provider),
            None => Err(SetupError::Config(format!(
                "Inference provider is required before {}. Select one in Step 3 (Inference Provider).",
                context
            ))),
        }
    }

    /// Step 3: Inference provider selection.
    ///
    /// Lets the user pick from all supported LLM backends, then runs the
    /// provider-specific auth sub-flow (API key entry, NEAR AI login, etc.).
    async fn step_inference_provider(&mut self) -> Result<(), SetupError> {
        match self.configured_wizard_provider() {
            Ok(Some(current)) => {
                print_info(&format!(
                    "Current provider: {}",
                    provider_display_name(&current)
                ));
                println!();

                if confirm("Keep current provider?", true).map_err(SetupError::Io)? {
                    // Still run the auth sub-flow in case they need to update keys
                    match current.as_str() {
                        "nearai" => return self.setup_nearai().await,
                        "anthropic" => return self.setup_anthropic().await,
                        "openai" => return self.setup_openai().await,
                        "ollama" => return self.setup_ollama(),
                        "openai_compatible" => return self.setup_openai_compatible().await,
                        _ => {
                            return Err(SetupError::Config(format!(
                                "Unhandled provider: {}",
                                current
                            )));
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                print_info(&format!("{}", e));
                print_info("Please select a supported provider.");
                self.settings.llm_backend = None;
            }
        }

        print_info("Select your inference provider:");
        println!();

        let options = &[
            "NEAR AI          - multi-model access via NEAR account",
            "Anthropic        - Claude models (direct API key)",
            "OpenAI           - GPT models (direct API key)",
            "Ollama           - local models, no API key needed",
            "OpenAI-compatible - custom endpoint (vLLM, LiteLLM, Together, etc.)",
        ];

        let choice = select_one("Provider:", options).map_err(SetupError::Io)?;

        match choice {
            0 => self.setup_nearai().await?,
            1 => self.setup_anthropic().await?,
            2 => self.setup_openai().await?,
            3 => self.setup_ollama()?,
            4 => self.setup_openai_compatible().await?,
            _ => return Err(SetupError::Config("Invalid provider selection".to_string())),
        }

        Ok(())
    }

    /// NEAR AI provider setup (extracted from the old step_authentication).
    async fn setup_nearai(&mut self) -> Result<(), SetupError> {
        self.set_llm_backend("nearai");

        // Check if we already have a session
        if let Some(ref session) = self.session_manager
            && session.has_token().await
        {
            print_info("Existing session found. Validating...");
            match session.ensure_authenticated().await {
                Ok(()) => {
                    print_success("NEAR AI session valid");
                    return Ok(());
                }
                Err(e) => {
                    print_info(&format!("Session invalid: {}. Re-authenticating...", e));
                }
            }
        }

        // Create session manager if we don't have one
        let session = if let Some(ref s) = self.session_manager {
            Arc::clone(s)
        } else {
            let config = SessionConfig::default();
            Arc::new(SessionManager::new(config))
        };

        // Trigger authentication flow
        session
            .ensure_authenticated()
            .await
            .map_err(|e| SetupError::Auth(e.to_string()))?;

        self.session_manager = Some(session);
        print_success("NEAR AI configured");
        Ok(())
    }

    /// Anthropic provider setup: collect API key and store in secrets.
    async fn setup_anthropic(&mut self) -> Result<(), SetupError> {
        self.setup_api_key_provider(
            "anthropic",
            "ANTHROPIC_API_KEY",
            "llm_anthropic_api_key",
            "Anthropic API key",
            "https://console.anthropic.com/settings/keys",
        )
        .await
    }

    /// OpenAI provider setup: collect API key and store in secrets.
    async fn setup_openai(&mut self) -> Result<(), SetupError> {
        self.setup_api_key_provider(
            "openai",
            "OPENAI_API_KEY",
            "llm_openai_api_key",
            "OpenAI API key",
            "https://platform.openai.com/api-keys",
        )
        .await
    }

    /// Shared setup flow for API-key-based providers (Anthropic, OpenAI).
    async fn setup_api_key_provider(
        &mut self,
        backend: &str,
        env_var: &str,
        secret_name: &str,
        prompt_label: &str,
        hint_url: &str,
    ) -> Result<(), SetupError> {
        let display_name = match backend {
            "anthropic" => "Anthropic",
            "openai" => "OpenAI",
            other => other,
        };

        self.set_llm_backend(backend);

        // Check env var first
        if let Ok(existing) = std::env::var(env_var) {
            print_info(&format!("{env_var} found: {}", mask_api_key(&existing)));
            if confirm("Use this key?", true).map_err(SetupError::Io)? {
                // Persist env-provided key to secrets store for future runs
                if let Ok(ctx) = self.init_secrets_context().await {
                    let key = SecretString::from(existing.clone());
                    if let Err(e) = ctx.save_secret(secret_name, &key).await {
                        tracing::warn!("Failed to persist env key to secrets: {}", e);
                    }
                }
                self.llm_api_key = Some(SecretString::from(existing));
                print_success(&format!("{display_name} configured (from env)"));
                return Ok(());
            }
        }

        println!();
        print_info(&format!("Get your API key from: {hint_url}"));
        println!();

        let key = secret_input(prompt_label).map_err(SetupError::Io)?;
        let key_str = key.expose_secret();

        if key_str.is_empty() {
            return Err(SetupError::Config("API key cannot be empty".to_string()));
        }

        // Store in secrets if available
        if let Ok(ctx) = self.init_secrets_context().await {
            ctx.save_secret(secret_name, &key)
                .await
                .map_err(|e| SetupError::Config(format!("Failed to save API key: {e}")))?;
            print_success("API key encrypted and saved");
        } else {
            print_info(&format!(
                "Secrets not available. Set {env_var} in your environment."
            ));
        }

        // Cache key in memory for model fetching later in the wizard
        self.llm_api_key = Some(SecretString::from(key_str.to_string()));

        print_success(&format!("{display_name} configured"));
        Ok(())
    }

    /// Ollama provider setup: just needs a base URL, no API key.
    fn setup_ollama(&mut self) -> Result<(), SetupError> {
        self.set_llm_backend("ollama");

        let default_url = self
            .settings
            .ollama_base_url
            .as_deref()
            .unwrap_or("http://localhost:11434");

        let url_input = optional_input(
            "Ollama base URL",
            Some(&format!("default: {}", default_url)),
        )
        .map_err(SetupError::Io)?;

        let url = url_input.unwrap_or_else(|| default_url.to_string());
        self.settings.ollama_base_url = Some(url.clone());

        print_success(&format!("Ollama configured ({})", url));
        Ok(())
    }

    /// OpenAI-compatible provider setup: base URL + optional API key.
    async fn setup_openai_compatible(&mut self) -> Result<(), SetupError> {
        self.set_llm_backend("openai_compatible");

        let existing_url = self
            .settings
            .openai_compatible_base_url
            .clone()
            .or_else(|| std::env::var("LLM_BASE_URL").ok());

        let url = if let Some(ref u) = existing_url {
            let url_input = optional_input("Base URL", Some(&format!("current: {}", u)))
                .map_err(SetupError::Io)?;
            url_input.unwrap_or_else(|| u.clone())
        } else {
            input("Base URL (e.g., http://localhost:8000/v1)").map_err(SetupError::Io)?
        };

        if url.is_empty() {
            return Err(SetupError::Config(
                "Base URL is required for OpenAI-compatible provider".to_string(),
            ));
        }

        self.settings.openai_compatible_base_url = Some(url.clone());

        // Optional API key
        if confirm("Does this endpoint require an API key?", false).map_err(SetupError::Io)? {
            let key = secret_input("API key").map_err(SetupError::Io)?;
            let key_str = key.expose_secret();

            if !key_str.is_empty() {
                if let Ok(ctx) = self.init_secrets_context().await {
                    ctx.save_secret("llm_compatible_api_key", &key)
                        .await
                        .map_err(|e| {
                            SetupError::Config(format!("Failed to save API key: {}", e))
                        })?;
                    print_success("API key encrypted and saved");
                } else {
                    print_info("Secrets not available. Set LLM_API_KEY in your environment.");
                }
            }
        }

        print_success(&format!("OpenAI-compatible configured ({})", url));
        Ok(())
    }

    /// Step 4: Model selection.
    ///
    /// Branches on the selected LLM backend and fetches models from the
    /// appropriate provider API, with static defaults as fallback.
    async fn step_model_selection(&mut self) -> Result<(), SetupError> {
        // Show current model if already configured
        if let Some(ref current) = self.settings.selected_model {
            print_info(&format!("Current model: {}", current));
            println!();

            let options = ["Keep current model", "Change model"];
            let choice =
                select_one("What would you like to do?", &options).map_err(SetupError::Io)?;

            if choice == 0 {
                print_success(&format!("Keeping {}", current));
                return Ok(());
            }
        }

        let backend = self.require_wizard_provider("model selection")?;

        match backend.as_str() {
            "anthropic" => {
                let cached = self
                    .llm_api_key
                    .as_ref()
                    .map(|k| k.expose_secret().to_string());
                let models = fetch_anthropic_models(cached.as_deref()).await;
                self.select_from_model_list(&models)?;
            }
            "openai" => {
                let cached = self
                    .llm_api_key
                    .as_ref()
                    .map(|k| k.expose_secret().to_string());
                let models = fetch_openai_models(cached.as_deref()).await;
                self.select_from_model_list(&models)?;
            }
            "ollama" => {
                let base_url = self
                    .settings
                    .ollama_base_url
                    .as_deref()
                    .unwrap_or("http://localhost:11434");
                let models = fetch_ollama_models(base_url).await;
                if models.is_empty() {
                    print_info("No models found. Pull one first: ollama pull llama3");
                }
                self.select_from_model_list(&models)?;
            }
            "openai_compatible" => {
                // No standard API for listing models on arbitrary endpoints
                let model_id = input("Model name (e.g., meta-llama/Llama-3-8b-chat-hf)")
                    .map_err(SetupError::Io)?;
                if model_id.is_empty() {
                    return Err(SetupError::Config("Model name is required".to_string()));
                }
                self.settings.selected_model = Some(model_id.clone());
                print_success(&format!("Selected {}", model_id));
            }
            _ => {
                // NEAR AI: use existing provider list_models()
                let fetched = self.fetch_nearai_models().await;
                let default_models: Vec<(String, String)> = vec![
                    (
                        "fireworks::accounts/fireworks/models/llama4-maverick-instruct-basic"
                            .into(),
                        "Llama 4 Maverick (default, fast)".into(),
                    ),
                    (
                        "anthropic::claude-sonnet-4-20250514".into(),
                        "Claude Sonnet 4 (best quality)".into(),
                    ),
                    ("openai::gpt-4o".into(), "GPT-4o".into()),
                ];

                let models = if fetched.is_empty() {
                    default_models
                } else {
                    fetched.iter().map(|m| (m.clone(), m.clone())).collect()
                };
                self.select_from_model_list(&models)?;
            }
        }

        Ok(())
    }

    /// Present a model list to the user, with a "Custom model ID" escape hatch.
    ///
    /// Each entry is `(model_id, display_label)`.
    fn select_from_model_list(&mut self, models: &[(String, String)]) -> Result<(), SetupError> {
        println!("Available models:");
        println!();

        let mut options: Vec<&str> = models.iter().map(|(_, desc)| desc.as_str()).collect();
        options.push("Custom model ID");

        let choice = select_one("Select a model:", &options).map_err(SetupError::Io)?;

        let selected = if choice == options.len() - 1 {
            loop {
                let raw = input("Enter model ID").map_err(SetupError::Io)?;
                let trimmed = raw.trim().to_string();
                if trimmed.is_empty() {
                    println!("Model ID cannot be empty.");
                    continue;
                }
                break trimmed;
            }
        } else {
            models[choice].0.clone()
        };

        self.settings.selected_model = Some(selected.clone());
        print_success(&format!("Selected {}", selected));
        Ok(())
    }

    /// Fetch available models from the NEAR AI API.
    async fn fetch_nearai_models(&self) -> Vec<String> {
        let session = match self.session_manager {
            Some(ref s) => Arc::clone(s),
            None => return vec![],
        };

        use crate::config::LlmConfig;
        use crate::llm::create_llm_provider;

        let base_url = std::env::var("NEARAI_BASE_URL")
            .unwrap_or_else(|_| "https://private.near.ai".to_string());
        let auth_base_url = std::env::var("NEARAI_AUTH_URL")
            .unwrap_or_else(|_| "https://private.near.ai".to_string());

        let config = LlmConfig {
            backend: crate::config::LlmBackend::NearAi,
            nearai: crate::config::NearAiConfig {
                model: "dummy".to_string(),
                cheap_model: None,
                base_url,
                auth_base_url,
                session_path: crate::llm::session::default_session_path(),
                api_mode: crate::config::NearAiApiMode::Responses,
                api_key: None,
                fallback_model: None,
                max_retries: 3,
                circuit_breaker_threshold: None,
                circuit_breaker_recovery_secs: 30,
                response_cache_enabled: false,
                response_cache_ttl_secs: 3600,
                response_cache_max_entries: 1000,
                failover_cooldown_secs: 300,
                failover_cooldown_threshold: 3,
            },
            openai: None,
            anthropic: None,
            ollama: None,
            openai_compatible: None,
            tinfoil: None,
        };

        match create_llm_provider(&config, session) {
            Ok(provider) => match provider.list_models().await {
                Ok(models) => models,
                Err(e) => {
                    print_info(&format!("Could not fetch models: {}. Using defaults.", e));
                    vec![]
                }
            },
            Err(e) => {
                print_info(&format!(
                    "Could not initialize provider: {}. Using defaults.",
                    e
                ));
                vec![]
            }
        }
    }

    /// Step 5: Embeddings configuration.
    fn step_embeddings(&mut self) -> Result<(), SetupError> {
        print_info("Embeddings enable semantic search in your workspace memory.");
        println!();

        if !confirm("Enable semantic search?", true).map_err(SetupError::Io)? {
            self.settings.embeddings.enabled = false;
            print_info("Embeddings disabled. Workspace will use keyword search only.");
            return Ok(());
        }

        let backend = self.require_wizard_provider("embeddings setup")?;
        let has_openai_key = std::env::var("OPENAI_API_KEY").is_ok()
            || (backend == "openai" && self.llm_api_key.is_some());
        let has_nearai = backend == "nearai" || self.session_manager.is_some();

        // If the LLM backend is OpenAI and we already have a key, default to OpenAI embeddings
        if backend == "openai" && has_openai_key {
            self.settings.embeddings.enabled = true;
            self.settings.embeddings.provider = "openai".to_string();
            self.settings.embeddings.model = "text-embedding-3-small".to_string();
            print_success("Embeddings enabled via OpenAI (using existing API key)");
            return Ok(());
        }

        // If no NEAR AI session and no OpenAI key, only OpenAI is viable
        if !has_nearai && !has_openai_key {
            print_info("No NEAR AI session or OpenAI key found for embeddings.");
            print_info("Set OPENAI_API_KEY in your environment to enable embeddings.");
            self.settings.embeddings.enabled = false;
            return Ok(());
        }

        let mut options = Vec::new();
        if has_nearai {
            options.push("NEAR AI (uses same auth, no extra cost)");
        }
        options.push("OpenAI (requires API key)");

        let choice = select_one("Select embeddings provider:", &options).map_err(SetupError::Io)?;

        // Map choice back to provider name
        let provider = if has_nearai && choice == 0 {
            "nearai"
        } else {
            "openai"
        };

        match provider {
            "nearai" => {
                self.settings.embeddings.enabled = true;
                self.settings.embeddings.provider = "nearai".to_string();
                self.settings.embeddings.model = "text-embedding-3-small".to_string();
                print_success("Embeddings enabled via NEAR AI");
            }
            _ => {
                if !has_openai_key {
                    print_info("OPENAI_API_KEY not set in environment.");
                    print_info("Add it to your .env file or environment to enable embeddings.");
                }
                self.settings.embeddings.enabled = true;
                self.settings.embeddings.provider = "openai".to_string();
                self.settings.embeddings.model = "text-embedding-3-small".to_string();
                print_success("Embeddings configured for OpenAI");
            }
        }

        Ok(())
    }

    /// Initialize secrets context for channel setup.
    async fn init_secrets_context(&mut self) -> Result<SecretsContext, SetupError> {
        // Get crypto (should be set from step 2, or load from keychain/env)
        let crypto = if let Some(ref c) = self.secrets_crypto {
            Arc::clone(c)
        } else {
            // Try to load master key from keychain or env
            let key = if let Ok(env_key) = std::env::var("SECRETS_MASTER_KEY") {
                env_key
            } else if let Ok(keychain_key) = crate::secrets::keychain::get_master_key().await {
                keychain_key.iter().map(|b| format!("{:02x}", b)).collect()
            } else {
                return Err(SetupError::Config(
                    "Secrets not configured. Run full setup or set SECRETS_MASTER_KEY.".to_string(),
                ));
            };

            let crypto = Arc::new(
                SecretsCrypto::new(SecretString::from(key))
                    .map_err(|e| SetupError::Config(e.to_string()))?,
            );
            self.secrets_crypto = Some(Arc::clone(&crypto));
            crypto
        };

        // Create backend-appropriate secrets store.
        // Respect the user's selected backend when both features are compiled,
        // so we don't accidentally use a postgres pool from DATABASE_URL when
        // libsql was chosen (or vice versa).
        let selected_backend = self
            .settings
            .database_backend
            .as_deref()
            .unwrap_or("postgres");

        #[cfg(all(feature = "libsql", feature = "postgres"))]
        {
            if selected_backend == "libsql" {
                if let Some(store) = self.create_libsql_secrets_store(&crypto)? {
                    return Ok(SecretsContext::from_store(store, "default"));
                }
                if let Some(store) = self.create_postgres_secrets_store(&crypto).await? {
                    return Ok(SecretsContext::from_store(store, "default"));
                }
            } else {
                if let Some(store) = self.create_postgres_secrets_store(&crypto).await? {
                    return Ok(SecretsContext::from_store(store, "default"));
                }
                if let Some(store) = self.create_libsql_secrets_store(&crypto)? {
                    return Ok(SecretsContext::from_store(store, "default"));
                }
            }
        }

        #[cfg(all(feature = "postgres", not(feature = "libsql")))]
        {
            let _ = selected_backend;
            if let Some(store) = self.create_postgres_secrets_store(&crypto).await? {
                return Ok(SecretsContext::from_store(store, "default"));
            }
        }

        #[cfg(all(feature = "libsql", not(feature = "postgres")))]
        {
            let _ = selected_backend;
            if let Some(store) = self.create_libsql_secrets_store(&crypto)? {
                return Ok(SecretsContext::from_store(store, "default"));
            }
        }

        Err(SetupError::Config(
            "No database backend available for secrets storage".to_string(),
        ))
    }

    /// Create a PostgreSQL secrets store from the current pool.
    #[cfg(feature = "postgres")]
    async fn create_postgres_secrets_store(
        &mut self,
        crypto: &Arc<SecretsCrypto>,
    ) -> Result<Option<Arc<dyn SecretsStore>>, SetupError> {
        let pool = if let Some(ref p) = self.db_pool {
            p.clone()
        } else {
            // Fall back to creating one from settings/env
            let url = self
                .settings
                .database_url
                .clone()
                .or_else(|| std::env::var("DATABASE_URL").ok());

            if let Some(url) = url {
                self.test_database_connection_postgres(&url).await?;
                self.run_migrations_postgres().await?;
                match self.db_pool.clone() {
                    Some(pool) => pool,
                    None => {
                        return Err(SetupError::Database(
                            "Database pool not initialized after connection test".to_string(),
                        ));
                    }
                }
            } else {
                return Ok(None);
            }
        };

        let store: Arc<dyn SecretsStore> = Arc::new(crate::secrets::PostgresSecretsStore::new(
            pool,
            Arc::clone(crypto),
        ));
        Ok(Some(store))
    }

    /// Create a libSQL secrets store from the current backend.
    #[cfg(feature = "libsql")]
    fn create_libsql_secrets_store(
        &self,
        crypto: &Arc<SecretsCrypto>,
    ) -> Result<Option<Arc<dyn SecretsStore>>, SetupError> {
        if let Some(ref backend) = self.db_backend {
            let store: Arc<dyn SecretsStore> = Arc::new(crate::secrets::LibSqlSecretsStore::new(
                backend.shared_db(),
                Arc::clone(crypto),
            ));
            Ok(Some(store))
        } else {
            Ok(None)
        }
    }

    /// Step 6: Channel configuration.
    async fn step_channels(&mut self) -> Result<(), SetupError> {
        // First, configure tunnel (shared across all channels that need webhooks)
        match setup_tunnel(&self.settings) {
            Ok(tunnel_settings) => {
                self.settings.tunnel = tunnel_settings;
            }
            Err(e) => {
                print_info(&format!("Tunnel setup skipped: {}", e));
            }
        }
        println!();

        // Discover available WASM channels
        let channels_dir = dirs::home_dir()
            .ok_or_else(|| SetupError::Config("Could not determine home directory".into()))?
            .join(".enclagent/channels");

        let mut discovered_channels = discover_wasm_channels(&channels_dir).await;
        let installed_names: HashSet<String> = discovered_channels
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let wasm_channel_names = wasm_channel_option_names(&discovered_channels);

        // Build options list dynamically
        let mut options: Vec<(String, bool)> = vec![
            ("CLI/TUI (always enabled)".to_string(), true),
            (
                "HTTP webhook".to_string(),
                self.settings.channels.http_enabled,
            ),
        ];

        // Add available WASM channels (installed + bundled)
        for name in &wasm_channel_names {
            let is_enabled = self.settings.channels.wasm_channels.contains(name);
            let display_name = format!("{} (WASM)", capitalize_first(name));
            options.push((display_name, is_enabled));
        }

        let options_refs: Vec<(&str, bool)> =
            options.iter().map(|(s, b)| (s.as_str(), *b)).collect();

        let selected = select_many("Which channels do you want to enable?", &options_refs)
            .map_err(SetupError::Io)?;

        let selected_wasm_channels: Vec<String> = wasm_channel_names
            .iter()
            .enumerate()
            .filter_map(|(idx, name)| {
                if selected.contains(&(idx + 2)) {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();

        if let Some(installed) = install_selected_bundled_channels(
            &channels_dir,
            &selected_wasm_channels,
            &installed_names,
        )
        .await?
            && !installed.is_empty()
        {
            print_success(&format!("Installed channels: {}", installed.join(", ")));
            discovered_channels = discover_wasm_channels(&channels_dir).await;
        }

        // Determine if we need secrets context
        let needs_secrets = selected.contains(&1) || !selected_wasm_channels.is_empty();
        let secrets = if needs_secrets {
            match self.init_secrets_context().await {
                Ok(ctx) => Some(ctx),
                Err(e) => {
                    print_info(&format!("Secrets not available: {}", e));
                    print_info("Channel tokens must be set via environment variables.");
                    None
                }
            }
        } else {
            None
        };

        // HTTP is index 1
        if selected.contains(&1) {
            println!();
            if let Some(ref ctx) = secrets {
                let result = setup_http(ctx).await?;
                self.settings.channels.http_enabled = result.enabled;
                self.settings.channels.http_port = Some(result.port);
            } else {
                self.settings.channels.http_enabled = true;
                self.settings.channels.http_port = Some(8080);
                print_info("HTTP webhook enabled on port 8080 (set HTTP_WEBHOOK_SECRET in env)");
            }
        } else {
            self.settings.channels.http_enabled = false;
        }

        let discovered_by_name: HashMap<String, ChannelCapabilitiesFile> =
            discovered_channels.into_iter().collect();

        // Process selected WASM channels
        let mut enabled_wasm_channels = Vec::new();
        for channel_name in selected_wasm_channels {
            println!();
            if let Some(ref ctx) = secrets {
                let result = if let Some(cap_file) = discovered_by_name.get(&channel_name) {
                    if !cap_file.setup.required_secrets.is_empty() {
                        setup_wasm_channel(ctx, &channel_name, &cap_file.setup).await?
                    } else if channel_name == "telegram" {
                        let telegram_result = setup_telegram(ctx, &self.settings).await?;
                        if let Some(owner_id) = telegram_result.owner_id {
                            self.settings.channels.telegram_owner_id = Some(owner_id);
                        }
                        crate::setup::channels::WasmChannelSetupResult {
                            enabled: telegram_result.enabled,
                            channel_name: "telegram".to_string(),
                        }
                    } else {
                        print_info(&format!(
                            "No setup configuration found for {}",
                            channel_name
                        ));
                        crate::setup::channels::WasmChannelSetupResult {
                            enabled: true,
                            channel_name: channel_name.clone(),
                        }
                    }
                } else {
                    print_info(&format!(
                        "Channel '{}' is selected but not available on disk.",
                        channel_name
                    ));
                    continue;
                };

                if result.enabled {
                    enabled_wasm_channels.push(result.channel_name);
                }
            } else {
                // No secrets context, just enable the channel
                print_info(&format!(
                    "{} enabled (configure tokens via environment)",
                    capitalize_first(&channel_name)
                ));
                enabled_wasm_channels.push(channel_name.clone());
            }
        }

        self.settings.channels.wasm_channels = enabled_wasm_channels;

        Ok(())
    }

    /// Step 7: Heartbeat configuration.
    fn step_heartbeat(&mut self) -> Result<(), SetupError> {
        print_info("Heartbeat runs periodic background tasks (e.g., checking your calendar,");
        print_info("monitoring for notifications, running scheduled workflows).");
        println!();

        if !confirm("Enable heartbeat?", false).map_err(SetupError::Io)? {
            self.settings.heartbeat.enabled = false;
            print_info("Heartbeat disabled.");
            return Ok(());
        }

        self.settings.heartbeat.enabled = true;

        // Interval
        let interval_str = optional_input("Check interval in minutes", Some("default: 30"))
            .map_err(SetupError::Io)?;

        if let Some(s) = interval_str {
            if let Ok(mins) = s.parse::<u64>() {
                self.settings.heartbeat.interval_secs = mins * 60;
            }
        } else {
            self.settings.heartbeat.interval_secs = 1800; // 30 minutes
        }

        // Notify channel
        let notify_channel = optional_input("Notify channel on findings", Some("e.g., telegram"))
            .map_err(SetupError::Io)?;
        self.settings.heartbeat.notify_channel = notify_channel;

        print_success(&format!(
            "Heartbeat enabled (every {} minutes)",
            self.settings.heartbeat.interval_secs / 60
        ));

        Ok(())
    }

    /// Step 8: Hyperliquid runtime profile configuration.
    fn step_hyperliquid_runtime_profile(&mut self) -> Result<(), SetupError> {
        print_info("Configure Hyperliquid network/runtime behavior.");
        print_info("Defaults are conservative: testnet + paper-first policy.");
        println!();

        let current = &self.settings.hyperliquid_runtime;
        let current_network = normalize_hyperliquid_network(&current.network)
            .unwrap_or_else(|| "testnet".to_string());
        let current_policy = normalize_paper_live_policy(&current.paper_live_policy)
            .unwrap_or_else(|| "paper_first".to_string());

        print_info(&format!(
            "Current profile: network={}, policy={}, timeout={}ms",
            current_network, current_policy, current.timeout_ms
        ));
        println!();

        if confirm("Keep current Hyperliquid runtime profile?", true).map_err(SetupError::Io)? {
            print_success("Keeping existing Hyperliquid runtime profile");
            self.step_copytrading_profile()?;
            return Ok(());
        }

        let mut network = current_network;
        loop {
            let raw = optional_input(
                "Network (testnet/mainnet)",
                Some(&format!("default: {}", network)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            match normalize_hyperliquid_network(&raw) {
                Some(normalized) => {
                    network = normalized;
                    break;
                }
                None => print_error("Invalid network. Expected: testnet or mainnet."),
            }
        }

        let default_api_url = default_hyperliquid_api_base_url(&network).to_string();
        let default_ws_url = default_hyperliquid_ws_url(&network).to_string();

        let mut api_base_url = current.api_base_url.clone();
        let mut ws_url = current.ws_url.clone();

        let use_network_defaults = confirm(
            "Use network default API and WebSocket endpoints?",
            api_base_url.is_none() && ws_url.is_none(),
        )
        .map_err(SetupError::Io)?;

        if use_network_defaults {
            api_base_url = None;
            ws_url = None;
        } else {
            let api_default = api_base_url
                .as_deref()
                .unwrap_or(default_api_url.as_str())
                .to_string();
            loop {
                let raw = optional_input(
                    "API base URL override (http/https)",
                    Some(&format!("default: {}", api_default)),
                )
                .map_err(SetupError::Io)?;
                let candidate = raw.unwrap_or_else(|| api_default.clone());
                if !looks_like_http_url(&candidate) {
                    print_error("Invalid API URL. Expected URL starting with http:// or https://.");
                    continue;
                }
                api_base_url = if candidate == default_api_url {
                    None
                } else {
                    Some(candidate)
                };
                break;
            }

            let ws_default = ws_url
                .as_deref()
                .unwrap_or(default_ws_url.as_str())
                .to_string();
            loop {
                let raw = optional_input(
                    "WebSocket URL override (ws/wss)",
                    Some(&format!("default: {}", ws_default)),
                )
                .map_err(SetupError::Io)?;
                let candidate = raw.unwrap_or_else(|| ws_default.clone());
                if !looks_like_ws_url(&candidate) {
                    print_error(
                        "Invalid WebSocket URL. Expected URL starting with ws:// or wss://.",
                    );
                    continue;
                }
                ws_url = if candidate == default_ws_url {
                    None
                } else {
                    Some(candidate)
                };
                break;
            }
        }

        let mut timeout_ms = current.timeout_ms;
        loop {
            let raw = optional_input(
                "Request timeout (ms)",
                Some(&format!("default: {}", timeout_ms)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            match parse_positive_u64(&raw, "Request timeout (ms)") {
                Ok(value) => {
                    timeout_ms = value;
                    break;
                }
                Err(message) => print_error(&message),
            }
        }

        let mut max_retries = current.max_retries;
        loop {
            let raw = optional_input("Max retries", Some(&format!("default: {}", max_retries)))
                .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            match parse_non_negative_u32(&raw, "Max retries") {
                Ok(value) => {
                    max_retries = value;
                    break;
                }
                Err(message) => print_error(&message),
            }
        }

        let mut retry_backoff_ms = current.retry_backoff_ms;
        loop {
            let raw = optional_input(
                "Retry backoff (ms)",
                Some(&format!("default: {}", retry_backoff_ms)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            match parse_non_negative_u64(&raw, "Retry backoff (ms)") {
                Ok(value) => {
                    retry_backoff_ms = value;
                    break;
                }
                Err(message) => print_error(&message),
            }
        }

        let mut paper_live_policy = current_policy;
        loop {
            let raw = optional_input(
                "Paper/live policy (paper_only/paper_first/live_allowed)",
                Some(&format!("default: {}", paper_live_policy)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            match normalize_paper_live_policy(&raw) {
                Some(normalized) => {
                    paper_live_policy = normalized;
                    break;
                }
                None => {
                    print_error(
                        "Invalid policy. Expected: paper_only, paper_first, or live_allowed.",
                    );
                }
            }
        }

        if network == "mainnet" && paper_live_policy == "live_allowed" {
            let confirmed = confirm(
                "Mainnet + live_allowed enables real trading. Keep this setting?",
                false,
            )
            .map_err(SetupError::Io)?;
            if !confirmed {
                paper_live_policy = "paper_first".to_string();
            }
        }

        self.settings.hyperliquid_runtime.network = network.clone();
        self.settings.hyperliquid_runtime.api_base_url = api_base_url;
        self.settings.hyperliquid_runtime.ws_url = ws_url;
        self.settings.hyperliquid_runtime.timeout_ms = timeout_ms;
        self.settings.hyperliquid_runtime.max_retries = max_retries;
        self.settings.hyperliquid_runtime.retry_backoff_ms = retry_backoff_ms;
        self.settings.hyperliquid_runtime.paper_live_policy = paper_live_policy.clone();

        print_success(&format!(
            "Hyperliquid runtime set to {} / {}",
            network, paper_live_policy
        ));
        self.step_copytrading_profile()?;
        Ok(())
    }

    /// WS-10: fixed copytrading initialization profile.
    fn step_copytrading_profile(&mut self) -> Result<(), SetupError> {
        print_info("Configure copytrading initialization guardrails.");
        print_info("These controls compile into deterministic checks before mirrored execution.");
        println!();

        let current = &self.settings.copytrading;
        print_info(&format!(
            "Current copy profile: allocation={} USD, per_trade_cap={} USD, leverage={}x, slippage={} bps",
            current.max_allocation_usd,
            current.per_trade_notional_cap_usd,
            current.max_leverage,
            current.max_slippage_bps
        ));
        println!();

        if confirm("Keep current copytrading profile?", true).map_err(SetupError::Io)? {
            print_success("Keeping existing copytrading profile");
            return Ok(());
        }

        let mut max_allocation_usd = current.max_allocation_usd;
        loop {
            let raw = optional_input(
                "Max mirrored allocation (USD)",
                Some(&format!("default: {}", max_allocation_usd)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };
            match parse_positive_u64(&raw, "Max mirrored allocation (USD)") {
                Ok(value) => {
                    max_allocation_usd = value;
                    break;
                }
                Err(message) => print_error(&message),
            }
        }

        let mut per_trade_notional_cap_usd = current.per_trade_notional_cap_usd;
        loop {
            let raw = optional_input(
                "Per-trade notional cap (USD)",
                Some(&format!("default: {}", per_trade_notional_cap_usd)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };
            match parse_positive_u64(&raw, "Per-trade notional cap (USD)") {
                Ok(value) => {
                    per_trade_notional_cap_usd = value;
                    break;
                }
                Err(message) => print_error(&message),
            }
        }

        let mut max_leverage = current.max_leverage;
        loop {
            let raw = optional_input(
                "Max leverage multiplier",
                Some(&format!("default: {}", max_leverage)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };
            match parse_positive_u64(&raw, "Max leverage multiplier") {
                Ok(value) => {
                    max_leverage = value as u32;
                    break;
                }
                Err(message) => print_error(&message),
            }
        }

        let symbol_allowlist = parse_symbol_list(
            optional_input(
                "Symbol allowlist (comma-separated, required)",
                Some(&format!("default: {}", current.symbol_allowlist.join(","))),
            )
            .map_err(SetupError::Io)?
            .as_deref(),
            &current.symbol_allowlist,
            true,
        )?;

        let symbol_denylist = parse_symbol_list(
            optional_input(
                "Symbol denylist (comma-separated, optional)",
                Some(&format!("default: {}", current.symbol_denylist.join(","))),
            )
            .map_err(SetupError::Io)?
            .as_deref(),
            &current.symbol_denylist,
            false,
        )?;

        let mut max_slippage_bps = current.max_slippage_bps;
        loop {
            let raw = optional_input(
                "Max slippage (bps)",
                Some(&format!("default: {}", max_slippage_bps)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };
            match parse_non_negative_u32(&raw, "Max slippage (bps)") {
                Ok(value) if value > 0 => {
                    max_slippage_bps = value;
                    break;
                }
                Ok(_) => print_error("Max slippage (bps) must be greater than zero."),
                Err(message) => print_error(&message),
            }
        }

        let mut information_sharing_scope = current.information_sharing_scope.clone();
        loop {
            let raw = optional_input(
                "Information-sharing scope (none/signals_only/signals_and_execution/full_audit)",
                Some(&format!("default: {}", information_sharing_scope)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };
            match normalize_information_sharing_scope(&raw) {
                Some(scope) => {
                    information_sharing_scope = scope;
                    break;
                }
                None => print_error(
                    "Invalid scope. Expected: none, signals_only, signals_and_execution, or full_audit.",
                ),
            }
        }

        self.settings.copytrading.max_allocation_usd = max_allocation_usd;
        self.settings.copytrading.per_trade_notional_cap_usd = per_trade_notional_cap_usd;
        self.settings.copytrading.max_leverage = max_leverage;
        self.settings.copytrading.symbol_allowlist = symbol_allowlist;
        self.settings.copytrading.symbol_denylist = symbol_denylist;
        self.settings.copytrading.max_slippage_bps = max_slippage_bps;
        self.settings.copytrading.information_sharing_scope = information_sharing_scope.clone();

        print_success(&format!(
            "Copytrading profile set (allocation={} USD, per_trade_cap={} USD, leverage={}x, sharing={})",
            max_allocation_usd, per_trade_notional_cap_usd, max_leverage, information_sharing_scope
        ));
        Ok(())
    }

    /// Step 9: Wallet and vault policy configuration.
    fn step_wallet_vault_policy(&mut self) -> Result<(), SetupError> {
        print_info("Configure wallet custody mode, wallet provisioning, and risk guardrails.");
        print_info("Conservative defaults: operator wallet, 1000 USD max position, 2x leverage.");
        println!();

        let current = &self.settings.wallet_vault_policy;
        let current_custody = normalize_custody_mode(&current.custody_mode)
            .unwrap_or_else(|| "operator_wallet".to_string());
        let current_kill_behavior = normalize_kill_switch_behavior(&current.kill_switch_behavior)
            .unwrap_or_else(|| "pause_agent".to_string());
        let current_policy_complete = validate_wallet_policy_requirements(
            &current_custody,
            current.operator_wallet_address.as_deref(),
            current.user_wallet_address.as_deref(),
        )
        .is_ok()
            && current
                .vault_address
                .as_deref()
                .map(is_hex_wallet_address)
                .unwrap_or(true);

        print_info(&format!(
            "Current policy: custody={}, max_position={} USD, leverage={}x, kill_switch={}",
            current_custody,
            current.max_position_size_usd,
            current.leverage_cap,
            current.kill_switch_enabled
        ));
        println!();

        if current_policy_complete {
            if confirm("Keep current wallet and vault policy?", true).map_err(SetupError::Io)? {
                print_success("Keeping existing wallet and vault policy");
                return Ok(());
            }
        } else {
            print_info(
                "Current policy is incomplete for the selected custody mode and cannot be kept.",
            );
            println!();
        }

        let custody_options = [
            "operator_wallet - operator-controlled wallet",
            "user_wallet - user-controlled wallet",
            "dual_mode - operator + user wallets",
        ];
        let custody_choice =
            select_one("Select custody mode:", &custody_options).map_err(SetupError::Io)?;
        let custody_mode = match custody_choice {
            0 => "operator_wallet".to_string(),
            1 => "user_wallet".to_string(),
            2 => "dual_mode".to_string(),
            _ => current_custody,
        };

        let operator_required = custody_mode == "operator_wallet" || custody_mode == "dual_mode";
        let user_required = custody_mode == "user_wallet" || custody_mode == "dual_mode";

        let operator_wallet_address = prompt_wallet_address_with_provisioning(
            "Operator",
            current.operator_wallet_address.as_deref(),
            operator_required,
        )?;
        let user_wallet_address = prompt_wallet_address_with_provisioning(
            "User",
            current.user_wallet_address.as_deref(),
            user_required,
        )?;
        let vault_address = prompt_wallet_address(
            "Vault address (0x..., optional)",
            current.vault_address.as_deref(),
            false,
        )?;

        let mut max_position_size_usd = current.max_position_size_usd;
        loop {
            let raw = optional_input(
                "Max position size (USD)",
                Some(&format!("default: {}", max_position_size_usd)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            match parse_positive_u64(&raw, "Max position size (USD)") {
                Ok(value) => {
                    max_position_size_usd = value;
                    break;
                }
                Err(message) => print_error(&message),
            }
        }

        let mut leverage_cap = current.leverage_cap;
        loop {
            let raw = optional_input("Leverage cap", Some(&format!("default: {}", leverage_cap)))
                .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            match parse_positive_u32(&raw, "Leverage cap") {
                Ok(value) => {
                    leverage_cap = value;
                    break;
                }
                Err(message) => print_error(&message),
            }
        }

        let kill_switch_enabled = confirm(
            "Enable kill switch protections?",
            current.kill_switch_enabled,
        )
        .map_err(SetupError::Io)?;

        let mut kill_switch_behavior = current_kill_behavior;
        if kill_switch_enabled {
            loop {
                let raw = optional_input(
                    "Kill switch behavior (pause_agent/cancel_open_orders/cancel_and_flatten)",
                    Some(&format!("default: {}", kill_switch_behavior)),
                )
                .map_err(SetupError::Io)?;
                let Some(raw) = raw else {
                    break;
                };

                match normalize_kill_switch_behavior(&raw) {
                    Some(normalized) => {
                        kill_switch_behavior = normalized;
                        break;
                    }
                    None => {
                        print_error(
                            "Invalid kill switch behavior. Expected: pause_agent, cancel_open_orders, or cancel_and_flatten.",
                        );
                    }
                }
            }
        } else {
            kill_switch_behavior = "pause_agent".to_string();
        }

        validate_wallet_policy_requirements(
            &custody_mode,
            operator_wallet_address.as_deref(),
            user_wallet_address.as_deref(),
        )
        .map_err(SetupError::Config)?;

        self.settings.wallet_vault_policy.custody_mode = custody_mode.clone();
        self.settings.wallet_vault_policy.operator_wallet_address = operator_wallet_address;
        self.settings.wallet_vault_policy.user_wallet_address = user_wallet_address;
        self.settings.wallet_vault_policy.vault_address = vault_address;
        self.settings.wallet_vault_policy.max_position_size_usd = max_position_size_usd;
        self.settings.wallet_vault_policy.leverage_cap = leverage_cap;
        self.settings.wallet_vault_policy.kill_switch_enabled = kill_switch_enabled;
        self.settings.wallet_vault_policy.kill_switch_behavior = kill_switch_behavior.clone();

        print_success(&format!(
            "Wallet policy set: custody={}, max_position={} USD, leverage={}x",
            custody_mode, max_position_size_usd, leverage_cap
        ));
        Ok(())
    }

    /// Step 10: Verification backend configuration.
    fn step_verification_backend(&mut self) -> Result<(), SetupError> {
        print_info("Configure verification backend and signed fallback receipts.");
        print_info("Conservative defaults keep fallback enabled and signed receipts required.");
        println!();

        let current = &self.settings.verification_backend;
        let current_backend = normalize_verification_backend(&current.backend)
            .unwrap_or_else(|| "eigencloud_primary".to_string());
        let current_auth_scheme = normalize_eigencloud_auth_scheme(&current.eigencloud_auth_scheme)
            .unwrap_or_else(|| "bearer".to_string());

        print_info(&format!(
            "Current verification: backend={}, fallback_enabled={}, require_signed_receipts={}",
            current_backend, current.fallback_enabled, current.fallback_require_signed_receipts
        ));
        println!();

        if confirm("Keep current verification backend policy?", true).map_err(SetupError::Io)? {
            print_success("Keeping existing verification backend policy");
            return Ok(());
        }

        let mut backend = current_backend;
        loop {
            let raw = optional_input(
                "Verification backend (eigencloud_primary/fallback_only)",
                Some(&format!("default: {}", backend)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            match normalize_verification_backend(&raw) {
                Some(normalized) => {
                    backend = normalized;
                    break;
                }
                None => {
                    print_error("Invalid backend. Expected: eigencloud_primary or fallback_only.");
                }
            }
        }

        let mut eigencloud_endpoint = current.eigencloud_endpoint.clone();
        loop {
            let hint = eigencloud_endpoint
                .as_deref()
                .map(|value| format!("current: {}", value))
                .unwrap_or_else(|| "optional".to_string());

            let raw = optional_input("EigenCloud endpoint URL (http/https)", Some(&hint))
                .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            if !looks_like_http_url(&raw) {
                print_error("Invalid endpoint URL. Expected http:// or https://.");
                continue;
            }
            eigencloud_endpoint = Some(raw);
            break;
        }

        let mut eigencloud_auth_scheme = current_auth_scheme;
        loop {
            let raw = optional_input(
                "EigenCloud auth scheme (bearer/api_key)",
                Some(&format!("default: {}", eigencloud_auth_scheme)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            match normalize_eigencloud_auth_scheme(&raw) {
                Some(normalized) => {
                    eigencloud_auth_scheme = normalized;
                    break;
                }
                None => {
                    print_error("Invalid auth scheme. Expected: bearer or api_key.");
                }
            }
        }

        let mut eigencloud_auth_token = current.eigencloud_auth_token.clone();
        let keep_existing_token_default = eigencloud_auth_token.is_some();
        if confirm(
            "Keep existing EigenCloud auth token?",
            keep_existing_token_default,
        )
        .map_err(SetupError::Io)?
        {
            // keep current token as-is
        } else {
            let token = secret_input("EigenCloud auth token (leave empty to unset)")
                .map_err(SetupError::Io)?;
            let token = token.expose_secret().trim().to_string();
            eigencloud_auth_token = if token.is_empty() { None } else { Some(token) };
        }

        let mut eigencloud_timeout_ms = current.eigencloud_timeout_ms;
        loop {
            let raw = optional_input(
                "EigenCloud timeout (ms)",
                Some(&format!("default: {}", eigencloud_timeout_ms)),
            )
            .map_err(SetupError::Io)?;
            let Some(raw) = raw else {
                break;
            };

            match parse_positive_u64(&raw, "EigenCloud timeout (ms)") {
                Ok(value) => {
                    eigencloud_timeout_ms = value;
                    break;
                }
                Err(message) => print_error(&message),
            }
        }

        let mut fallback_enabled = confirm(
            "Enable signed local fallback receipt chain?",
            current.fallback_enabled,
        )
        .map_err(SetupError::Io)?;

        if backend == "fallback_only" && !fallback_enabled {
            print_info("fallback_only backend requires fallback receipts. Enabling fallback.");
            fallback_enabled = true;
        }

        let mut fallback_signing_key_id = current.fallback_signing_key_id.clone();
        if fallback_enabled {
            let hint = fallback_signing_key_id
                .as_deref()
                .map(|value| format!("current: {}", value))
                .unwrap_or_else(|| "optional".to_string());
            let raw =
                optional_input("Fallback signing key id", Some(&hint)).map_err(SetupError::Io)?;
            if let Some(raw) = raw {
                let trimmed = raw.trim();
                fallback_signing_key_id = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
            }
        }

        let mut fallback_chain_path = current.fallback_chain_path.clone();
        if fallback_enabled {
            let default_chain_path = fallback_chain_path
                .clone()
                .unwrap_or_else(default_fallback_chain_path);
            let raw = optional_input(
                "Fallback receipt chain path",
                Some(&format!("default: {}", default_chain_path.display())),
            )
            .map_err(SetupError::Io)?;

            fallback_chain_path = match raw {
                Some(path) => {
                    let trimmed = path.trim();
                    if trimmed.is_empty() {
                        Some(default_chain_path)
                    } else {
                        Some(std::path::PathBuf::from(trimmed))
                    }
                }
                None => Some(default_chain_path),
            };
        }

        let mut fallback_require_signed_receipts = current.fallback_require_signed_receipts;
        if fallback_enabled {
            fallback_require_signed_receipts = confirm(
                "Require signed fallback receipts?",
                current.fallback_require_signed_receipts,
            )
            .map_err(SetupError::Io)?;
        }

        validate_verification_backend_requirements(&backend, fallback_enabled)
            .map_err(SetupError::Config)?;

        self.settings.verification_backend.backend = backend.clone();
        self.settings.verification_backend.eigencloud_endpoint = eigencloud_endpoint;
        self.settings.verification_backend.eigencloud_auth_scheme = eigencloud_auth_scheme;
        self.settings.verification_backend.eigencloud_auth_token = eigencloud_auth_token;
        self.settings.verification_backend.eigencloud_timeout_ms = eigencloud_timeout_ms;
        self.settings.verification_backend.fallback_enabled = fallback_enabled;
        self.settings.verification_backend.fallback_signing_key_id = fallback_signing_key_id;
        self.settings.verification_backend.fallback_chain_path = fallback_chain_path;
        self.settings
            .verification_backend
            .fallback_require_signed_receipts = fallback_require_signed_receipts;

        print_success(&format!(
            "Verification backend set to {} (fallback enabled: {})",
            backend, fallback_enabled
        ));
        Ok(())
    }

    /// Save settings to the database and `~/.enclagent/.env`, then print summary.
    async fn save_and_summarize(&mut self) -> Result<(), SetupError> {
        self.settings.onboard_completed = true;

        // Write all settings to the database (whichever backend is active).
        {
            let db_map = self.settings.to_db_map();
            let saved = false;

            #[cfg(feature = "postgres")]
            let saved = if !saved {
                if let Some(ref pool) = self.db_pool {
                    let store = crate::history::Store::from_pool(pool.clone());
                    store
                        .set_all_settings("default", &db_map)
                        .await
                        .map_err(|e| {
                            SetupError::Database(format!(
                                "Failed to save settings to database: {}",
                                e
                            ))
                        })?;
                    true
                } else {
                    false
                }
            } else {
                saved
            };

            #[cfg(feature = "libsql")]
            let saved = if !saved {
                if let Some(ref backend) = self.db_backend {
                    use crate::db::SettingsStore as _;
                    backend
                        .set_all_settings("default", &db_map)
                        .await
                        .map_err(|e| {
                            SetupError::Database(format!(
                                "Failed to save settings to database: {}",
                                e
                            ))
                        })?;
                    true
                } else {
                    false
                }
            } else {
                saved
            };

            if !saved {
                return Err(SetupError::Database(
                    "No database connection, cannot save settings".to_string(),
                ));
            }
        }

        // Persist database bootstrap vars to ~/.enclagent/.env.
        // These are the chicken-and-egg settings: we need them to decide
        // which database to connect to, so they can't live in the database.
        {
            let mut env_vars: Vec<(&str, String)> = Vec::new();

            if let Some(ref backend) = self.settings.database_backend {
                env_vars.push(("DATABASE_BACKEND", backend.clone()));
            }
            if let Some(ref url) = self.settings.database_url {
                env_vars.push(("DATABASE_URL", url.clone()));
            }
            if let Some(ref path) = self.settings.libsql_path {
                env_vars.push(("LIBSQL_PATH", path.clone()));
            }
            if let Some(ref url) = self.settings.libsql_url {
                env_vars.push(("LIBSQL_URL", url.clone()));
            }

            // LLM bootstrap vars: same chicken-and-egg problem as DATABASE_BACKEND.
            // Config::from_env() needs the backend before the DB is connected.
            if let Some(ref backend) = self.settings.llm_backend {
                env_vars.push(("LLM_BACKEND", backend.clone()));
            }
            if let Some(ref url) = self.settings.openai_compatible_base_url {
                env_vars.push(("LLM_BASE_URL", url.clone()));
            }
            if let Some(ref url) = self.settings.ollama_base_url {
                env_vars.push(("OLLAMA_BASE_URL", url.clone()));
            }

            if !env_vars.is_empty() {
                let pairs: Vec<(&str, &str)> =
                    env_vars.iter().map(|(k, v)| (*k, v.as_str())).collect();
                crate::bootstrap::save_bootstrap_env(&pairs).map_err(|e| {
                    SetupError::Io(std::io::Error::other(format!(
                        "Failed to save bootstrap env to .env: {}",
                        e
                    )))
                })?;
            }
        }

        println!();
        print_success("Configuration saved to database");
        println!();

        // Print summary
        println!("Configuration Summary:");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        let backend = self
            .settings
            .database_backend
            .as_deref()
            .unwrap_or("postgres");
        match backend {
            "libsql" => {
                if let Some(ref path) = self.settings.libsql_path {
                    println!("  Database: libSQL ({})", path);
                } else {
                    println!("  Database: libSQL (default path)");
                }
                if self.settings.libsql_url.is_some() {
                    println!("  Turso sync: enabled");
                }
            }
            _ => {
                if self.settings.database_url.is_some() {
                    println!("  Database: PostgreSQL (configured)");
                }
            }
        }

        match self.settings.secrets_master_key_source {
            KeySource::Keychain => println!("  Security: OS keychain"),
            KeySource::Env => println!("  Security: environment variable"),
            KeySource::None => println!("  Security: disabled"),
        }

        if let Some(ref provider) = self.settings.llm_backend {
            let display = match provider.as_str() {
                "nearai" => "NEAR AI",
                "anthropic" => "Anthropic",
                "openai" => "OpenAI",
                "ollama" => "Ollama",
                "openai_compatible" => "OpenAI-compatible",
                other => other,
            };
            println!("  Provider: {}", display);
        }

        if let Some(ref model) = self.settings.selected_model {
            // Truncate long model names (char-based to avoid UTF-8 panic)
            let display = if model.chars().count() > 40 {
                let truncated: String = model.chars().take(37).collect();
                format!("{}...", truncated)
            } else {
                model.clone()
            };
            println!("  Model: {}", display);
        }

        if self.settings.embeddings.enabled {
            println!(
                "  Embeddings: {} ({})",
                self.settings.embeddings.provider, self.settings.embeddings.model
            );
        } else {
            println!("  Embeddings: disabled");
        }

        if let Some(ref tunnel_url) = self.settings.tunnel.public_url {
            println!("  Tunnel: {} (static)", tunnel_url);
        } else if let Some(ref provider) = self.settings.tunnel.provider {
            println!("  Tunnel: {} (managed, starts at boot)", provider);
        }

        let has_tunnel =
            self.settings.tunnel.public_url.is_some() || self.settings.tunnel.provider.is_some();

        println!("  Channels:");
        println!("    - CLI/TUI: enabled");

        if self.settings.channels.http_enabled {
            let port = self.settings.channels.http_port.unwrap_or(8080);
            println!("    - HTTP: enabled (port {})", port);
        }

        for channel_name in &self.settings.channels.wasm_channels {
            let mode = if has_tunnel { "webhook" } else { "polling" };
            println!(
                "    - {}: enabled ({})",
                capitalize_first(channel_name),
                mode
            );
        }

        if self.settings.heartbeat.enabled {
            println!(
                "  Heartbeat: every {} minutes",
                self.settings.heartbeat.interval_secs / 60
            );
        }

        let runtime = &self.settings.hyperliquid_runtime;
        let runtime_network = normalize_hyperliquid_network(&runtime.network)
            .unwrap_or_else(|| "testnet".to_string());
        let runtime_policy = normalize_paper_live_policy(&runtime.paper_live_policy)
            .unwrap_or_else(|| "paper_first".to_string());
        let runtime_api = runtime
            .api_base_url
            .clone()
            .unwrap_or_else(|| default_hyperliquid_api_base_url(&runtime_network).to_string());
        let runtime_ws = runtime
            .ws_url
            .clone()
            .unwrap_or_else(|| default_hyperliquid_ws_url(&runtime_network).to_string());

        println!(
            "  Hyperliquid Runtime: {} / {}",
            runtime_network, runtime_policy
        );
        println!(
            "    - Timeout: {}ms, retries: {}, backoff: {}ms",
            runtime.timeout_ms, runtime.max_retries, runtime.retry_backoff_ms
        );
        println!("    - API: {}", runtime_api);
        println!("    - WS: {}", runtime_ws);

        let copy = &self.settings.copytrading;
        let sharing_scope = normalize_information_sharing_scope(&copy.information_sharing_scope)
            .unwrap_or_else(|| "signals_only".to_string());
        println!(
            "  Copytrading Profile: allocation={} USD, per_trade_cap={} USD, leverage={}x, slippage={} bps",
            copy.max_allocation_usd,
            copy.per_trade_notional_cap_usd,
            copy.max_leverage,
            copy.max_slippage_bps
        );
        println!(
            "    - Sharing scope: {}, allowlist={}, denylist={}",
            sharing_scope,
            copy.symbol_allowlist.join(","),
            if copy.symbol_denylist.is_empty() {
                "-".to_string()
            } else {
                copy.symbol_denylist.join(",")
            }
        );

        let wallet = &self.settings.wallet_vault_policy;
        let custody_mode = normalize_custody_mode(&wallet.custody_mode)
            .unwrap_or_else(|| "operator_wallet".to_string());
        let kill_switch_behavior = normalize_kill_switch_behavior(&wallet.kill_switch_behavior)
            .unwrap_or_else(|| "pause_agent".to_string());
        println!(
            "  Wallet/Vault Policy: custody={}, max_position={} USD, leverage={}x",
            custody_mode, wallet.max_position_size_usd, wallet.leverage_cap
        );
        if let Some(ref operator) = wallet.operator_wallet_address {
            println!("    - Operator wallet: {}", mask_wallet_address(operator));
        }
        if let Some(ref user) = wallet.user_wallet_address {
            println!("    - User wallet: {}", mask_wallet_address(user));
        }
        if let Some(ref vault) = wallet.vault_address {
            println!("    - Vault: {}", mask_wallet_address(vault));
        }
        if wallet.kill_switch_enabled {
            println!("    - Kill switch: enabled ({})", kill_switch_behavior);
        } else {
            println!("    - Kill switch: disabled");
        }

        let verification = &self.settings.verification_backend;
        let verification_backend = normalize_verification_backend(&verification.backend)
            .unwrap_or_else(|| "eigencloud_primary".to_string());
        let eigencloud_auth_scheme =
            normalize_eigencloud_auth_scheme(&verification.eigencloud_auth_scheme)
                .unwrap_or_else(|| "bearer".to_string());
        println!("  Verification: {}", verification_backend);
        if let Some(ref endpoint) = verification.eigencloud_endpoint {
            println!("    - EigenCloud endpoint: {}", endpoint);
        }
        println!(
            "    - EigenCloud auth: {} ({})",
            eigencloud_auth_scheme,
            if verification.eigencloud_auth_token.is_some() {
                "token configured"
            } else {
                "no token configured"
            }
        );
        println!(
            "    - EigenCloud timeout: {}ms",
            verification.eigencloud_timeout_ms
        );
        println!(
            "    - Fallback: enabled={}, require_signed_receipts={}",
            verification.fallback_enabled, verification.fallback_require_signed_receipts
        );
        if let Some(ref key_id) = verification.fallback_signing_key_id {
            println!("    - Fallback signing key id: {}", key_id);
        }
        if let Some(ref chain_path) = verification.fallback_chain_path {
            println!("    - Fallback chain path: {}", chain_path.display());
        }

        println!();
        println!("To start the agent, run:");
        println!("  enclagent");
        println!();
        println!("To change settings later:");
        println!("  enclagent config set <setting> <value>");
        println!("  enclagent onboard");
        println!();

        Ok(())
    }
}

impl Default for SetupWizard {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_variant(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn normalize_hyperliquid_network(value: &str) -> Option<String> {
    match normalize_variant(value).as_str() {
        "testnet" | "test_net" => Some("testnet".to_string()),
        "mainnet" | "main_net" => Some("mainnet".to_string()),
        _ => None,
    }
}

fn normalize_paper_live_policy(value: &str) -> Option<String> {
    match normalize_variant(value).as_str() {
        "paper" | "paper_only" => Some("paper_only".to_string()),
        "paper_first" => Some("paper_first".to_string()),
        "live" | "live_allowed" => Some("live_allowed".to_string()),
        _ => None,
    }
}

fn normalize_custody_mode(value: &str) -> Option<String> {
    match normalize_variant(value).as_str() {
        "operator" | "operator_wallet" => Some("operator_wallet".to_string()),
        "user" | "user_wallet" => Some("user_wallet".to_string()),
        "dual" | "dual_mode" => Some("dual_mode".to_string()),
        _ => None,
    }
}

fn normalize_kill_switch_behavior(value: &str) -> Option<String> {
    match normalize_variant(value).as_str() {
        "pause" | "pause_agent" => Some("pause_agent".to_string()),
        "cancel_open_orders" | "cancel_orders" => Some("cancel_open_orders".to_string()),
        "cancel_and_flatten" | "flatten" => Some("cancel_and_flatten".to_string()),
        _ => None,
    }
}

fn normalize_verification_backend(value: &str) -> Option<String> {
    match normalize_variant(value).as_str() {
        "eigencloud" | "eigencloud_primary" => Some("eigencloud_primary".to_string()),
        "fallback" | "fallback_only" => Some("fallback_only".to_string()),
        _ => None,
    }
}

fn normalize_eigencloud_auth_scheme(value: &str) -> Option<String> {
    match normalize_variant(value).as_str() {
        "bearer" => Some("bearer".to_string()),
        "api_key" | "apikey" => Some("api_key".to_string()),
        _ => None,
    }
}

fn normalize_information_sharing_scope(value: &str) -> Option<String> {
    match normalize_variant(value).as_str() {
        "none" => Some("none".to_string()),
        "signals" | "signals_only" => Some("signals_only".to_string()),
        "signals_and_execution" | "signals_execution" | "execution" => {
            Some("signals_and_execution".to_string())
        }
        "full" | "full_audit" => Some("full_audit".to_string()),
        _ => None,
    }
}

fn parse_symbol_list(
    raw: Option<&str>,
    current: &[String],
    required: bool,
) -> Result<Vec<String>, SetupError> {
    let parsed = match raw {
        Some(value) => value
            .split(',')
            .map(|symbol| symbol.trim().to_ascii_uppercase())
            .filter(|symbol| !symbol.is_empty())
            .collect::<Vec<_>>(),
        None => current.to_vec(),
    };

    if required && parsed.is_empty() {
        return Err(SetupError::Config(
            "At least one symbol is required in allowlist".to_string(),
        ));
    }
    Ok(parsed)
}

fn default_hyperliquid_api_base_url(network: &str) -> &'static str {
    match normalize_hyperliquid_network(network).as_deref() {
        Some("mainnet") => "https://api.hyperliquid.xyz",
        _ => "https://api.hyperliquid-testnet.xyz",
    }
}

fn default_hyperliquid_ws_url(network: &str) -> &'static str {
    match normalize_hyperliquid_network(network).as_deref() {
        Some("mainnet") => "wss://api.hyperliquid.xyz/ws",
        _ => "wss://api.hyperliquid-testnet.xyz/ws",
    }
}

fn default_fallback_chain_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".enclagent")
        .join("receipts")
        .join("receipt-chain.ndjson")
}

fn looks_like_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn looks_like_ws_url(value: &str) -> bool {
    value.starts_with("ws://") || value.starts_with("wss://")
}

fn is_hex_wallet_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value.chars().skip(2).all(|ch| ch.is_ascii_hexdigit())
}

fn mask_wallet_address(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 12 {
        return format!("{}...", chars.iter().take(4).collect::<String>());
    }
    let prefix: String = chars.iter().take(6).collect();
    let suffix: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{prefix}...{suffix}")
}

fn to_hex_prefixed(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(2 + bytes.len() * 2);
    output.push_str("0x");
    for byte in bytes {
        let _ = write!(&mut output, "{:02x}", byte);
    }
    output
}

fn generate_wallet_address() -> String {
    let mut bytes = [0u8; 20];
    OsRng.fill_bytes(&mut bytes);
    to_hex_prefixed(&bytes)
}

fn parse_positive_u64(raw: &str, field: &str) -> Result<u64, String> {
    let value = raw
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("{field} must be a positive integer."))?;
    if value == 0 {
        return Err(format!("{field} must be > 0."));
    }
    Ok(value)
}

fn parse_non_negative_u64(raw: &str, field: &str) -> Result<u64, String> {
    raw.trim()
        .parse::<u64>()
        .map_err(|_| format!("{field} must be a non-negative integer."))
}

fn parse_positive_u32(raw: &str, field: &str) -> Result<u32, String> {
    let value = raw
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("{field} must be a positive integer."))?;
    if value == 0 {
        return Err(format!("{field} must be > 0."));
    }
    Ok(value)
}

fn parse_non_negative_u32(raw: &str, field: &str) -> Result<u32, String> {
    raw.trim()
        .parse::<u32>()
        .map_err(|_| format!("{field} must be a non-negative integer."))
}

fn validate_wallet_policy_requirements(
    custody_mode: &str,
    operator_wallet_address: Option<&str>,
    user_wallet_address: Option<&str>,
) -> Result<(), String> {
    if let Some(address) = operator_wallet_address
        && !is_hex_wallet_address(address)
    {
        return Err("Operator wallet address must be a 0x-prefixed 40-hex address.".to_string());
    }
    if let Some(address) = user_wallet_address
        && !is_hex_wallet_address(address)
    {
        return Err("User wallet address must be a 0x-prefixed 40-hex address.".to_string());
    }

    match custody_mode {
        "operator_wallet" if operator_wallet_address.is_none() => {
            Err("operator_wallet mode requires operator wallet address.".to_string())
        }
        "user_wallet" if user_wallet_address.is_none() => {
            Err("user_wallet mode requires user wallet address.".to_string())
        }
        "dual_mode" if operator_wallet_address.is_none() || user_wallet_address.is_none() => {
            Err("dual_mode requires both operator and user wallet addresses.".to_string())
        }
        "operator_wallet" | "user_wallet" | "dual_mode" => Ok(()),
        other => Err(format!("Invalid custody mode '{other}'.")),
    }
}

fn validate_verification_backend_requirements(
    backend: &str,
    fallback_enabled: bool,
) -> Result<(), String> {
    let Some(normalized_backend) = normalize_verification_backend(backend) else {
        return Err(format!("Invalid verification backend '{backend}'."));
    };

    if normalized_backend == "fallback_only" && !fallback_enabled {
        return Err("fallback_only backend requires fallback_enabled=true.".to_string());
    }

    Ok(())
}

fn prompt_wallet_address(
    label: &str,
    current: Option<&str>,
    required: bool,
) -> Result<Option<String>, SetupError> {
    let current_valid = current.filter(|value| is_hex_wallet_address(value));

    let hint = current_valid
        .map(mask_wallet_address)
        .map(|masked| format!("current: {}", masked))
        .unwrap_or_else(|| {
            if required {
                "required".to_string()
            } else {
                "optional".to_string()
            }
        });

    loop {
        let raw = optional_input(label, Some(&hint)).map_err(SetupError::Io)?;
        let Some(raw) = raw else {
            if required && current_valid.is_none() {
                print_error("This address is required for the selected custody mode.");
                continue;
            }
            return Ok(current_valid.map(|v| v.to_string()));
        };

        let candidate = raw.trim();
        if candidate.is_empty() {
            if required && current_valid.is_none() {
                print_error("This address is required for the selected custody mode.");
                continue;
            }
            return Ok(current_valid.map(|v| v.to_string()));
        }

        if !is_hex_wallet_address(candidate) {
            print_error("Invalid address. Expected a 0x-prefixed 40-hex address.");
            continue;
        }

        return Ok(Some(candidate.to_string()));
    }
}

fn prompt_wallet_address_with_provisioning(
    role_label: &str,
    current: Option<&str>,
    required: bool,
) -> Result<Option<String>, SetupError> {
    let current_valid = current.filter(|value| is_hex_wallet_address(value));
    let mut options = Vec::new();
    if current_valid.is_some() {
        options.push("Keep current wallet address");
    }
    options.push("Add existing wallet address");
    options.push("Create new wallet locally");
    if !required {
        options.push("Skip for now");
    }

    let choice =
        select_one(&format!("{role_label} wallet setup:"), &options).map_err(SetupError::Io)?;
    match options.get(choice).copied() {
        Some("Keep current wallet address") => Ok(current_valid.map(|value| value.to_string())),
        Some("Add existing wallet address") => prompt_wallet_address(
            &format!("{role_label} wallet address (0x...)"),
            current_valid,
            required,
        ),
        Some("Create new wallet locally") => {
            let address = generate_wallet_address();
            print_success(&format!(
                "Generated {} wallet address: {}",
                role_label.to_ascii_lowercase(),
                mask_wallet_address(&address)
            ));
            print_info(
                "This generated address is suitable for local paper-mode setup. Replace with a real funded wallet before live trading.",
            );
            Ok(Some(address))
        }
        Some("Skip for now") => Ok(None),
        _ => Err(SetupError::Config(
            "Invalid wallet provisioning choice".to_string(),
        )),
    }
}

/// Mask password in a database URL for display.
#[cfg(feature = "postgres")]
fn mask_password_in_url(url: &str) -> String {
    // URL format: scheme://user:password@host/database
    // Find "://" to locate start of credentials
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let credentials_start = scheme_end + 3; // After "://"

    // Find "@" to locate end of credentials
    let Some(at_pos) = url[credentials_start..].find('@') else {
        return url.to_string();
    };
    let at_abs = credentials_start + at_pos;

    // Find ":" in the credentials section (separates user from password)
    let credentials = &url[credentials_start..at_abs];
    let Some(colon_pos) = credentials.find(':') else {
        return url.to_string();
    };

    // Build masked URL: scheme://user:****@host/database
    let scheme = &url[..credentials_start]; // "postgres://"
    let username = &credentials[..colon_pos]; // "user"
    let after_at = &url[at_abs..]; // "@localhost/db"

    format!("{}{}:****{}", scheme, username, after_at)
}

/// Fetch models from the Anthropic API.
///
/// Returns `(model_id, display_label)` pairs. Falls back to static defaults on error.
async fn fetch_anthropic_models(cached_key: Option<&str>) -> Vec<(String, String)> {
    let static_defaults = vec![
        ("claude-sonnet-4-20250514".into(), "Claude Sonnet 4".into()),
        ("claude-opus-4-20250514".into(), "Claude Opus 4".into()),
        (
            "claude-3-5-haiku-20241022".into(),
            "Claude 3.5 Haiku (fast)".into(),
        ),
    ];

    let api_key = cached_key
        .map(String::from)
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .filter(|k| !k.is_empty());

    let api_key = match api_key {
        Some(k) => k,
        None => return static_defaults,
    };

    let client = reqwest::Client::new();
    let resp = match client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return static_defaults,
    };

    #[derive(serde::Deserialize)]
    struct ModelEntry {
        id: String,
    }
    #[derive(serde::Deserialize)]
    struct ModelsResponse {
        data: Vec<ModelEntry>,
    }

    match resp.json::<ModelsResponse>().await {
        Ok(body) => {
            let mut models: Vec<(String, String)> = body
                .data
                .into_iter()
                .filter(|m| !m.id.contains("embedding") && !m.id.contains("audio"))
                .map(|m| {
                    let label = m.id.clone();
                    (m.id, label)
                })
                .collect();
            if models.is_empty() {
                return static_defaults;
            }
            models.sort_by(|a, b| a.0.cmp(&b.0));
            models
        }
        Err(_) => static_defaults,
    }
}

/// Fetch models from the OpenAI API.
///
/// Returns `(model_id, display_label)` pairs. Falls back to static defaults on error.
async fn fetch_openai_models(cached_key: Option<&str>) -> Vec<(String, String)> {
    let static_defaults = vec![
        ("gpt-5".into(), "GPT-5 (flagship)".into()),
        ("gpt-5-mini".into(), "GPT-5 Mini (fast)".into()),
        ("gpt-4.1".into(), "GPT-4.1".into()),
        ("gpt-4o".into(), "GPT-4o".into()),
        ("o3".into(), "o3 (reasoning)".into()),
    ];

    let api_key = cached_key
        .map(String::from)
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .filter(|k| !k.is_empty());

    let api_key = match api_key {
        Some(k) => k,
        None => return static_defaults,
    };

    let client = reqwest::Client::new();
    let resp = match client
        .get("https://api.openai.com/v1/models")
        .bearer_auth(&api_key)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return static_defaults,
    };

    #[derive(serde::Deserialize)]
    struct ModelEntry {
        id: String,
    }
    #[derive(serde::Deserialize)]
    struct ModelsResponse {
        data: Vec<ModelEntry>,
    }

    match resp.json::<ModelsResponse>().await {
        Ok(body) => {
            let mut models: Vec<(String, String)> = body
                .data
                .into_iter()
                .filter(|m| is_openai_chat_model(&m.id))
                .map(|m| {
                    let label = m.id.clone();
                    (m.id, label)
                })
                .collect();
            if models.is_empty() {
                return static_defaults;
            }
            sort_openai_models(&mut models);
            models
        }
        Err(_) => static_defaults,
    }
}

fn is_openai_chat_model(model_id: &str) -> bool {
    let id = model_id.to_ascii_lowercase();

    let is_chat_family = id.starts_with("gpt-")
        || id.starts_with("chatgpt-")
        || id.starts_with("o1")
        || id.starts_with("o3")
        || id.starts_with("o4")
        || id.starts_with("o5");

    let is_non_chat_variant = id.contains("realtime")
        || id.contains("audio")
        || id.contains("transcribe")
        || id.contains("tts")
        || id.contains("embedding")
        || id.contains("moderation")
        || id.contains("image");

    is_chat_family && !is_non_chat_variant
}

fn openai_model_priority(model_id: &str) -> usize {
    let id = model_id.to_ascii_lowercase();

    const EXACT_PRIORITY: &[&str] = &[
        "gpt-5",
        "gpt-5-mini",
        "gpt-5-nano",
        "o3",
        "o4-mini",
        "o1",
        "gpt-4.1",
        "gpt-4.1-mini",
        "gpt-4o",
        "gpt-4o-mini",
    ];
    if let Some(pos) = EXACT_PRIORITY.iter().position(|m| id == *m) {
        return pos;
    }

    const PREFIX_PRIORITY: &[&str] = &[
        "gpt-5-", "o3-", "o4-", "o1-", "gpt-4.1-", "gpt-4o-", "gpt-3.5-", "chatgpt-",
    ];
    if let Some(pos) = PREFIX_PRIORITY
        .iter()
        .position(|prefix| id.starts_with(prefix))
    {
        return EXACT_PRIORITY.len() + pos;
    }

    EXACT_PRIORITY.len() + PREFIX_PRIORITY.len() + 1
}

fn sort_openai_models(models: &mut [(String, String)]) {
    models.sort_by(|a, b| {
        openai_model_priority(&a.0)
            .cmp(&openai_model_priority(&b.0))
            .then_with(|| a.0.cmp(&b.0))
    });
}

/// Fetch installed models from a local Ollama instance.
///
/// Returns `(model_name, display_label)` pairs. Falls back to static defaults on error.
async fn fetch_ollama_models(base_url: &str) -> Vec<(String, String)> {
    let static_defaults = vec![
        ("llama3".into(), "llama3".into()),
        ("mistral".into(), "mistral".into()),
        ("codellama".into(), "codellama".into()),
    ];

    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();

    let resp = match client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(_) => return static_defaults,
        Err(_) => {
            print_info("Could not connect to Ollama. Is it running?");
            return static_defaults;
        }
    };

    #[derive(serde::Deserialize)]
    struct ModelEntry {
        name: String,
    }
    #[derive(serde::Deserialize)]
    struct TagsResponse {
        models: Vec<ModelEntry>,
    }

    match resp.json::<TagsResponse>().await {
        Ok(body) => {
            let models: Vec<(String, String)> = body
                .models
                .into_iter()
                .map(|m| {
                    let label = m.name.clone();
                    (m.name, label)
                })
                .collect();
            if models.is_empty() {
                return static_defaults;
            }
            models
        }
        Err(_) => static_defaults,
    }
}

/// Discover WASM channels in a directory.
///
/// Returns a list of (channel_name, capabilities_file) pairs.
async fn discover_wasm_channels(dir: &std::path::Path) -> Vec<(String, ChannelCapabilitiesFile)> {
    let mut channels = Vec::new();

    if !dir.is_dir() {
        return channels;
    }

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return channels,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();

        // Look for .capabilities.json files
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if !filename.ends_with(".capabilities.json") {
            continue;
        }

        // Extract channel name
        let name = filename.trim_end_matches(".capabilities.json").to_string();
        if name.is_empty() {
            continue;
        }

        // Check if corresponding .wasm file exists
        let wasm_path = dir.join(format!("{}.wasm", name));
        if !wasm_path.exists() {
            continue;
        }

        // Parse capabilities file
        match tokio::fs::read(&path).await {
            Ok(bytes) => match ChannelCapabilitiesFile::from_bytes(&bytes) {
                Ok(cap_file) => {
                    channels.push((name, cap_file));
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to parse channel capabilities file"
                    );
                }
            },
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "Failed to read channel capabilities file"
                );
            }
        }
    }

    // Sort by name for consistent ordering
    channels.sort_by(|a, b| a.0.cmp(&b.0));
    channels
}

/// Mask an API key for display: show first 6 + last 4 chars.
///
/// Uses char-based indexing to avoid panicking on multi-byte UTF-8.
fn mask_api_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() < 12 {
        let prefix: String = chars.iter().take(4).collect();
        return format!("{prefix}...");
    }
    let prefix: String = chars[..6].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{prefix}...{suffix}")
}

#[cfg(feature = "postgres")]
fn start_local_postgres_via_docker() -> Result<String, String> {
    let docker_check = std::process::Command::new("docker")
        .arg("--version")
        .output()
        .map_err(|e| format!("failed to execute docker: {}", e))?;
    if !docker_check.status.success() {
        return Err(command_output_error("docker --version", &docker_check));
    }

    if let Some(compose_path) = find_compose_file_for_wizard() {
        let compose_output = std::process::Command::new("docker")
            .arg("compose")
            .arg("-f")
            .arg(&compose_path)
            .args(["up", "-d", "postgres"])
            .output()
            .map_err(|e| format!("failed to execute docker compose: {}", e))?;
        if !compose_output.status.success() {
            return Err(command_output_error(
                "docker compose up -d postgres",
                &compose_output,
            ));
        }
        return Ok(format!(
            "Started postgres service via {}",
            compose_path.display()
        ));
    }

    // Fallback when no compose file is available (installed binary use-cases).
    let container_name = "enclagent-postgres";
    let start_existing = std::process::Command::new("docker")
        .args(["start", container_name])
        .output();
    if let Ok(output) = start_existing
        && output.status.success()
    {
        return Ok(format!(
            "Started existing Docker container '{}'",
            container_name
        ));
    }

    let run_output = std::process::Command::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            container_name,
            "-p",
            "5432:5432",
            "-e",
            "POSTGRES_DB=enclagent",
            "-e",
            "POSTGRES_USER=enclagent",
            "-e",
            "POSTGRES_PASSWORD=enclagent",
            "pgvector/pgvector:pg16",
        ])
        .output()
        .map_err(|e| format!("failed to execute docker run: {}", e))?;
    if !run_output.status.success() {
        return Err(command_output_error("docker run postgres", &run_output));
    }

    Ok(format!(
        "Started Docker container '{}' (pgvector/pgvector:pg16)",
        container_name
    ))
}

#[cfg(feature = "postgres")]
fn find_compose_file_for_wizard() -> Option<std::path::PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join("docker-compose.yml");
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

#[cfg(feature = "postgres")]
fn command_output_error(command: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        format!("{} failed: {}", command, stderr)
    } else if !stdout.is_empty() {
        format!("{} failed: {}", command, stdout)
    } else {
        format!("{} failed with status {}", command, output.status)
    }
}

/// Capitalize the first letter of a string.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

#[cfg(test)]
async fn install_missing_bundled_channels(
    channels_dir: &std::path::Path,
    already_installed: &HashSet<String>,
) -> Result<Vec<String>, SetupError> {
    let mut installed = Vec::new();

    for name in available_channel_names().iter().copied() {
        if already_installed.contains(name) {
            continue;
        }

        install_bundled_channel(name, channels_dir, false)
            .await
            .map_err(SetupError::Channel)?;
        installed.push(name.to_string());
    }

    Ok(installed)
}

fn wasm_channel_option_names(discovered: &[(String, ChannelCapabilitiesFile)]) -> Vec<String> {
    let mut names: Vec<String> = discovered.iter().map(|(name, _)| name.clone()).collect();

    for bundled in available_channel_names().iter().copied() {
        if !names.iter().any(|name| name == bundled) {
            names.push(bundled.to_string());
        }
    }

    names
}

async fn install_selected_bundled_channels(
    channels_dir: &std::path::Path,
    selected_channels: &[String],
    already_installed: &HashSet<String>,
) -> Result<Option<Vec<String>>, SetupError> {
    let bundled: HashSet<&str> = available_channel_names().iter().copied().collect();
    let selected_missing: HashSet<String> = selected_channels
        .iter()
        .filter(|name| bundled.contains(name.as_str()) && !already_installed.contains(*name))
        .cloned()
        .collect();

    if selected_missing.is_empty() {
        return Ok(None);
    }

    let mut installed = Vec::new();
    for name in selected_missing {
        install_bundled_channel(&name, channels_dir, false)
            .await
            .map_err(SetupError::Channel)?;
        installed.push(name);
    }

    installed.sort();
    Ok(Some(installed))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_wizard_creation() {
        let wizard = SetupWizard::new();
        assert!(!wizard.config.skip_auth);
        assert!(!wizard.config.channels_only);
    }

    #[test]
    fn test_wizard_with_config() {
        let config = SetupConfig {
            skip_auth: true,
            channels_only: false,
        };
        let wizard = SetupWizard::with_config(config);
        assert!(wizard.config.skip_auth);
    }

    #[test]
    fn test_normalize_wizard_provider_aliases_and_scope() {
        assert_eq!(
            normalize_wizard_provider("OpenAI-Compatible"),
            Some("openai_compatible".to_string())
        );
        assert_eq!(
            normalize_wizard_provider("NEAR AI"),
            Some("nearai".to_string())
        );
        assert_eq!(normalize_wizard_provider("tinfoil"), None);
    }

    #[test]
    fn test_normalize_hyperliquid_runtime_inputs() {
        assert_eq!(
            normalize_hyperliquid_network("mainnet"),
            Some("mainnet".to_string())
        );
        assert_eq!(
            normalize_hyperliquid_network("test-net"),
            Some("testnet".to_string())
        );
        assert_eq!(normalize_hyperliquid_network("invalid"), None);

        assert_eq!(
            normalize_paper_live_policy("paper"),
            Some("paper_only".to_string())
        );
        assert_eq!(
            normalize_paper_live_policy("live-allowed"),
            Some("live_allowed".to_string())
        );
        assert_eq!(normalize_paper_live_policy("unknown"), None);

        assert_eq!(
            default_hyperliquid_api_base_url("mainnet"),
            "https://api.hyperliquid.xyz"
        );
        assert_eq!(
            default_hyperliquid_api_base_url("testnet"),
            "https://api.hyperliquid-testnet.xyz"
        );
        assert_eq!(
            default_hyperliquid_ws_url("mainnet"),
            "wss://api.hyperliquid.xyz/ws"
        );
        assert_eq!(
            default_hyperliquid_ws_url("testnet"),
            "wss://api.hyperliquid-testnet.xyz/ws"
        );
    }

    #[test]
    fn test_normalize_wallet_and_verification_inputs() {
        assert_eq!(
            normalize_custody_mode("operator"),
            Some("operator_wallet".to_string())
        );
        assert_eq!(
            normalize_custody_mode("dual-mode"),
            Some("dual_mode".to_string())
        );
        assert_eq!(
            normalize_kill_switch_behavior("cancel_orders"),
            Some("cancel_open_orders".to_string())
        );
        assert_eq!(
            normalize_kill_switch_behavior("flatten"),
            Some("cancel_and_flatten".to_string())
        );

        assert_eq!(
            normalize_verification_backend("eigencloud"),
            Some("eigencloud_primary".to_string())
        );
        assert_eq!(
            normalize_verification_backend("fallback"),
            Some("fallback_only".to_string())
        );
        assert_eq!(
            normalize_eigencloud_auth_scheme("apiKey"),
            Some("api_key".to_string())
        );
        assert_eq!(normalize_eigencloud_auth_scheme("oauth"), None);
        assert_eq!(
            normalize_information_sharing_scope("signals"),
            Some("signals_only".to_string())
        );
        assert_eq!(
            normalize_information_sharing_scope("full"),
            Some("full_audit".to_string())
        );
        assert_eq!(normalize_information_sharing_scope("invalid"), None);
    }

    #[test]
    fn test_validate_wallet_policy_requirements() {
        let valid_operator = "0x1234567890abcdef1234567890abcdef12345678";
        let valid_user = "0xabcdef1234567890abcdef1234567890abcdef12";

        assert!(
            validate_wallet_policy_requirements("operator_wallet", Some(valid_operator), None)
                .is_ok()
        );
        assert!(validate_wallet_policy_requirements("user_wallet", None, Some(valid_user)).is_ok());
        assert!(
            validate_wallet_policy_requirements(
                "dual_mode",
                Some(valid_operator),
                Some(valid_user)
            )
            .is_ok()
        );

        assert!(
            validate_wallet_policy_requirements("operator_wallet", None, None)
                .unwrap_err()
                .contains("requires operator")
        );
        assert!(
            validate_wallet_policy_requirements("user_wallet", None, None)
                .unwrap_err()
                .contains("requires user")
        );
        assert!(
            validate_wallet_policy_requirements("dual_mode", Some(valid_operator), None)
                .unwrap_err()
                .contains("requires both")
        );
        assert!(
            validate_wallet_policy_requirements("operator_wallet", Some("0xdeadbeef"), None)
                .unwrap_err()
                .contains("0x-prefixed 40-hex")
        );
    }

    #[test]
    fn test_validate_verification_backend_requirements() {
        assert!(validate_verification_backend_requirements("eigencloud_primary", false).is_ok());
        assert!(validate_verification_backend_requirements("fallback_only", true).is_ok());
        assert!(
            validate_verification_backend_requirements("fallback_only", false)
                .unwrap_err()
                .contains("requires fallback_enabled=true")
        );
        assert!(
            validate_verification_backend_requirements("invalid", true)
                .unwrap_err()
                .contains("Invalid verification backend")
        );
    }

    #[test]
    fn test_url_and_number_validators() {
        assert!(looks_like_http_url("https://example.com"));
        assert!(!looks_like_http_url("wss://example.com/ws"));
        assert!(looks_like_ws_url("wss://example.com/ws"));
        assert!(!looks_like_ws_url("https://example.com"));

        assert_eq!(parse_positive_u64("1", "Timeout").unwrap(), 1);
        assert!(parse_positive_u64("0", "Timeout").is_err());
        assert_eq!(parse_non_negative_u64("0", "Backoff").unwrap(), 0);
        assert_eq!(parse_positive_u32("2", "Leverage").unwrap(), 2);
        assert!(parse_positive_u32("0", "Leverage").is_err());
        assert_eq!(parse_non_negative_u32("0", "Retries").unwrap(), 0);
    }

    #[test]
    fn test_wallet_address_helpers() {
        let address = "0x1234567890abcdef1234567890abcdef12345678";
        assert!(is_hex_wallet_address(address));
        assert!(!is_hex_wallet_address("0x1234"));
        assert_eq!(mask_wallet_address(address), "0x1234...5678");
    }

    #[test]
    fn test_generate_wallet_address_outputs_valid_hex_value() {
        let address = generate_wallet_address();
        assert!(is_hex_wallet_address(&address));
    }

    #[test]
    fn test_set_llm_backend_resets_model_only_on_change() {
        let mut wizard = SetupWizard::new();
        wizard.settings.llm_backend = Some("openai".to_string());
        wizard.settings.selected_model = Some("gpt-5".to_string());

        wizard.set_llm_backend("openai");
        assert_eq!(wizard.settings.selected_model, Some("gpt-5".to_string()));

        wizard.set_llm_backend("anthropic");
        assert_eq!(wizard.settings.selected_model, None);
    }

    #[test]
    fn test_require_wizard_provider_missing_errors() {
        let mut wizard = SetupWizard::new();
        wizard.settings.llm_backend = None;
        let _guard = EnvGuard::clear("LLM_BACKEND");

        let err = wizard
            .require_wizard_provider("model selection")
            .unwrap_err();
        assert!(err.to_string().contains("Inference provider is required"));
    }

    #[test]
    fn test_require_wizard_provider_accepts_env_backend() {
        let mut wizard = SetupWizard::new();
        wizard.settings.llm_backend = None;
        let _guard = EnvGuard::set("LLM_BACKEND", "openai-compatible");

        let backend = wizard.require_wizard_provider("model selection").unwrap();
        assert_eq!(backend, "openai_compatible");
        assert_eq!(
            wizard.settings.llm_backend,
            Some("openai_compatible".to_string())
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn test_mask_password_in_url() {
        assert_eq!(
            mask_password_in_url("postgres://user:secret@localhost/db"),
            "postgres://user:****@localhost/db"
        );

        // URL without password
        assert_eq!(
            mask_password_in_url("postgres://localhost/db"),
            "postgres://localhost/db"
        );
    }

    #[test]
    fn test_capitalize_first() {
        assert_eq!(capitalize_first("telegram"), "Telegram");
        assert_eq!(capitalize_first("CAPS"), "CAPS");
        assert_eq!(capitalize_first(""), "");
    }

    #[test]
    fn test_mask_api_key() {
        assert_eq!(
            mask_api_key("sk-ant-api03-abcdef1234567890"),
            "sk-ant...7890"
        );
        assert_eq!(mask_api_key("short"), "shor...");
        assert_eq!(mask_api_key("exactly12ch"), "exac...");
        assert_eq!(mask_api_key("exactly12chr"), "exactl...2chr");
        assert_eq!(mask_api_key(""), "...");
        // Multi-byte chars should not panic
        assert_eq!(mask_api_key("日本語キー"), "日本語キ...");
    }

    #[tokio::test]
    async fn test_install_missing_bundled_channels_installs_telegram() {
        // WASM artifacts only exist in dev builds (not CI). Skip gracefully
        // rather than fail when the telegram channel hasn't been compiled.
        if !available_channel_names().contains(&"telegram") {
            eprintln!("skipping: telegram WASM artifacts not built");
            return;
        }

        let dir = tempdir().unwrap();
        let installed = HashSet::<String>::new();

        install_missing_bundled_channels(dir.path(), &installed)
            .await
            .unwrap();

        assert!(dir.path().join("telegram.wasm").exists());
        assert!(dir.path().join("telegram.capabilities.json").exists());
    }

    #[test]
    fn test_wasm_channel_option_names_includes_available_when_missing() {
        let discovered = Vec::new();
        let options = wasm_channel_option_names(&discovered);
        let available = available_channel_names();
        // All available (built) channels should appear
        for name in &available {
            assert!(
                options.contains(&name.to_string()),
                "expected '{}' in options",
                name
            );
        }
    }

    #[test]
    fn test_wasm_channel_option_names_dedupes_available() {
        let discovered = vec![(String::from("telegram"), ChannelCapabilitiesFile::default())];
        let options = wasm_channel_option_names(&discovered);
        // telegram should appear exactly once despite being both discovered and available
        assert_eq!(
            options.iter().filter(|n| *n == "telegram").count(),
            1,
            "telegram should not be duplicated"
        );
    }

    #[tokio::test]
    async fn test_fetch_anthropic_models_static_fallback() {
        // With no API key, should return static defaults
        let _guard = EnvGuard::clear("ANTHROPIC_API_KEY");
        let models = fetch_anthropic_models(None).await;
        assert!(!models.is_empty());
        assert!(
            models.iter().any(|(id, _)| id.contains("claude")),
            "static defaults should include a Claude model"
        );
    }

    #[tokio::test]
    async fn test_fetch_openai_models_static_fallback() {
        let _guard = EnvGuard::clear("OPENAI_API_KEY");
        let models = fetch_openai_models(None).await;
        assert!(!models.is_empty());
        assert_eq!(models[0].0, "gpt-5");
        assert!(
            models.iter().any(|(id, _)| id.contains("gpt")),
            "static defaults should include a GPT model"
        );
    }

    #[test]
    fn test_is_openai_chat_model_includes_gpt5_and_filters_non_chat_variants() {
        assert!(is_openai_chat_model("gpt-5"));
        assert!(is_openai_chat_model("gpt-5-mini-2026-01-01"));
        assert!(is_openai_chat_model("o3-2025-04-16"));
        assert!(!is_openai_chat_model("chatgpt-image-latest"));
        assert!(!is_openai_chat_model("gpt-4o-realtime-preview"));
        assert!(!is_openai_chat_model("gpt-4o-mini-transcribe"));
        assert!(!is_openai_chat_model("text-embedding-3-large"));
    }

    #[test]
    fn test_sort_openai_models_prioritizes_best_models_first() {
        let mut models = vec![
            ("gpt-4o-mini".to_string(), "gpt-4o-mini".to_string()),
            ("gpt-5-mini".to_string(), "gpt-5-mini".to_string()),
            ("o3".to_string(), "o3".to_string()),
            ("gpt-4.1".to_string(), "gpt-4.1".to_string()),
            ("gpt-5".to_string(), "gpt-5".to_string()),
        ];

        sort_openai_models(&mut models);

        let ordered: Vec<String> = models.into_iter().map(|(id, _)| id).collect();
        assert_eq!(
            ordered,
            vec![
                "gpt-5".to_string(),
                "gpt-5-mini".to_string(),
                "o3".to_string(),
                "gpt-4.1".to_string(),
                "gpt-4o-mini".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn test_fetch_ollama_models_unreachable_fallback() {
        // Point at a port nothing listens on
        let models = fetch_ollama_models("http://127.0.0.1:1").await;
        assert!(!models.is_empty(), "should fall back to static defaults");
    }

    #[tokio::test]
    async fn test_discover_wasm_channels_empty_dir() {
        let dir = tempdir().unwrap();
        let channels = discover_wasm_channels(dir.path()).await;
        assert!(channels.is_empty());
    }

    #[tokio::test]
    async fn test_discover_wasm_channels_nonexistent_dir() {
        let channels =
            discover_wasm_channels(std::path::Path::new("/tmp/enclagent_nonexistent_dir")).await;
        assert!(channels.is_empty());
    }

    /// RAII guard that sets/clears an env var for the duration of a test.
    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn clear(key: &'static str) -> Self {
            let original = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, original }
        }

        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(ref val) = self.original {
                    std::env::set_var(self.key, val);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }
}
