//! libSQL/Turso backend for the Database trait.
//!
//! Provides an embedded SQLite-compatible database using Turso's libSQL fork.
//! Supports three modes:
//! - Local embedded (file-based, no server needed)
//! - Turso cloud with embedded replica (sync to cloud)
//! - In-memory (for testing)

mod conversations;
mod jobs;
mod routines;
mod sandbox;
mod settings;
mod tool_failures;
mod workspace;

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use libsql::{Connection, Database as LibSqlDatabase};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::agent::{
    IntentAuditRecord, VerificationStatus,
    routine::{
        NotifyConfig, Routine, RoutineAction, RoutineGuardrails, RoutineRun, RunStatus, Trigger,
    },
};
use crate::context::JobState;
use crate::db::{Database, IntentAuditStore};
use crate::error::DatabaseError;
use crate::workspace::MemoryDocument;

use crate::db::libsql_migrations;
use serde_json::json;

/// Explicit column list for routines table (matches positional access in `row_to_routine_libsql`).
pub(crate) const ROUTINE_COLUMNS: &str = "\
    id, name, description, user_id, enabled, \
    trigger_type, trigger_config, action_type, action_config, \
    cooldown_secs, max_concurrent, dedup_window_secs, \
    notify_channel, notify_user, notify_on_success, notify_on_failure, notify_on_attention, \
    state, last_run_at, next_fire_at, run_count, consecutive_failures, \
    created_at, updated_at";

/// Explicit column list for routine_runs table (matches positional access in `row_to_routine_run_libsql`).
pub(crate) const ROUTINE_RUN_COLUMNS: &str = "\
    id, routine_id, trigger_type, trigger_detail, started_at, \
    status, completed_at, result_summary, tokens_used, job_id, created_at";

/// libSQL/Turso database backend.
///
/// Stores the `Database` handle in an `Arc` so that the same underlying
/// database can be shared with stores (SecretsStore, WasmToolStore) that
/// create their own connections per-operation.
pub struct LibSqlBackend {
    db: Arc<LibSqlDatabase>,
}

impl LibSqlBackend {
    /// Create a new local embedded database.
    pub async fn new_local(path: &Path) -> Result<Self, DatabaseError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DatabaseError::Pool(format!("Failed to create database directory: {}", e))
            })?;
        }

        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .map_err(|e| DatabaseError::Pool(format!("Failed to open libSQL database: {}", e)))?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Create a new in-memory database (for testing).
    pub async fn new_memory() -> Result<Self, DatabaseError> {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .map_err(|e| {
                DatabaseError::Pool(format!("Failed to create in-memory database: {}", e))
            })?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Create with Turso cloud sync (embedded replica).
    pub async fn new_remote_replica(
        path: &Path,
        url: &str,
        auth_token: &str,
    ) -> Result<Self, DatabaseError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DatabaseError::Pool(format!("Failed to create database directory: {}", e))
            })?;
        }

        let db = libsql::Builder::new_remote_replica(path, url.to_string(), auth_token.to_string())
            .build()
            .await
            .map_err(|e| DatabaseError::Pool(format!("Failed to open remote replica: {}", e)))?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Get a shared reference to the underlying database handle.
    ///
    /// Use this to pass the database to stores (SecretsStore, WasmToolStore)
    /// that need to create their own connections per-operation.
    pub fn shared_db(&self) -> Arc<LibSqlDatabase> {
        Arc::clone(&self.db)
    }

    /// Create a new connection to the database.
    ///
    /// Sets `PRAGMA busy_timeout = 5000` on every connection so concurrent
    /// writers wait up to 5 seconds instead of failing instantly with
    /// "database is locked".
    pub async fn connect(&self) -> Result<Connection, DatabaseError> {
        let conn = self
            .db
            .connect()
            .map_err(|e| DatabaseError::Pool(format!("Failed to create connection: {}", e)))?;
        conn.query("PRAGMA busy_timeout = 5000", ())
            .await
            .map_err(|e| DatabaseError::Pool(format!("Failed to set busy_timeout: {}", e)))?;
        Ok(conn)
    }

    async fn ensure_intent_audit_table(&self, conn: &mut Connection) -> Result<(), DatabaseError> {
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS intent_audit_records (
                intent_id TEXT PRIMARY KEY,
                agent_id TEXT,
                user_id TEXT NOT NULL,
                signal_hash TEXT,
                intent_hash TEXT NOT NULL,
                receipt_id TEXT NOT NULL,
                receipt_hash TEXT NOT NULL,
                verification_id TEXT,
                verification_hash TEXT,
                verification_status TEXT,
                settlement_id TEXT,
                settlement_hash TEXT,
                provider_attributions TEXT NOT NULL DEFAULT '[]',
                mirrored_pnl_usd TEXT,
                revenue_share_fee_usd TEXT,
                chain_hash TEXT NOT NULL,
                workspace_path TEXT NOT NULL,
                created_at TEXT NOT NULL
            )
            "#,
            (),
        )
        .await?;
        for stmt in [
            "ALTER TABLE intent_audit_records ADD COLUMN signal_hash TEXT",
            "ALTER TABLE intent_audit_records ADD COLUMN settlement_id TEXT",
            "ALTER TABLE intent_audit_records ADD COLUMN settlement_hash TEXT",
            "ALTER TABLE intent_audit_records ADD COLUMN provider_attributions TEXT NOT NULL DEFAULT '[]'",
            "ALTER TABLE intent_audit_records ADD COLUMN mirrored_pnl_usd TEXT",
            "ALTER TABLE intent_audit_records ADD COLUMN revenue_share_fee_usd TEXT",
        ] {
            if let Err(e) = conn.execute(stmt, ()).await {
                // Existing deployments may already have these columns.
                if !e
                    .to_string()
                    .to_lowercase()
                    .contains("duplicate column name")
                {
                    return Err(DatabaseError::Query(format!(
                        "failed intent_audit_records schema upgrade: {} ({})",
                        stmt, e
                    )));
                }
            }
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_intent_audit_user ON intent_audit_records(user_id)",
            (),
        )
        .await?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_intent_audit_chain ON intent_audit_records(chain_hash)",
            (),
        )
        .await?;
        Ok(())
    }
}

// ==================== Helper functions ====================

/// Parse an ISO-8601 timestamp string from SQLite into DateTime<Utc>.
///
/// Tries multiple formats in order:
/// 1. RFC 3339 with timezone (e.g. `2024-01-15T10:30:00.123Z`)
/// 2. Naive datetime with fractional seconds (e.g. `2024-01-15 10:30:00.123`)
/// 3. Naive datetime without fractional seconds (e.g. `2024-01-15 10:30:00`)
///
/// Returns an error if none of the formats match.
pub(crate) fn parse_timestamp(s: &str) -> Result<DateTime<Utc>, String> {
    // RFC 3339 (our canonical write format)
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Naive with fractional seconds (legacy or SQLite datetime() output)
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(ndt.and_utc());
    }
    // Naive without fractional seconds (legacy format)
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(ndt.and_utc());
    }
    Err(format!("unparseable timestamp: {:?}", s))
}

/// Format a DateTime<Utc> for SQLite storage (RFC 3339 with millisecond precision).
pub(crate) fn fmt_ts(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Format an optional DateTime<Utc>.
pub(crate) fn fmt_opt_ts(dt: &Option<DateTime<Utc>>) -> libsql::Value {
    match dt {
        Some(dt) => libsql::Value::Text(fmt_ts(dt)),
        None => libsql::Value::Null,
    }
}

pub(crate) fn parse_job_state(s: &str) -> JobState {
    match s {
        "pending" => JobState::Pending,
        "in_progress" => JobState::InProgress,
        "completed" => JobState::Completed,
        "submitted" => JobState::Submitted,
        "accepted" => JobState::Accepted,
        "failed" => JobState::Failed,
        "stuck" => JobState::Stuck,
        "cancelled" => JobState::Cancelled,
        _ => JobState::Pending,
    }
}

fn parse_uuid_opt(value: Option<String>) -> Result<Option<Uuid>, DatabaseError> {
    match value {
        Some(text) => Ok(Some(
            Uuid::parse_str(&text).map_err(|e| DatabaseError::Query(e.to_string()))?,
        )),
        None => Ok(None),
    }
}

fn row_to_intent_audit_record(row: &libsql::Row) -> Result<IntentAuditRecord, DatabaseError> {
    let intent_id =
        Uuid::parse_str(&get_text(row, 0)).map_err(|e| DatabaseError::Query(e.to_string()))?;
    let agent_id = parse_uuid_opt(get_opt_text(row, 1))?;
    let user_id = get_text(row, 2);
    let signal_hash = get_opt_text(row, 3);
    let intent_hash = get_text(row, 4);
    let receipt_id =
        Uuid::parse_str(&get_text(row, 5)).map_err(|e| DatabaseError::Query(e.to_string()))?;
    let receipt_hash = get_text(row, 6);
    let verification_id = parse_uuid_opt(get_opt_text(row, 7))?;
    let verification_hash = get_opt_text(row, 8);
    let verification_status =
        get_opt_text(row, 9).and_then(|value| VerificationStatus::parse(&value));
    let settlement_id = parse_uuid_opt(get_opt_text(row, 10))?;
    let settlement_hash = get_opt_text(row, 11);
    let provider_attributions = get_opt_text(row, 12)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let mirrored_pnl_usd = get_opt_text(row, 13);
    let revenue_share_fee_usd = get_opt_text(row, 14);
    let chain_hash = get_text(row, 15);
    let workspace_path = get_text(row, 16);
    let created_at = get_ts(row, 17);

    Ok(IntentAuditRecord {
        intent_id,
        agent_id,
        user_id,
        signal_hash,
        intent_hash,
        receipt_id,
        receipt_hash,
        verification_id,
        verification_hash,
        verification_status,
        settlement_id,
        settlement_hash,
        provider_attributions,
        mirrored_pnl_usd,
        revenue_share_fee_usd,
        workspace_path,
        chain_hash,
        created_at,
    })
}

/// Extract a text column from a libsql Row, returning empty string for NULL.
pub(crate) fn get_text(row: &libsql::Row, idx: i32) -> String {
    row.get::<String>(idx).unwrap_or_default()
}

/// Extract an optional text column.
/// Returns None for SQL NULL, preserves empty strings as Some("").
pub(crate) fn get_opt_text(row: &libsql::Row, idx: i32) -> Option<String> {
    row.get::<String>(idx).ok()
}

/// Convert an `Option<&str>` to a `libsql::Value` (Text or Null).
/// Use this instead of `.unwrap_or("")` to preserve NULL semantics.
pub(crate) fn opt_text(s: Option<&str>) -> libsql::Value {
    match s {
        Some(s) => libsql::Value::Text(s.to_string()),
        None => libsql::Value::Null,
    }
}

/// Convert an `Option<String>` to a `libsql::Value` (Text or Null).
pub(crate) fn opt_text_owned(s: Option<String>) -> libsql::Value {
    match s {
        Some(s) => libsql::Value::Text(s),
        None => libsql::Value::Null,
    }
}

/// Extract an i64 column, defaulting to 0.
pub(crate) fn get_i64(row: &libsql::Row, idx: i32) -> i64 {
    row.get::<i64>(idx).unwrap_or(0)
}

/// Extract an optional bool from an integer column.
pub(crate) fn get_opt_bool(row: &libsql::Row, idx: i32) -> Option<bool> {
    row.get::<i64>(idx).ok().map(|v| v != 0)
}

/// Parse a Decimal from a text column.
pub(crate) fn get_decimal(row: &libsql::Row, idx: i32) -> Decimal {
    row.get::<String>(idx)
        .ok()
        .and_then(|s| s.parse::<Decimal>().ok())
        .unwrap_or_default()
}

/// Parse an optional Decimal from a text column.
pub(crate) fn get_opt_decimal(row: &libsql::Row, idx: i32) -> Option<Decimal> {
    row.get::<String>(idx)
        .ok()
        .and_then(|s| s.parse::<Decimal>().ok())
}

/// Parse a JSON value from a text column.
pub(crate) fn get_json(row: &libsql::Row, idx: i32) -> serde_json::Value {
    row.get::<String>(idx)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Parse a timestamp from a text column.
///
/// If the column is NULL or the value cannot be parsed, logs a warning and
/// returns the Unix epoch (1970-01-01T00:00:00Z) so the error is detectable
/// rather than silently replaced by the current time.
pub(crate) fn get_ts(row: &libsql::Row, idx: i32) -> DateTime<Utc> {
    match row.get::<String>(idx) {
        Ok(s) => match parse_timestamp(&s) {
            Ok(dt) => dt,
            Err(e) => {
                tracing::warn!("Timestamp parse failure at column {}: {}", idx, e);
                DateTime::UNIX_EPOCH
            }
        },
        Err(_) => DateTime::UNIX_EPOCH,
    }
}

/// Parse an optional timestamp from a text column.
///
/// Returns None if the column is NULL. Logs a warning and returns None if the
/// value is present but cannot be parsed.
pub(crate) fn get_opt_ts(row: &libsql::Row, idx: i32) -> Option<DateTime<Utc>> {
    match row.get::<String>(idx) {
        Ok(s) if s.is_empty() => None,
        Ok(s) => match parse_timestamp(&s) {
            Ok(dt) => Some(dt),
            Err(e) => {
                tracing::warn!("Timestamp parse failure at column {}: {}", idx, e);
                None
            }
        },
        Err(_) => None,
    }
}

#[async_trait]
impl Database for LibSqlBackend {
    async fn run_migrations(&self) -> Result<(), DatabaseError> {
        let conn = self.connect().await?;
        // WAL mode persists in the database file: all future connections benefit.
        // Readers no longer block writers and vice versa.
        conn.query("PRAGMA journal_mode=WAL", ())
            .await
            .map_err(|e| DatabaseError::Migration(format!("Failed to enable WAL mode: {}", e)))?;
        conn.execute_batch(libsql_migrations::SCHEMA)
            .await
            .map_err(|e| DatabaseError::Migration(format!("libSQL migration failed: {}", e)))?;
        Ok(())
    }
}

// ==================== Row conversion helpers ====================

pub(crate) fn row_to_memory_document(row: &libsql::Row) -> MemoryDocument {
    MemoryDocument {
        id: get_text(row, 0).parse().unwrap_or_default(),
        user_id: get_text(row, 1),
        agent_id: get_opt_text(row, 2).and_then(|s| s.parse().ok()),
        path: get_text(row, 3),
        content: get_text(row, 4),
        created_at: get_ts(row, 5),
        updated_at: get_ts(row, 6),
        metadata: get_json(row, 7),
    }
}

pub(crate) fn row_to_routine_libsql(row: &libsql::Row) -> Result<Routine, DatabaseError> {
    let trigger_type = get_text(row, 5);
    let trigger_config = get_json(row, 6);
    let action_type = get_text(row, 7);
    let action_config = get_json(row, 8);
    let cooldown_secs = get_i64(row, 9);
    let max_concurrent = get_i64(row, 10);
    let dedup_window_secs: Option<i64> = row.get::<i64>(11).ok();

    let trigger =
        Trigger::from_db(&trigger_type, trigger_config).map_err(DatabaseError::Serialization)?;
    let action = RoutineAction::from_db(&action_type, action_config)
        .map_err(DatabaseError::Serialization)?;

    Ok(Routine {
        id: get_text(row, 0).parse().unwrap_or_default(),
        name: get_text(row, 1),
        description: get_text(row, 2),
        user_id: get_text(row, 3),
        enabled: get_i64(row, 4) != 0,
        trigger,
        action,
        guardrails: RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(cooldown_secs as u64),
            max_concurrent: max_concurrent as u32,
            dedup_window: dedup_window_secs.map(|s| std::time::Duration::from_secs(s as u64)),
        },
        notify: NotifyConfig {
            channel: get_opt_text(row, 12),
            user: get_text(row, 13),
            on_success: get_i64(row, 14) != 0,
            on_failure: get_i64(row, 15) != 0,
            on_attention: get_i64(row, 16) != 0,
        },
        state: get_json(row, 17),
        last_run_at: get_opt_ts(row, 18),
        next_fire_at: get_opt_ts(row, 19),
        run_count: get_i64(row, 20) as u64,
        consecutive_failures: get_i64(row, 21) as u32,
        created_at: get_ts(row, 22),
        updated_at: get_ts(row, 23),
    })
}

pub(crate) fn row_to_routine_run_libsql(row: &libsql::Row) -> Result<RoutineRun, DatabaseError> {
    let status_str = get_text(row, 5);
    let status: RunStatus = status_str
        .parse()
        .map_err(|e: String| DatabaseError::Serialization(e))?;

    Ok(RoutineRun {
        id: get_text(row, 0).parse().unwrap_or_default(),
        routine_id: get_text(row, 1).parse().unwrap_or_default(),
        trigger_type: get_text(row, 2),
        trigger_detail: get_opt_text(row, 3),
        started_at: get_ts(row, 4),
        completed_at: get_opt_ts(row, 6),
        status,
        result_summary: get_opt_text(row, 7),
        tokens_used: row.get::<i64>(8).ok().map(|v| v as i32),
        job_id: get_opt_text(row, 9).and_then(|s| s.parse().ok()),
        created_at: get_ts(row, 10),
    })
}

#[async_trait]
impl IntentAuditStore for LibSqlBackend {
    async fn persist_intent_audit_record(
        &self,
        record: &IntentAuditRecord,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.connect().await?;
        self.ensure_intent_audit_table(&mut conn).await?;

        let verification_status = record.verification_status.map(|status| status.as_str());
        conn.execute(
            r#"
            INSERT INTO intent_audit_records (
                intent_id,
                agent_id,
                user_id,
                signal_hash,
                intent_hash,
                receipt_id,
                receipt_hash,
                verification_id,
                verification_hash,
                verification_status,
                settlement_id,
                settlement_hash,
                provider_attributions,
                mirrored_pnl_usd,
                revenue_share_fee_usd,
                chain_hash,
                workspace_path,
                created_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
            ON CONFLICT (intent_id) DO UPDATE SET
                agent_id = excluded.agent_id,
                signal_hash = excluded.signal_hash,
                intent_hash = excluded.intent_hash,
                receipt_id = excluded.receipt_id,
                receipt_hash = excluded.receipt_hash,
                verification_id = excluded.verification_id,
                verification_hash = excluded.verification_hash,
                verification_status = excluded.verification_status,
                settlement_id = excluded.settlement_id,
                settlement_hash = excluded.settlement_hash,
                provider_attributions = excluded.provider_attributions,
                mirrored_pnl_usd = excluded.mirrored_pnl_usd,
                revenue_share_fee_usd = excluded.revenue_share_fee_usd,
                chain_hash = excluded.chain_hash,
                workspace_path = excluded.workspace_path,
                created_at = excluded.created_at
            "#,
            libsql::params![
                record.intent_id.to_string(),
                record.agent_id.map(|id| id.to_string()),
                record.user_id.as_str(),
                record.signal_hash.as_deref(),
                record.intent_hash.as_str(),
                record.receipt_id.to_string(),
                record.receipt_hash.as_str(),
                record.verification_id.map(|id| id.to_string()),
                record.verification_hash.as_deref(),
                verification_status,
                record.settlement_id.map(|id| id.to_string()),
                record.settlement_hash.as_deref(),
                serde_json::to_string(&record.provider_attributions)
                    .unwrap_or_else(|_| "[]".to_string()),
                record.mirrored_pnl_usd.as_deref(),
                record.revenue_share_fee_usd.as_deref(),
                record.chain_hash.as_str(),
                record.workspace_path.as_str(),
                fmt_ts(&record.created_at),
            ],
        )
        .await?;

        let settings_payload = json!({
            "intent_id": record.intent_id,
            "receipt_id": record.receipt_id,
            "chain_hash": record.chain_hash,
            "workspace_path": record.workspace_path,
            "verification_status": verification_status,
            "signal_hash": record.signal_hash,
            "settlement_id": record.settlement_id,
            "settlement_hash": record.settlement_hash,
            "provider_attributions": record.provider_attributions,
            "mirrored_pnl_usd": record.mirrored_pnl_usd,
            "revenue_share_fee_usd": record.revenue_share_fee_usd,
            "created_at": record.created_at,
        });

        conn.execute(
            r#"
            INSERT INTO settings (user_id, key, value, updated_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT (user_id, key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
            libsql::params![
                record.user_id.as_str(),
                IntentAuditRecord::SETTINGS_KEY,
                settings_payload.to_string(),
                fmt_ts(&Utc::now()),
            ],
        )
        .await?;

        Ok(())
    }

    async fn get_intent_audit_record(
        &self,
        intent_id: Uuid,
    ) -> Result<Option<IntentAuditRecord>, DatabaseError> {
        let mut conn = self.connect().await?;
        self.ensure_intent_audit_table(&mut conn).await?;

        let mut rows = conn
            .query(
                r#"
                SELECT intent_id, agent_id, user_id, signal_hash, intent_hash, receipt_id, receipt_hash,
                       verification_id, verification_hash, verification_status,
                       settlement_id, settlement_hash, provider_attributions,
                       mirrored_pnl_usd, revenue_share_fee_usd,
                       chain_hash, workspace_path, created_at
                FROM intent_audit_records
                WHERE intent_id = ?1
                "#,
                libsql::params![intent_id.to_string()],
            )
            .await?;

        match rows.next().await {
            Ok(Some(row)) => Ok(Some(row_to_intent_audit_record(&row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(DatabaseError::Query(e.to_string())),
        }
    }

    async fn list_intent_audit_records(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<IntentAuditRecord>, DatabaseError> {
        let mut conn = self.connect().await?;
        self.ensure_intent_audit_table(&mut conn).await?;

        let mut rows = conn
            .query(
                r#"
                SELECT intent_id, agent_id, user_id, signal_hash, intent_hash, receipt_id, receipt_hash,
                       verification_id, verification_hash, verification_status,
                       settlement_id, settlement_hash, provider_attributions,
                       mirrored_pnl_usd, revenue_share_fee_usd,
                       chain_hash, workspace_path, created_at
                FROM intent_audit_records
                WHERE user_id = ?1
                ORDER BY created_at DESC
                LIMIT ?2
                "#,
                libsql::params![user_id, limit],
            )
            .await?;

        let mut records = Vec::new();
        loop {
            match rows.next().await {
                Ok(Some(row)) => records.push(row_to_intent_audit_record(&row)?),
                Ok(None) => break,
                Err(e) => return Err(DatabaseError::Query(e.to_string())),
            }
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::db::libsql::LibSqlBackend;

    #[tokio::test]
    async fn test_wal_mode_after_migrations() {
        let backend = LibSqlBackend::new_memory().await.unwrap();
        backend.run_migrations().await.unwrap();

        let conn = backend.connect().await.unwrap();
        let mut rows = conn.query("PRAGMA journal_mode", ()).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let mode: String = row.get(0).unwrap();
        // In-memory databases use "memory" journal mode (WAL doesn't apply to :memory:),
        // but the PRAGMA still executes without error. For file-based databases it returns "wal".
        assert!(
            mode == "wal" || mode == "memory",
            "expected wal or memory, got: {}",
            mode,
        );
    }

    #[tokio::test]
    async fn test_busy_timeout_set_on_connect() {
        let backend = LibSqlBackend::new_memory().await.unwrap();
        backend.run_migrations().await.unwrap();

        let conn = backend.connect().await.unwrap();
        let mut rows = conn.query("PRAGMA busy_timeout", ()).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let timeout: i64 = row.get(0).unwrap();
        assert_eq!(timeout, 5000);
    }

    #[tokio::test]
    async fn test_concurrent_writes_succeed() {
        // Use a temp file so connections share state (in-memory DBs are connection-local)
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_concurrent.db");
        let backend = LibSqlBackend::new_local(&db_path).await.unwrap();
        backend.run_migrations().await.unwrap();

        // Spawn 20 concurrent inserts into the conversations table
        let mut handles = Vec::new();
        for i in 0..20 {
            let conn = backend.connect().await.unwrap();
            let handle = tokio::spawn(async move {
                let id = uuid::Uuid::new_v4().to_string();
                let val = format!("ch_{}", i);
                conn.execute(
                    "INSERT INTO conversations (id, channel, user_id) VALUES (?1, ?2, ?3)",
                    libsql::params![id, val, "test_user"],
                )
                .await
            });
            handles.push(handle);
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(
                result.is_ok(),
                "concurrent write failed: {:?}",
                result.err()
            );
        }

        // Verify all 20 rows landed
        let conn = backend.connect().await.unwrap();
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM conversations WHERE user_id = ?1",
                libsql::params!["test_user"],
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let count: i64 = row.get(0).unwrap();
        assert_eq!(count, 20);
    }
}
