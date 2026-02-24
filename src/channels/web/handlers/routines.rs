//! Routine management API handlers.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::channels::IncomingMessage;
use crate::channels::web::server::GatewayState;
use crate::channels::web::types::*;

pub async fn routines_list_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<RoutineListResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routines = store
        .list_routines(&state.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<RoutineInfo> = routines.iter().map(routine_to_info).collect();

    Ok(Json(RoutineListResponse { routines: items }))
}

pub async fn routines_summary_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<RoutineSummaryResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routines = store
        .list_routines(&state.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let total = routines.len() as u64;
    let enabled = routines.iter().filter(|r| r.enabled).count() as u64;
    let disabled = total - enabled;
    let failing = routines
        .iter()
        .filter(|r| r.consecutive_failures > 0)
        .count() as u64;

    let today_start = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc());
    let runs_today = if let Some(start) = today_start {
        routines
            .iter()
            .filter(|r| r.last_run_at.is_some_and(|ts| ts >= start))
            .count() as u64
    } else {
        0
    };

    Ok(Json(RoutineSummaryResponse {
        total,
        enabled,
        disabled,
        failing,
        runs_today,
    }))
}

pub async fn routines_detail_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<RoutineDetailResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routine_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid routine ID".to_string()))?;

    let routine = store
        .get_routine(routine_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Routine not found".to_string()))?;

    let runs = store
        .list_routine_runs(routine_id, 20)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let recent_runs: Vec<RoutineRunInfo> = runs
        .iter()
        .map(|run| RoutineRunInfo {
            id: run.id,
            trigger_type: run.trigger_type.clone(),
            started_at: run.started_at.to_rfc3339(),
            completed_at: run.completed_at.map(|dt| dt.to_rfc3339()),
            status: format!("{:?}", run.status),
            result_summary: run.result_summary.clone(),
            tokens_used: run.tokens_used,
        })
        .collect();

    let webhook_path = crate::agent::routine_engine::routine_webhook_path(routine.id, &routine.trigger);
    let webhook_secret_configured = match &routine.trigger {
        crate::agent::routine::Trigger::Webhook { secret, .. } => Some(secret.is_some()),
        _ => None,
    };

    Ok(Json(RoutineDetailResponse {
        id: routine.id,
        name: routine.name.clone(),
        description: routine.description.clone(),
        enabled: routine.enabled,
        trigger: serde_json::to_value(&routine.trigger).unwrap_or_default(),
        action: serde_json::to_value(&routine.action).unwrap_or_default(),
        guardrails: serde_json::to_value(&routine.guardrails).unwrap_or_default(),
        notify: serde_json::to_value(&routine.notify).unwrap_or_default(),
        last_run_at: routine.last_run_at.map(|dt| dt.to_rfc3339()),
        next_fire_at: routine.next_fire_at.map(|dt| dt.to_rfc3339()),
        run_count: routine.run_count,
        consecutive_failures: routine.consecutive_failures,
        status: crate::agent::routine_engine::routine_status_label(&routine).to_string(),
        health: crate::agent::routine_engine::routine_health_label(&routine).to_string(),
        created_at: routine.created_at.to_rfc3339(),
        trigger_channel: crate::agent::routine_engine::routine_trigger_channel(&routine.trigger),
        webhook_path,
        webhook_secret_configured,
        recent_runs,
    }))
}

pub async fn routines_visibility_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<RoutineTriggerVisibilityResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routines = store
        .list_routines(&state.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(build_routine_visibility_response(&routines)))
}

pub async fn routines_trigger_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routine_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid routine ID".to_string()))?;

    let routine = store
        .get_routine(routine_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Routine not found".to_string()))?;

    // Send the routine prompt through the message pipeline as a manual trigger.
    let prompt = match &routine.action {
        crate::agent::routine::RoutineAction::Lightweight { prompt, .. } => prompt.clone(),
        crate::agent::routine::RoutineAction::FullJob {
            title, description, ..
        } => format!("{}: {}", title, description),
    };

    let content = format!("[routine:{}] {}", routine.name, prompt);
    let msg = IncomingMessage::new("gateway", &state.user_id, content);

    let tx_guard = state.msg_tx.read().await;
    let tx = tx_guard.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Channel not started".to_string(),
    ))?;

    tx.send(msg).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Channel closed".to_string(),
        )
    })?;

    Ok(Json(serde_json::json!({
        "status": "triggered",
        "routine_id": routine_id,
    })))
}

#[derive(Deserialize)]
pub struct ToggleRequest {
    pub enabled: Option<bool>,
}

pub async fn routines_toggle_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    body: Option<Json<ToggleRequest>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routine_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid routine ID".to_string()))?;

    let mut routine = store
        .get_routine(routine_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Routine not found".to_string()))?;

    // If a specific value was provided, use it; otherwise toggle.
    routine.enabled = match body {
        Some(Json(req)) => req.enabled.unwrap_or(!routine.enabled),
        None => !routine.enabled,
    };

    store
        .update_routine(&routine)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": if routine.enabled { "enabled" } else { "disabled" },
        "routine_id": routine_id,
    })))
}

pub async fn routines_delete_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routine_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid routine ID".to_string()))?;

    let deleted = store
        .delete_routine(routine_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if deleted {
        Ok(Json(serde_json::json!({
            "status": "deleted",
            "routine_id": routine_id,
        })))
    } else {
        Err((StatusCode::NOT_FOUND, "Routine not found".to_string()))
    }
}

pub async fn routines_runs_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let routine_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid routine ID".to_string()))?;

    let runs = store
        .list_routine_runs(routine_id, 50)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let run_infos: Vec<RoutineRunInfo> = runs
        .iter()
        .map(|run| RoutineRunInfo {
            id: run.id,
            trigger_type: run.trigger_type.clone(),
            started_at: run.started_at.to_rfc3339(),
            completed_at: run.completed_at.map(|dt| dt.to_rfc3339()),
            status: format!("{:?}", run.status),
            result_summary: run.result_summary.clone(),
            tokens_used: run.tokens_used,
        })
        .collect();

    Ok(Json(serde_json::json!({
        "routine_id": routine_id,
        "runs": run_infos,
    })))
}

/// Convert a Routine to the trimmed RoutineInfo for list display.
fn routine_to_info(r: &crate::agent::routine::Routine) -> RoutineInfo {
    let (trigger_type, trigger_summary) = match &r.trigger {
        crate::agent::routine::Trigger::Cron { schedule } => {
            ("cron".to_string(), format!("cron: {}", schedule))
        }
        crate::agent::routine::Trigger::Event {
            pattern, channel, ..
        } => {
            let ch = channel.as_deref().unwrap_or("any");
            ("event".to_string(), format!("on {} /{}/", ch, pattern))
        }
        crate::agent::routine::Trigger::Webhook { path, .. } => {
            let p = path.as_deref().unwrap_or("/");
            ("webhook".to_string(), format!("webhook: {}", p))
        }
        crate::agent::routine::Trigger::Manual => ("manual".to_string(), "manual only".to_string()),
    };

    let action_type = match &r.action {
        crate::agent::routine::RoutineAction::Lightweight { .. } => "lightweight",
        crate::agent::routine::RoutineAction::FullJob { .. } => "full_job",
    };
    let webhook_path = crate::agent::routine_engine::routine_webhook_path(r.id, &r.trigger);
    let webhook_secret_configured = match &r.trigger {
        crate::agent::routine::Trigger::Webhook { secret, .. } => Some(secret.is_some()),
        _ => None,
    };

    RoutineInfo {
        id: r.id,
        name: r.name.clone(),
        description: r.description.clone(),
        enabled: r.enabled,
        trigger_type,
        trigger_summary,
        action_type: action_type.to_string(),
        last_run_at: r.last_run_at.map(|dt| dt.to_rfc3339()),
        next_fire_at: r.next_fire_at.map(|dt| dt.to_rfc3339()),
        run_count: r.run_count,
        consecutive_failures: r.consecutive_failures,
        status: crate::agent::routine_engine::routine_status_label(r).to_string(),
        health: crate::agent::routine_engine::routine_health_label(r).to_string(),
        trigger_channel: crate::agent::routine_engine::routine_trigger_channel(&r.trigger),
        webhook_path,
        webhook_secret_configured,
    }
}

#[derive(Default)]
struct ChannelRoutineAccumulator {
    total_routines: u64,
    enabled_routines: u64,
    failing_routines: u64,
    last_run_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn status_from_failures(enabled: u64, failing: u64) -> &'static str {
    if enabled == 0 {
        "idle"
    } else if failing == 0 {
        "healthy"
    } else if failing < enabled {
        "degraded"
    } else {
        "failing"
    }
}

fn build_routine_visibility_response(
    routines: &[crate::agent::routine::Routine],
) -> RoutineTriggerVisibilityResponse {
    let mut channels: BTreeMap<String, ChannelRoutineAccumulator> = BTreeMap::new();
    let mut webhook_routes = Vec::new();
    let mut webhook_total = 0u64;
    let mut webhook_enabled = 0u64;
    let mut webhook_with_secret = 0u64;
    let mut webhook_failing = 0u64;

    for routine in routines {
        if let Some(channel_name) = crate::agent::routine_engine::routine_trigger_channel(&routine.trigger) {
            let entry = channels.entry(channel_name).or_default();
            entry.total_routines += 1;
            if routine.enabled {
                entry.enabled_routines += 1;
            }
            if routine.enabled && routine.consecutive_failures > 0 {
                entry.failing_routines += 1;
            }
            if let Some(last_run_at) = routine.last_run_at
                && match entry.last_run_at {
                    Some(current) => last_run_at > current,
                    None => true,
                }
            {
                entry.last_run_at = Some(last_run_at);
            }
        }

        if let crate::agent::routine::Trigger::Webhook { secret, .. } = &routine.trigger {
            webhook_total += 1;
            if routine.enabled {
                webhook_enabled += 1;
            }
            if secret.is_some() {
                webhook_with_secret += 1;
            }
            if routine.enabled && routine.consecutive_failures > 0 {
                webhook_failing += 1;
            }

            webhook_routes.push(WebhookRouteInfo {
                routine_id: routine.id,
                routine_name: routine.name.clone(),
                path: crate::agent::routine_engine::routine_webhook_path(routine.id, &routine.trigger)
                    .unwrap_or_else(|| format!("/hooks/routine/{}", routine.id)),
                enabled: routine.enabled,
                secret_configured: secret.is_some(),
                status: crate::agent::routine_engine::routine_health_label(routine).to_string(),
            });
        }
    }

    let channel_rows = channels
        .into_iter()
        .map(|(channel, row)| {
            let healthy_routines = row.enabled_routines.saturating_sub(row.failing_routines);
            RoutineChannelHealth {
                channel,
                total_routines: row.total_routines,
                enabled_routines: row.enabled_routines,
                healthy_routines,
                failing_routines: row.failing_routines,
                status: status_from_failures(row.enabled_routines, row.failing_routines).to_string(),
                last_run_at: row.last_run_at.map(|dt| dt.to_rfc3339()),
            }
        })
        .collect();

    webhook_routes.sort_by(|a, b| a.path.cmp(&b.path).then(a.routine_name.cmp(&b.routine_name)));

    RoutineTriggerVisibilityResponse {
        generated_at: chrono::Utc::now().to_rfc3339(),
        channels: channel_rows,
        webhook: WebhookTriggerVisibility {
            total_routines: webhook_total,
            enabled_routines: webhook_enabled,
            with_secret: webhook_with_secret,
            status: if webhook_total == 0 {
                "none".to_string()
            } else {
                status_from_failures(webhook_enabled, webhook_failing).to_string()
            },
            routes: webhook_routes,
        },
    }
}
