//! Bootstrap helpers for Enclagent.
//!
//! Bootstrap env vars are persisted to disk before the database is available.
//! This includes database connection selection (`DATABASE_*`, `LIBSQL_*`) and
//! provider-selection vars needed before DB settings load (`LLM_BACKEND`,
//! `LLM_BASE_URL`, `OLLAMA_BASE_URL`).
//!
//! File: `~/.enclagent/.env` (standard dotenvy format)

use std::path::PathBuf;

use chrono::{DateTime, Utc};

/// Path to the Enclagent-specific `.env` file: `~/.enclagent/.env`.
pub fn enclagent_env_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".enclagent")
        .join(".env")
}

/// Load env vars from `~/.enclagent/.env` (in addition to the standard `.env`).
///
/// Call this **after** `dotenvy::dotenv()` so that the standard `./.env`
/// takes priority over `~/.enclagent/.env`. dotenvy never overwrites
/// existing env vars, so the effective priority is:
///
///   explicit env vars > `./.env` > `~/.enclagent/.env`
///
/// If `~/.enclagent/.env` doesn't exist but the legacy `bootstrap.json` does,
/// extracts `DATABASE_URL` from it and writes the `.env` file (one-time
/// upgrade from the old config format).
pub fn load_enclagent_env() {
    let path = enclagent_env_path();

    if !path.exists() {
        // One-time upgrade: extract DATABASE_URL from legacy bootstrap.json
        migrate_bootstrap_json_to_env(&path);
    }

    if path.exists() {
        let _ = dotenvy::from_path(&path);
    }
}

/// If `bootstrap.json` exists, pull `database_url` out of it and write `.env`.
fn migrate_bootstrap_json_to_env(env_path: &std::path::Path) {
    let enclagent_dir = env_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let bootstrap_path = enclagent_dir.join("bootstrap.json");

    if !bootstrap_path.exists() {
        return;
    }

    let content = match std::fs::read_to_string(&bootstrap_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    // Minimal parse: just grab database_url from the JSON
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };

    if let Some(url) = parsed.get("database_url").and_then(|v| v.as_str()) {
        if let Some(parent) = env_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("Warning: failed to create {}: {}", parent.display(), e);
            return;
        }
        if let Err(e) = std::fs::write(env_path, format!("DATABASE_URL=\"{}\"\n", url)) {
            eprintln!("Warning: failed to migrate bootstrap.json to .env: {}", e);
            return;
        }
        rename_to_migrated(&bootstrap_path);
        eprintln!(
            "Migrated DATABASE_URL from bootstrap.json to {}",
            env_path.display()
        );
    }
}

/// Write database bootstrap vars to `~/.enclagent/.env`.
///
/// These settings form the chicken-and-egg layer: they must be available
/// from the filesystem (env vars) BEFORE any database connection, because
/// they determine which database/provider startup should initialize before
/// DB settings are loaded.
///
/// Creates the parent directory if it doesn't exist.
/// Values are double-quoted so that `#` (common in URL-encoded passwords)
/// and other shell-special characters are preserved by dotenvy.
pub fn save_bootstrap_env(vars: &[(&str, &str)]) -> std::io::Result<()> {
    let path = enclagent_env_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = String::new();
    for (key, value) in vars {
        // Escape backslashes and double quotes to prevent env var injection
        // (e.g. a value containing `"\nINJECTED="x` would break out of quotes).
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        content.push_str(&format!("{}=\"{}\"\n", key, escaped));
    }
    std::fs::write(&path, content)
}

/// Write `DATABASE_URL` to `~/.enclagent/.env`.
///
/// Convenience wrapper around `save_bootstrap_env` for single-value migration
/// paths. Prefer `save_bootstrap_env` for new code.
pub fn save_database_url(url: &str) -> std::io::Result<()> {
    save_bootstrap_env(&[("DATABASE_URL", url)])
}

fn push_env_var(vars: &mut Vec<(&'static str, String)>, key: &'static str, value: Option<&str>) {
    if let Some(value) = value {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            vars.push((key, trimmed.to_string()));
        }
    }
}

fn bootstrap_env_vars_from_settings(
    settings: &crate::settings::Settings,
) -> Vec<(&'static str, String)> {
    let mut vars = Vec::new();
    push_env_var(
        &mut vars,
        "DATABASE_BACKEND",
        settings.database_backend.as_deref(),
    );
    push_env_var(&mut vars, "DATABASE_URL", settings.database_url.as_deref());
    push_env_var(&mut vars, "LIBSQL_PATH", settings.libsql_path.as_deref());
    push_env_var(&mut vars, "LIBSQL_URL", settings.libsql_url.as_deref());
    push_env_var(&mut vars, "LLM_BACKEND", settings.llm_backend.as_deref());
    push_env_var(
        &mut vars,
        "LLM_BASE_URL",
        settings.openai_compatible_base_url.as_deref(),
    );
    push_env_var(
        &mut vars,
        "OLLAMA_BASE_URL",
        settings.ollama_base_url.as_deref(),
    );
    if let Some(selected_model) = settings.selected_model.as_deref() {
        match settings.llm_backend.as_deref() {
            Some("ollama") => push_env_var(&mut vars, "OLLAMA_MODEL", Some(selected_model)),
            Some("openai_compatible") => push_env_var(&mut vars, "LLM_MODEL", Some(selected_model)),
            _ => {}
        }
    }
    vars
}

fn normalize_session_token(raw: &str) -> Result<String, String> {
    let token = raw.trim();
    if token.is_empty() {
        return Err("session token is empty".to_string());
    }
    Ok(token.to_string())
}

fn parse_created_at(value: Option<&serde_json::Value>) -> DateTime<Utc> {
    value
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

fn parse_session_data_from_value(
    value: serde_json::Value,
) -> Result<crate::llm::session::SessionData, String> {
    if let Ok(mut session) =
        serde_json::from_value::<crate::llm::session::SessionData>(value.clone())
    {
        session.session_token = normalize_session_token(&session.session_token)?;
        return Ok(session);
    }

    if let Some(token) = value.as_str() {
        return Ok(crate::llm::session::SessionData {
            session_token: normalize_session_token(token)?,
            created_at: Utc::now(),
            auth_provider: None,
        });
    }

    if let Some(obj) = value.as_object() {
        let token = obj
            .get("session_token")
            .or_else(|| obj.get("token"))
            .or_else(|| obj.get("sessionToken"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "session JSON object missing session_token/token field".to_string())?;

        return Ok(crate::llm::session::SessionData {
            session_token: normalize_session_token(token)?,
            created_at: parse_created_at(obj.get("created_at")),
            auth_provider: obj
                .get("auth_provider")
                .and_then(|v| v.as_str())
                .map(String::from),
        });
    }

    Err("unsupported session JSON format".to_string())
}

fn normalize_session_json_for_db(content: &str) -> Result<serde_json::Value, String> {
    let value: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("invalid JSON: {}", e))?;
    let session = parse_session_data_from_value(value)?;
    serde_json::to_value(&session).map_err(|e| format!("failed to serialize session: {}", e))
}

/// One-time migration of legacy `~/.enclagent/settings.json` into the database.
///
/// Only runs when a `settings.json` exists on disk AND the DB has no settings
/// yet. After the wizard writes directly to the DB, this path is only hit by
/// users upgrading from the old disk-only configuration.
///
/// After syncing, renames `settings.json` to `.migrated` so it won't trigger again.
pub async fn migrate_disk_to_db(
    store: &dyn crate::db::Database,
    user_id: &str,
) -> Result<(), MigrationError> {
    let enclagent_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".enclagent");
    let legacy_settings_path = enclagent_dir.join("settings.json");

    if !legacy_settings_path.exists() {
        tracing::debug!("No legacy settings.json found, skipping disk-to-DB migration");
        return Ok(());
    }

    // If DB already has settings, this is not a first boot, the wizard already
    // wrote directly to the DB. Just clean up the stale file.
    let has_settings = store.has_settings(user_id).await.map_err(|e| {
        MigrationError::Database(format!("Failed to check existing settings: {}", e))
    })?;
    if has_settings {
        tracing::info!("DB already has settings, renaming stale settings.json");
        rename_to_migrated(&legacy_settings_path);
        return Ok(());
    }

    tracing::info!("Migrating disk settings to database...");

    // 1. Load and migrate settings.json
    let settings = crate::settings::Settings::load_from(&legacy_settings_path);
    let db_map = settings.to_db_map();
    if !db_map.is_empty() {
        store
            .set_all_settings(user_id, &db_map)
            .await
            .map_err(|e| {
                MigrationError::Database(format!("Failed to write settings to DB: {}", e))
            })?;
        tracing::info!("Migrated {} settings to database", db_map.len());
    }

    // 2. Write bootstrap vars to ~/.enclagent/.env so startup can resolve
    // DB backend/provider before DB settings are loaded.
    let bootstrap_vars = bootstrap_env_vars_from_settings(&settings);
    if !bootstrap_vars.is_empty() {
        let pairs: Vec<(&str, &str)> = bootstrap_vars
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        save_bootstrap_env(&pairs)
            .map_err(|e| MigrationError::Io(format!("Failed to write .env: {}", e)))?;
        tracing::info!(
            "Wrote {} bootstrap vars to {}",
            bootstrap_vars.len(),
            enclagent_env_path().display()
        );
    }

    // 3. Migrate mcp-servers.json if it exists
    let mcp_path = enclagent_dir.join("mcp-servers.json");
    if mcp_path.exists() {
        match std::fs::read_to_string(&mcp_path) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(value) => {
                    store
                        .set_setting(user_id, "mcp_servers", &value)
                        .await
                        .map_err(|e| {
                            MigrationError::Database(format!(
                                "Failed to write MCP servers to DB: {}",
                                e
                            ))
                        })?;
                    tracing::info!("Migrated mcp-servers.json to database");

                    rename_to_migrated(&mcp_path);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse mcp-servers.json: {}", e);
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read mcp-servers.json: {}", e);
            }
        }
    }

    // 4. Migrate session.json if it exists
    let session_path = enclagent_dir.join("session.json");
    if session_path.exists() {
        match std::fs::read_to_string(&session_path) {
            Ok(content) => match normalize_session_json_for_db(&content) {
                Ok(value) => {
                    store
                        .set_setting(user_id, "nearai.session_token", &value)
                        .await
                        .map_err(|e| {
                            MigrationError::Database(format!(
                                "Failed to write session to DB: {}",
                                e
                            ))
                        })?;
                    tracing::info!("Migrated session.json to database");

                    rename_to_migrated(&session_path);
                }
                Err(e) => {
                    tracing::warn!("Failed to normalize session.json: {}", e);
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read session.json: {}", e);
            }
        }
    }

    // 5. Rename settings.json to .migrated (don't delete, safety net)
    rename_to_migrated(&legacy_settings_path);

    // 6. Clean up old bootstrap.json if it exists (superseded by .env)
    let old_bootstrap = enclagent_dir.join("bootstrap.json");
    if old_bootstrap.exists() {
        rename_to_migrated(&old_bootstrap);
        tracing::info!("Renamed old bootstrap.json to .migrated");
    }

    tracing::info!("Disk-to-DB migration complete");
    Ok(())
}

/// Rename a file to `<name>.migrated` as a safety net.
fn rename_to_migrated(path: &std::path::Path) {
    let mut migrated = path.as_os_str().to_owned();
    migrated.push(".migrated");
    if let Err(e) = std::fs::rename(path, &migrated) {
        tracing::warn!("Failed to rename {} to .migrated: {}", path.display(), e);
    }
}

/// Errors that can occur during disk-to-DB migration.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("IO error: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn test_save_and_load_database_url() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");

        // Write in the quoted format that save_database_url uses
        let url = "postgres://localhost:5432/enclagent_test";
        std::fs::write(&env_path, format!("DATABASE_URL=\"{}\"\n", url)).unwrap();

        // Verify the content is a valid dotenv line (quoted)
        let content = std::fs::read_to_string(&env_path).unwrap();
        assert_eq!(
            content,
            "DATABASE_URL=\"postgres://localhost:5432/enclagent_test\"\n"
        );

        // Verify dotenvy can parse it (strips quotes automatically)
        let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "DATABASE_URL");
        assert_eq!(parsed[0].1, url);
    }

    #[test]
    fn test_save_database_url_with_hash_in_password() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");

        // URLs with # in the password are common (URL-encoded special chars).
        // Without quoting, dotenvy treats # as a comment delimiter.
        let url = "postgres://user:p%23ss@localhost:5432/enclagent";
        std::fs::write(&env_path, format!("DATABASE_URL=\"{}\"\n", url)).unwrap();

        let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "DATABASE_URL");
        assert_eq!(parsed[0].1, url);
    }

    #[test]
    fn test_save_database_url_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("deep").join("nested");
        let env_path = nested.join(".env");

        // Parent doesn't exist yet
        assert!(!nested.exists());

        // The global function uses a fixed path, so we test the logic directly
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(&env_path, "DATABASE_URL=postgres://test\n").unwrap();

        assert!(env_path.exists());
        let content = std::fs::read_to_string(&env_path).unwrap();
        assert!(content.contains("DATABASE_URL=postgres://test"));
    }

    #[test]
    fn test_save_bootstrap_env_escapes_quotes() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");

        // A malicious URL attempting to inject a second env var
        let malicious = r#"http://evil.com"
INJECTED="pwned"#;
        let mut content = String::new();
        let escaped = malicious.replace('\\', "\\\\").replace('"', "\\\"");
        content.push_str(&format!("LLM_BASE_URL=\"{}\"\n", escaped));
        std::fs::write(&env_path, &content).unwrap();

        let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        // Must parse as exactly one variable, not two
        assert_eq!(parsed.len(), 1, "injection must not create extra vars");
        assert_eq!(parsed[0].0, "LLM_BASE_URL");
        // The value should contain the original malicious content (unescaped by dotenvy)
        assert!(
            parsed[0].1.contains("INJECTED"),
            "value should contain the literal injection attempt, not execute it"
        );
    }

    #[test]
    fn test_enclagent_env_path() {
        let path = enclagent_env_path();
        assert!(path.ends_with(".enclagent/.env"));
    }

    #[test]
    fn test_migrate_bootstrap_json_to_env() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");
        let bootstrap_path = dir.path().join("bootstrap.json");

        // Write a legacy bootstrap.json
        let bootstrap_json = serde_json::json!({
            "database_url": "postgres://localhost/enclagent_upgrade",
            "database_pool_size": 5,
            "secrets_master_key_source": "keychain",
            "onboard_completed": true
        });
        std::fs::write(
            &bootstrap_path,
            serde_json::to_string_pretty(&bootstrap_json).unwrap(),
        )
        .unwrap();

        assert!(!env_path.exists());
        assert!(bootstrap_path.exists());

        // Run the migration
        migrate_bootstrap_json_to_env(&env_path);

        // .env should now exist with DATABASE_URL
        assert!(env_path.exists());
        let content = std::fs::read_to_string(&env_path).unwrap();
        assert_eq!(
            content,
            "DATABASE_URL=\"postgres://localhost/enclagent_upgrade\"\n"
        );

        // bootstrap.json should be renamed to .migrated
        assert!(!bootstrap_path.exists());
        assert!(dir.path().join("bootstrap.json.migrated").exists());
    }

    #[test]
    fn test_migrate_bootstrap_json_no_database_url() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");
        let bootstrap_path = dir.path().join("bootstrap.json");

        // bootstrap.json with no database_url
        let bootstrap_json = serde_json::json!({
            "onboard_completed": false
        });
        std::fs::write(
            &bootstrap_path,
            serde_json::to_string_pretty(&bootstrap_json).unwrap(),
        )
        .unwrap();

        migrate_bootstrap_json_to_env(&env_path);

        // .env should NOT be created
        assert!(!env_path.exists());
        // bootstrap.json should remain (no migration happened)
        assert!(bootstrap_path.exists());
    }

    #[test]
    fn test_migrate_bootstrap_json_missing() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");

        // No bootstrap.json at all
        migrate_bootstrap_json_to_env(&env_path);

        // Nothing should happen
        assert!(!env_path.exists());
    }

    #[test]
    fn test_save_bootstrap_env_multiple_vars() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join("nested").join(".env");

        std::fs::create_dir_all(env_path.parent().unwrap()).unwrap();

        let vars = [
            ("DATABASE_BACKEND", "libsql"),
            ("LIBSQL_PATH", "/home/user/.enclagent/enclagent.db"),
        ];

        // Write manually to the temp path (save_bootstrap_env uses the global path)
        let mut content = String::new();
        for (key, value) in &vars {
            content.push_str(&format!("{}=\"{}\"\n", key, value));
        }
        std::fs::write(&env_path, &content).unwrap();

        // Verify dotenvy can parse all entries
        let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0],
            ("DATABASE_BACKEND".to_string(), "libsql".to_string())
        );
        assert_eq!(
            parsed[1],
            (
                "LIBSQL_PATH".to_string(),
                "/home/user/.enclagent/enclagent.db".to_string()
            )
        );
    }

    #[test]
    fn test_save_bootstrap_env_overwrites_previous() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");

        // Write initial content
        std::fs::write(&env_path, "DATABASE_URL=\"postgres://old\"\n").unwrap();

        // Overwrite with new vars (simulating save_bootstrap_env behavior)
        let content = "DATABASE_BACKEND=\"libsql\"\nLIBSQL_PATH=\"/new/path.db\"\n";
        std::fs::write(&env_path, content).unwrap();

        let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        // Old DATABASE_URL should be gone
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|(k, _)| k != "DATABASE_URL"));
    }

    #[test]
    fn test_bootstrap_env_vars_from_settings_includes_provider_fields() {
        let settings = crate::settings::Settings {
            database_backend: Some("libsql".to_string()),
            libsql_path: Some("/tmp/enclagent.db".to_string()),
            llm_backend: Some("openai_compatible".to_string()),
            openai_compatible_base_url: Some("https://example.ai/v1".to_string()),
            selected_model: Some("openai/gpt-5.1-codex".to_string()),
            ..Default::default()
        };

        let vars = bootstrap_env_vars_from_settings(&settings);
        let map: HashMap<&str, &str> = vars.iter().map(|(k, v)| (*k, v.as_str())).collect();

        assert_eq!(map.get("DATABASE_BACKEND"), Some(&"libsql"));
        assert_eq!(map.get("LIBSQL_PATH"), Some(&"/tmp/enclagent.db"));
        assert_eq!(map.get("LLM_BACKEND"), Some(&"openai_compatible"));
        assert_eq!(map.get("LLM_BASE_URL"), Some(&"https://example.ai/v1"));
        assert_eq!(map.get("LLM_MODEL"), Some(&"openai/gpt-5.1-codex"));
    }

    #[test]
    fn test_bootstrap_env_vars_from_settings_includes_ollama_model() {
        let settings = crate::settings::Settings {
            llm_backend: Some("ollama".to_string()),
            ollama_base_url: Some("http://localhost:11434".to_string()),
            selected_model: Some("glm-4.7-flash:latest".to_string()),
            ..Default::default()
        };

        let vars = bootstrap_env_vars_from_settings(&settings);
        let map: HashMap<&str, &str> = vars.iter().map(|(k, v)| (*k, v.as_str())).collect();

        assert_eq!(map.get("LLM_BACKEND"), Some(&"ollama"));
        assert_eq!(map.get("OLLAMA_BASE_URL"), Some(&"http://localhost:11434"));
        assert_eq!(map.get("OLLAMA_MODEL"), Some(&"glm-4.7-flash:latest"));
    }

    #[test]
    fn test_normalize_session_json_for_db_accepts_legacy_string() {
        let normalized = normalize_session_json_for_db("\"legacy_sess_123\"").unwrap();
        let parsed: crate::llm::session::SessionData = serde_json::from_value(normalized).unwrap();
        assert_eq!(parsed.session_token, "legacy_sess_123");
    }

    #[test]
    fn test_normalize_session_json_for_db_accepts_legacy_token_field() {
        let normalized = normalize_session_json_for_db(
            r#"{"token":"legacy_sess_456","auth_provider":"github"}"#,
        )
        .unwrap();
        let parsed: crate::llm::session::SessionData = serde_json::from_value(normalized).unwrap();
        assert_eq!(parsed.session_token, "legacy_sess_456");
        assert_eq!(parsed.auth_provider.as_deref(), Some("github"));
    }
}
