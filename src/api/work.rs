//! 作業のライフサイクル。占有と作業状態だけを変え、業務段階には決して触れない。

use axum::Json;
use axum::extract::{Path, State};
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::json;

use super::collab::{EvidenceInput, insert_comment, insert_link_artifact};
use super::tasks::task_event;
use super::{ApiJson, AppState, required_text};
use crate::auth::{Actor, load_grants};
use crate::db::{new_id, now};
use crate::error::{ApiError, ApiResult};
use crate::events::emit;
use crate::model::Permission;
use crate::tasks::{TaskRow, TaskView, load_task, refresh_claimable, resolve_task, to_view, with_dependents};

fn touch(conn: &Connection, task: &TaskRow) -> ApiResult<()> {
    conn.execute("UPDATE tasks SET updated_at = ?1 WHERE id = ?2", params![now(), task.id])?;
    Ok(())
}

fn finish(conn: &Connection, actor: &Actor, task_id: &str) -> ApiResult<Json<TaskView>> {
    refresh_claimable(conn, &actor.id, &with_dependents(conn, task_id)?)?;
    Ok(Json(to_view(&load_task(conn, task_id)?, actor)?))
}

/// 取得できないときは理由を全て返す。
fn ensure_claimable(task: &TaskRow) -> ApiResult<()> {
    let result = task.availability();
    if result.claimable {
        return Ok(());
    }
    let summary: Vec<&str> = result.reasons.iter().map(|r| r.message.as_str()).collect();
    Err(ApiError::conflict("not_claimable", summary.join("; "))
        .details(json!({ "reasons": result.reasons, "occupant": task.occupant })))
}

/// 現在の作業権を提示した本人であることを確かめる。交代後の古い作業権はここで拒否される。
fn require_claim(task: &TaskRow, actor: &Actor, claim_id: &str) -> ApiResult<()> {
    let holds = task.claim_id.as_deref() == Some(claim_id)
        && task.occupant.as_ref().is_some_and(|holder| holder.id == actor.id);
    if holds {
        return Ok(());
    }
    tracing::warn!(task = %task.id, actor = %actor.id, "rejected a stale claim");
    Err(ApiError::conflict("stale_claim", "this claim is no longer the current work right")
        .details(json!({ "occupant": task.occupant })))
}

fn end_claim(conn: &Connection, task: &TaskRow, actor: &Actor, reason: &str) -> ApiResult<()> {
    conn.execute(
        "UPDATE claims SET ended_at = ?1, end_reason = ?2, ended_by = ?3
         WHERE task_id = ?4 AND ended_at IS NULL",
        params![now(), reason, actor.id, task.id],
    )?;
    Ok(())
}

fn insert_claim(conn: &Connection, task: &TaskRow, holder_id: &str) -> ApiResult<String> {
    let id = new_id();
    conn.execute(
        "INSERT INTO claims (id, task_id, holder_id, activity, acquired_at) VALUES (?1, ?2, ?3, 'claimed', ?4)",
        params![id, task.id, holder_id, now()],
    )?;
    Ok(id)
}

/// 引継ぎ先が有効な主体で、現段階の仕事を取得する権限を持つことを確かめる。
fn require_transfer_target(conn: &Connection, task: &TaskRow, target_id: &str) -> ApiResult<()> {
    let target: Option<(String, String)> = rusqlite::OptionalExtension::optional(conn.query_row(
        "SELECT status, org_id FROM principals WHERE id = ?1",
        [target_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    match target {
        Some((status, org_id)) if status == "active" && org_id == task.org_id => {}
        _ => return Err(ApiError::invalid("the target is not an active principal of this organization")),
    }
    let permission = task.claim_permission()?;
    let allowed = load_grants(conn, target_id)?.iter().any(|grant| {
        grant.project_id.as_deref().is_none_or(|id| id == task.project_id)
            && (grant.permission == permission || grant.permission == Permission::Admin)
    });
    if allowed {
        Ok(())
    } else {
        Err(ApiError::invalid(format!(
            "the target lacks the '{}' permission for this stage",
            permission.as_str()
        )))
    }
}

pub async fn claim(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, task.claim_permission()?)?;
            ensure_claimable(&task)?;
            let claim_id = insert_claim(conn, &task, &actor.id)?;
            touch(conn, &task)?;
            emit(conn, task_event(&task, &actor, "work", "work.claimed", json!({
                "key": task.key(),
                "stage": task.stage.name,
                "holder_id": actor.id,
            })))?;
            tracing::info!(task = %task.id, actor = %actor.id, %claim_id, "work claimed");
            finish(conn, &actor, &task.id)
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRef {
    claim_id: String,
}

async fn change_activity(
    state: AppState,
    actor: Actor,
    reference: String,
    claim_id: String,
    from: &'static str,
    to: &'static str,
    event: &'static str,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            require_claim(&task, &actor, &claim_id)?;
            if task.activity.as_deref() != Some(from) {
                return Err(ApiError::conflict(
                    "invalid_activity_transition",
                    format!("activity must be '{from}' for this operation"),
                )
                .details(json!({ "activity": task.activity })));
            }
            conn.execute(
                "UPDATE claims SET activity = ?1 WHERE id = ?2",
                params![to, claim_id],
            )?;
            touch(conn, &task)?;
            emit(conn, task_event(&task, &actor, "work", event, json!({
                "key": task.key(),
                "from": from,
                "to": to,
            })))?;
            finish(conn, &actor, &task.id)
        })
        .await
}

pub async fn start(
    State(state): State<AppState>,
    actor: Actor,
    Path(task): Path<String>,
    ApiJson(request): ApiJson<ClaimRef>,
) -> ApiResult<Json<TaskView>> {
    change_activity(state, actor, task, request.claim_id, "claimed", "working", "work.started").await
}

pub async fn pause(
    State(state): State<AppState>,
    actor: Actor,
    Path(task): Path<String>,
    ApiJson(request): ApiJson<ClaimRef>,
) -> ApiResult<Json<TaskView>> {
    change_activity(state, actor, task, request.claim_id, "working", "paused", "work.paused").await
}

pub async fn resume(
    State(state): State<AppState>,
    actor: Actor,
    Path(task): Path<String>,
    ApiJson(request): ApiJson<ClaimRef>,
) -> ApiResult<Json<TaskView>> {
    change_activity(state, actor, task, request.claim_id, "paused", "working", "work.resumed").await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRequest {
    claim_id: String,
    /// 残課題や判断を残す引継ぎメモ
    note: Option<String>,
}

pub async fn release(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<ReleaseRequest>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            require_claim(&task, &actor, &request.claim_id)?;
            if let Some(note) = &request.note {
                let note = required_text("note", note, 20_000)?;
                insert_comment(conn, &task, &actor, "handoff", &note, None, None)?;
            }
            end_claim(conn, &task, &actor, "released")?;
            touch(conn, &task)?;
            emit(conn, task_event(&task, &actor, "work", "work.released", json!({
                "key": task.key(),
                "holder_id": actor.id,
            })))?;
            finish(conn, &actor, &task.id)
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferRequest {
    claim_id: String,
    to: String,
    note: String,
}

pub async fn transfer(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<TransferRequest>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            require_claim(&task, &actor, &request.claim_id)?;
            let note = required_text("note", &request.note, 20_000)?;
            require_transfer_target(conn, &task, &request.to)?;
            insert_comment(conn, &task, &actor, "handoff", &note, Some(&request.to), None)?;
            end_claim(conn, &task, &actor, "transferred")?;
            insert_claim(conn, &task, &request.to)?;
            touch(conn, &task)?;
            emit(conn, task_event(&task, &actor, "work", "work.transferred", json!({
                "key": task.key(),
                "from": actor.id,
                "to": request.to,
            })))?;
            finish(conn, &actor, &task.id)
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteRequest {
    claim_id: String,
    summary: String,
    #[serde(default)]
    evidence: Vec<EvidenceInput>,
}

/// 実施した仕事の完了報告。占有は終わるが、業務段階は変わらない。
pub async fn complete(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<CompleteRequest>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            require_claim(&task, &actor, &request.claim_id)?;
            let summary = required_text("summary", &request.summary, 20_000)?;
            insert_comment(conn, &task, &actor, "completion", &summary, None, None)?;
            for evidence in &request.evidence {
                insert_link_artifact(conn, &task, &actor, "artifact", evidence)?;
            }
            end_claim(conn, &task, &actor, "completed")?;
            conn.execute(
                "UPDATE tasks SET work_state = 'done', updated_at = ?1 WHERE id = ?2",
                params![now(), task.id],
            )?;
            emit(conn, task_event(&task, &actor, "work", "work.completed", json!({
                "key": task.key(),
                "stage": task.stage.name,
                "holder_id": actor.id,
                "summary": summary,
            })))?;
            finish(conn, &actor, &task.id)
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasonRequest {
    reason: String,
}

pub async fn reopen(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<ReasonRequest>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Edit)?;
            let reason = required_text("reason", &request.reason, 2_000)?;
            if task.work_state != "done" {
                return Err(ApiError::conflict("work_not_done", "only completed work can be reopened"));
            }
            conn.execute(
                "UPDATE tasks SET work_state = 'open', updated_at = ?1 WHERE id = ?2",
                params![now(), task.id],
            )?;
            emit(conn, task_event(&task, &actor, "work", "work.reopened", json!({
                "key": task.key(),
                "reason": reason,
            })))?;
            finish(conn, &actor, &task.id)
        })
        .await
}

fn reject_if_occupied(task: &TaskRow) -> ApiResult<()> {
    match &task.occupant {
        Some(holder) => Err(ApiError::conflict(
            "task_occupied",
            format!("{} holds this work", holder.display_name),
        )
        .details(json!({ "occupant": holder }))),
        None => Ok(()),
    }
}

pub async fn hold(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<ReasonRequest>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Edit)?;
            let reason = required_text("reason", &request.reason, 2_000)?;
            reject_if_occupied(&task)?;
            if task.work_state != "open" {
                return Err(ApiError::conflict("work_not_open", "only open work can be put on hold"));
            }
            conn.execute(
                "UPDATE tasks SET work_state = 'on_hold', hold_reason = ?1, updated_at = ?2 WHERE id = ?3",
                params![reason, now(), task.id],
            )?;
            emit(conn, task_event(&task, &actor, "work", "work.held", json!({
                "key": task.key(),
                "reason": reason,
            })))?;
            finish(conn, &actor, &task.id)
        })
        .await
}

pub async fn unhold(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Edit)?;
            if task.work_state != "on_hold" {
                return Err(ApiError::conflict("work_not_on_hold", "this work is not on hold"));
            }
            conn.execute(
                "UPDATE tasks SET work_state = 'open', hold_reason = NULL, updated_at = ?1 WHERE id = ?2",
                params![now(), task.id],
            )?;
            emit(conn, task_event(&task, &actor, "work", "work.unheld", json!({ "key": task.key() })))?;
            finish(conn, &actor, &task.id)
        })
        .await
}

fn active_holder(task: &TaskRow) -> ApiResult<String> {
    task.occupant
        .as_ref()
        .map(|holder| holder.id.clone())
        .ok_or_else(|| ApiError::conflict("not_occupied", "this work has no holder"))
}

pub async fn revoke(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<ReasonRequest>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Admin)?;
            let reason = required_text("reason", &request.reason, 2_000)?;
            let holder = active_holder(&task)?;
            end_claim(conn, &task, &actor, "revoked")?;
            touch(conn, &task)?;
            emit(conn, task_event(&task, &actor, "work", "work.revoked", json!({
                "key": task.key(),
                "holder_id": holder,
                "reason": reason,
            })))?;
            finish(conn, &actor, &task.id)
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReassignRequest {
    to: String,
    reason: String,
}

pub async fn reassign(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<ReassignRequest>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Admin)?;
            let reason = required_text("reason", &request.reason, 2_000)?;
            let holder = active_holder(&task)?;
            require_transfer_target(conn, &task, &request.to)?;
            end_claim(conn, &task, &actor, "reassigned")?;
            insert_claim(conn, &task, &request.to)?;
            touch(conn, &task)?;
            emit(conn, task_event(&task, &actor, "work", "work.reassigned", json!({
                "key": task.key(),
                "from": holder,
                "to": request.to,
                "reason": reason,
            })))?;
            finish(conn, &actor, &task.id)
        })
        .await
}
