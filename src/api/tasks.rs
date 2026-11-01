//! タスクの作成・検索・編集と、業務段階の変更。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::feed::task_history;
use super::{ApiJson, ApiQuery, AppState, explicit, required_text};
use crate::auth::Actor;
use crate::db::{new_id, now, parse_time};
use crate::error::{ApiError, ApiResult};
use crate::events::{EventInput, emit};
use crate::model::{Permission, Readiness};
use crate::tasks::{
    TaskRow, TaskView, load_task, load_tasks, parse_priority, priority_name, refresh_claimable,
    require_assignable, require_same_project, resolve_task, to_view, with_dependents,
};

const DEFAULT_PAGE_SIZE: usize = 100;
const MAX_PAGE_SIZE: usize = 500;
const MAX_DISCARD_REASON_CHARS: usize = 500;

pub fn task_event<'a>(
    task: &'a TaskRow,
    actor: &'a Actor,
    category: &'static str,
    kind: &'static str,
    data: Value,
) -> EventInput<'a> {
    EventInput {
        org_id: &task.org_id,
        project_id: Some(&task.project_id),
        task_id: Some(&task.id),
        actor_id: Some(&actor.id),
        delegated_token_id: actor.delegated_token_id(),
        category,
        kind,
        data,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    project: Option<String>,
    stage: Option<String>,
    /// タスクの状態として取得可能か
    claimable: Option<bool>,
    /// 取得可能で、かつ自分に取得権限がある
    can_claim: Option<bool>,
    priority: Option<String>,
    assignee: Option<String>,
    responsible: Option<String>,
    occupant: Option<String>,
    activity: Option<String>,
    readiness: Option<String>,
    parent: Option<String>,
    /// このタスクに直接依存する仕事。ID またはキー。
    depends_on: Option<String>,
    due_before: Option<String>,
    q: Option<String>,
    sort: Option<String>,
    order: Option<String>,
    limit: Option<usize>,
    cursor: Option<usize>,
}

fn me_or_id(actor: &Actor, value: String) -> String {
    if value == "me" { actor.id.clone() } else { value }
}

fn matches_readiness(readiness: Readiness, wanted: &str) -> ApiResult<bool> {
    let current = match readiness {
        Readiness::Ready => "ready",
        Readiness::Waiting => "waiting",
        Readiness::Done => "done",
        Readiness::NotRequired => "not_required",
        Readiness::Unknown => "unknown",
    };
    if !matches!(wanted, "ready" | "waiting" | "done" | "not_required" | "unknown") {
        return Err(ApiError::invalid("readiness must be ready, waiting, done, not_required or unknown"));
    }
    Ok(current == wanted)
}

pub async fn list(
    State(state): State<AppState>,
    actor: Actor,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(DEFAULT_PAGE_SIZE);
    if limit == 0 || limit > MAX_PAGE_SIZE {
        return Err(ApiError::invalid(format!("limit must be between 1 and {MAX_PAGE_SIZE}")));
    }
    let offset = query.cursor.unwrap_or(0);
    let direction = match query.order.as_deref() {
        Some("asc") => "ASC",
        Some("desc") | None => "DESC",
        Some(_) => return Err(ApiError::invalid("order must be asc or desc")),
    };
    let order = match query.sort.as_deref() {
        Some("priority") | None => format!("t.priority {direction}, t.id ASC"),
        Some("created") => format!("t.created_at {direction}, t.id ASC"),
        Some("updated") => format!("t.updated_at {direction}, t.id ASC"),
        // 期限なしは並びの向きに関係なく末尾に置く。
        Some("due") => format!("t.due_at IS NULL, t.due_at {direction}, t.id ASC"),
        Some(_) => return Err(ApiError::invalid("sort must be priority, created, updated or due")),
    };

    let mut filter = vec!["t.org_id = ?1".to_string()];
    let mut args = vec![SqlValue::Text(actor.org_id.clone())];
    let mut push = |clause: &str, value: SqlValue| {
        args.push(value);
        filter.push(clause.replace('?', &format!("?{}", args.len())));
    };
    if let Some(value) = query.project {
        push("t.project_id = ?", SqlValue::Text(value));
    }
    if let Some(value) = query.stage {
        push("t.stage_id = ?", SqlValue::Text(value));
    }
    if let Some(value) = &query.priority {
        push("t.priority = ?", SqlValue::Integer(parse_priority(value)?));
    }
    if let Some(value) = query.assignee {
        push("t.assignee_id = ?", SqlValue::Text(me_or_id(&actor, value)));
    }
    if let Some(value) = query.responsible {
        push("t.responsible_id = ?", SqlValue::Text(me_or_id(&actor, value)));
    }
    if let Some(value) = query.parent {
        push("t.parent_id = ?", SqlValue::Text(value));
    }
    if let Some(value) = &query.due_before {
        push("t.due_at < ?", SqlValue::Text(parse_time("due_before", value)?));
    }
    if let Some(value) = &query.q {
        let escaped = value.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        push(
            "(t.title LIKE ? ESCAPE '\\' OR t.description LIKE ? ESCAPE '\\')",
            SqlValue::Text(format!("%{escaped}%")),
        );
    }
    let occupant = query.occupant.map(|value| me_or_id(&actor, value));
    let mut filter = filter.join(" AND ");

    state
        .db
        .read(move |conn| {
            if let Some(reference) = &query.depends_on {
                let dependency = resolve_task(conn, &actor, reference)?;
                args.push(SqlValue::Text(dependency.id));
                filter.push_str(&format!(
                    " AND EXISTS (SELECT 1 FROM task_dependencies d JOIN tasks b ON b.id = d.depends_on_id
                      WHERE d.task_id = t.id AND d.depends_on_id = ?{} AND b.project_id = t.project_id)",
                    args.len()
                ));
            }
            let mut views = Vec::new();
            for task in load_tasks(conn, &filter, &args, &order)? {
                if !actor.can_view_task(&task.project_id, &task.id) {
                    continue;
                }
                let view = to_view(&task, &actor)?;
                let keep = query.claimable.is_none_or(|wanted| view.work.claimable == wanted)
                    && query.can_claim.is_none_or(|wanted| view.work.can_claim == wanted)
                    && occupant.as_ref().is_none_or(|id| {
                        view.work.occupant.as_ref().is_some_and(|holder| &holder.id == id)
                    })
                    && query.activity.as_ref().is_none_or(|activity| {
                        view.work.activity.as_ref() == Some(activity)
                    });
                let keep = keep
                    && match &query.readiness {
                        Some(wanted) => matches_readiness(view.work.readiness, wanted)?,
                        None => true,
                    };
                if keep {
                    views.push(view);
                }
            }
            let total = views.len();
            let page: Vec<TaskView> = views.into_iter().skip(offset).take(limit).collect();
            let end = offset + page.len();
            Ok(Json(json!({
                "tasks": page,
                "total": total,
                "next_cursor": if end < total { Some(end) } else { None },
            })))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTaskRequest {
    project_id: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    purpose: String,
    #[serde(default)]
    acceptance_criteria: String,
    /// 省略すると先頭の段階に作る
    stage_id: Option<String>,
    responsible_id: Option<String>,
    assignee_id: Option<String>,
    priority: Option<String>,
    due_at: Option<String>,
    parent: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    ApiJson(request): ApiJson<CreateTaskRequest>,
) -> ApiResult<(StatusCode, Json<TaskView>)> {
    let title = required_text("title", &request.title, 500)?;
    let priority = parse_priority(request.priority.as_deref().unwrap_or("none"))?;
    let due_at = request.due_at.as_deref().map(|v| parse_time("due_at", v)).transpose()?;
    state
        .db
        .write(move |conn| {
            let in_org: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1 AND org_id = ?2)",
                params![request.project_id, actor.org_id],
                |row| row.get(0),
            )?;
            if !in_org || !actor.can_view(&request.project_id) {
                return Err(ApiError::not_found("project"));
            }
            actor.require(&request.project_id, Permission::Create)?;
            // タスクを限定した委譲トークンは、範囲内のタスクの子だけを作れる。
            if actor.task_scope.is_some() && request.parent.is_none() {
                actor.log_delegated_denial("task inside the delegation");
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "outside_delegated_tasks",
                    "this token can only create children of its delegated tasks",
                ));
            }
            if request.responsible_id.is_some() || request.assignee_id.is_some() {
                actor.require(&request.project_id, Permission::Assign)?;
            }
            for id in [&request.responsible_id, &request.assignee_id].into_iter().flatten() {
                require_assignable(conn, &actor.org_id, id)?;
            }
            let stage_id: Option<String> = match &request.stage_id {
                Some(id) => conn
                    .query_row(
                        "SELECT id FROM stages WHERE id = ?1 AND project_id = ?2",
                        params![id, request.project_id],
                        |row| row.get(0),
                    )
                    .optional()?,
                None => conn
                    .query_row(
                        "SELECT id FROM stages WHERE project_id = ?1 ORDER BY position, id LIMIT 1",
                        [&request.project_id],
                        |row| row.get(0),
                    )
                    .optional()?,
            };
            let stage_id = stage_id.ok_or_else(|| ApiError::invalid("stage is not in this project"))?;
            let parent_id = match &request.parent {
                Some(reference) => {
                    let parent = resolve_task(conn, &actor, reference)?;
                    require_same_project(&request.project_id, &parent)?;
                    Some(parent.id)
                }
                None => None,
            };
            let number: i64 = conn.query_row(
                "UPDATE projects SET next_task_number = next_task_number + 1 WHERE id = ?1
                 RETURNING next_task_number - 1",
                [&request.project_id],
                |row| row.get(0),
            )?;
            let id = new_id();
            let moment = now();
            conn.execute(
                "INSERT INTO tasks (id, org_id, project_id, number, title, description, purpose,
                    acceptance_criteria, stage_id, responsible_id, assignee_id, priority, due_at,
                    parent_id, work_state, created_by, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'open', ?15, ?16, ?16)",
                params![
                    id,
                    actor.org_id,
                    request.project_id,
                    number,
                    title,
                    request.description,
                    request.purpose,
                    request.acceptance_criteria,
                    stage_id,
                    request.responsible_id,
                    request.assignee_id,
                    priority,
                    due_at,
                    parent_id,
                    actor.id,
                    moment
                ],
            )?;
            let task = load_task(conn, &id)?;
            emit(conn, task_event(&task, &actor, "task", "task.created", json!({
                "key": task.key(),
                "title": task.title,
                "stage": task.stage.name,
            })))?;
            refresh_claimable(conn, &actor, std::slice::from_ref(&id))?;
            Ok((StatusCode::CREATED, Json(to_view(&load_task(conn, &id)?, &actor)?)))
        })
        .await
}

pub async fn get(
    State(state): State<AppState>,
    actor: Actor,
    Path(task): Path<String>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .read(move |conn| {
            let row = resolve_task(conn, &actor, &task)?;
            Ok(Json(to_view(&row, &actor)?))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateTaskRequest {
    /// 読み取った時点の version。他者の変更を上書きしないために必須。
    version: i64,
    title: Option<String>,
    description: Option<String>,
    purpose: Option<String>,
    acceptance_criteria: Option<String>,
    priority: Option<String>,
    #[serde(default, deserialize_with = "explicit")]
    due_at: Option<Option<String>>,
    #[serde(default, deserialize_with = "explicit")]
    parent: Option<Option<String>>,
    #[serde(default, deserialize_with = "explicit")]
    responsible_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "explicit")]
    assignee_id: Option<Option<String>>,
}

pub fn check_version(task: &TaskRow, version: i64) -> ApiResult<()> {
    if task.version == version {
        return Ok(());
    }
    Err(ApiError::conflict("version_conflict", "the task was changed by someone else")
        .details(json!({ "current_version": task.version })))
}

fn ensure_no_parent_cycle(conn: &Connection, task_id: &str, parent_id: &str) -> ApiResult<()> {
    let mut current = Some(parent_id.to_string());
    while let Some(id) = current {
        if id == task_id {
            return Err(ApiError::invalid("a task cannot be its own ancestor"));
        }
        current = conn.query_row("SELECT parent_id FROM tasks WHERE id = ?1", [&id], |row| row.get(0))?;
    }
    Ok(())
}

pub async fn update(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<UpdateTaskRequest>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            let edits_content = request.title.is_some()
                || request.description.is_some()
                || request.purpose.is_some()
                || request.acceptance_criteria.is_some()
                || request.priority.is_some()
                || request.due_at.is_some()
                || request.parent.is_some();
            if edits_content {
                actor.require(&task.project_id, Permission::Edit)?;
            }
            if request.responsible_id.is_some() || request.assignee_id.is_some() {
                actor.require(&task.project_id, Permission::Assign)?;
            }
            check_version(&task, request.version)?;

            let mut changes = Map::new();
            let mut set = |column: &str, from: Value, to: Value, sql_value: SqlValue| -> ApiResult<()> {
                if from == to {
                    return Ok(());
                }
                conn.execute(
                    &format!("UPDATE tasks SET {column} = ?1 WHERE id = ?2"),
                    params![sql_value, task.id],
                )?;
                changes.insert(column.to_string(), json!({ "from": from, "to": to }));
                Ok(())
            };
            let text = |value: &str| SqlValue::Text(value.to_string());
            let optional = |value: &Option<String>| match value {
                Some(value) => SqlValue::Text(value.clone()),
                None => SqlValue::Null,
            };

            if let Some(title) = &request.title {
                let title = required_text("title", title, 500)?;
                set("title", json!(task.title), json!(title), text(&title))?;
            }
            if let Some(value) = &request.description {
                set("description", json!(task.description), json!(value), text(value))?;
            }
            if let Some(value) = &request.purpose {
                set("purpose", json!(task.purpose), json!(value), text(value))?;
            }
            if let Some(value) = &request.acceptance_criteria {
                set("acceptance_criteria", json!(task.acceptance_criteria), json!(value), text(value))?;
            }
            if let Some(value) = &request.priority {
                let priority = parse_priority(value)?;
                set(
                    "priority",
                    json!(priority_name(task.priority)),
                    json!(priority_name(priority)),
                    SqlValue::Integer(priority),
                )?;
            }
            if let Some(value) = &request.due_at {
                let due_at = value.as_deref().map(|v| parse_time("due_at", v)).transpose()?;
                set("due_at", json!(task.due_at), json!(due_at), optional(&due_at))?;
            }
            if let Some(value) = &request.parent {
                let parent_id = match value {
                    Some(reference) => {
                        let parent = resolve_task(conn, &actor, reference)?;
                        require_same_project(&task.project_id, &parent)?;
                        ensure_no_parent_cycle(conn, &task.id, &parent.id)?;
                        Some(parent.id)
                    }
                    None => None,
                };
                set("parent_id", json!(task.parent_id), json!(parent_id), optional(&parent_id))?;
            }
            if let Some(value) = &request.responsible_id {
                if let Some(id) = value {
                    require_assignable(conn, &actor.org_id, id)?;
                }
                let from = task.responsible.as_ref().map(|p| p.id.clone());
                set("responsible_id", json!(from), json!(value), optional(value))?;
            }
            if let Some(value) = &request.assignee_id {
                if let Some(id) = value {
                    require_assignable(conn, &actor.org_id, id)?;
                }
                let from = task.assignee.as_ref().map(|p| p.id.clone());
                set("assignee_id", json!(from), json!(value), optional(value))?;
            }

            if !changes.is_empty() {
                conn.execute(
                    "UPDATE tasks SET version = version + 1, updated_at = ?1 WHERE id = ?2",
                    params![now(), task.id],
                )?;
                emit(conn, task_event(&task, &actor, "task", "task.updated", json!({
                    "key": task.key(),
                    "changes": changes,
                })))?;
            }
            Ok(Json(to_view(&load_task(conn, &task.id)?, &actor)?))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveStageRequest {
    to_stage_id: String,
    version: i64,
}

/// 業務段階の変更。作業の完了や解放とは独立した明示的な操作で、ここでだけ段階が変わる。
/// 新しい段階では作業状態を初期化する。占有が残ったままの移動は拒否する。
fn reject_occupied(task: &TaskRow) -> ApiResult<()> {
    match &task.occupant {
        Some(holder) => Err(ApiError::conflict(
            "task_occupied",
            format!("{} holds this work; it must be completed or released first", holder.display_name),
        )
        .details(json!({ "occupant": holder }))),
        None => Ok(()),
    }
}

/// 破棄の属性は最終段階にあるタスクだけに付く。通常の移動で崩さないよう、取り消しを経由させる。
fn reject_discarded(task: &TaskRow) -> ApiResult<()> {
    if task.discard.is_some() {
        return Err(ApiError::conflict(
            "task_discarded",
            "this task is discarded; restore it before changing its stage",
        ));
    }
    Ok(())
}

/// 同じプロジェクトの段階の名前と、最終段階かどうかを返す。
fn stage_in_project(conn: &Connection, task: &TaskRow, stage_id: &str) -> ApiResult<(String, bool)> {
    conn.query_row(
        "SELECT name, is_final FROM stages WHERE id = ?1 AND project_id = ?2",
        params![stage_id, task.project_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()?
    .ok_or_else(|| ApiError::invalid("stage is not in this project"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscardRequest {
    /// 読み取った時点の version。他者の変更を見落として閉じないために必須。
    version: i64,
    to_stage_id: String,
    reason: String,
}

/// 実施しないと決めたタスクを最終段階へ閉じる。完了の承認でも依存の解除でもないので、
/// 遷移ルールは適用しない。
pub async fn discard(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<DiscardRequest>,
) -> ApiResult<Json<TaskView>> {
    let reason = required_text("reason", &request.reason, MAX_DISCARD_REASON_CHARS)?;
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Edit)?;
            check_version(&task, request.version)?;
            reject_discarded(&task)?;
            if task.stage.is_final {
                return Err(ApiError::conflict(
                    "task_finished",
                    "this task already reached a final stage; move it back before discarding it",
                ));
            }
            let (target, is_final) = stage_in_project(conn, &task, &request.to_stage_id)?;
            if !is_final {
                return Err(ApiError::invalid("a task can only be discarded into a final stage"));
            }
            reject_occupied(&task)?;

            let at = now();
            conn.execute(
                "UPDATE tasks SET stage_id = ?1, work_state = 'open', hold_reason = NULL,
                    discarded_at = ?2, discard_reason = ?3, discarded_by = ?4,
                    version = version + 1, updated_at = ?2 WHERE id = ?5",
                params![request.to_stage_id, at, reason, actor.id, task.id],
            )?;
            emit(conn, task_event(&task, &actor, "stage", "task.discarded", json!({
                "key": task.key(),
                "reason": reason,
                "from": { "id": task.stage.id, "name": task.stage.name },
                "to": { "id": request.to_stage_id, "name": target },
                "previous_work_state": task.work_state,
            })))?;
            tracing::info!(task = %task.id, actor = %actor.id, to_stage = %request.to_stage_id, "discarded task");
            refresh_claimable(conn, &actor, &with_dependents(conn, &task.id)?)?;
            Ok(Json(to_view(&load_task(conn, &task.id)?, &actor)?))
        })
        .await
}

/// 破棄を取り消し、これから仕事を行う段階へ戻す。
pub async fn restore(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<DiscardRequest>,
) -> ApiResult<Json<TaskView>> {
    let reason = required_text("reason", &request.reason, MAX_DISCARD_REASON_CHARS)?;
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Edit)?;
            check_version(&task, request.version)?;
            let Some(discard) = &task.discard else {
                return Err(ApiError::conflict("task_not_discarded", "this task is not discarded"));
            };
            let (target, is_final) = stage_in_project(conn, &task, &request.to_stage_id)?;
            if is_final {
                return Err(ApiError::invalid("a task can only be restored into a non-final stage"));
            }

            conn.execute(
                "UPDATE tasks SET stage_id = ?1, work_state = 'open', hold_reason = NULL,
                    discarded_at = NULL, discard_reason = NULL, discarded_by = NULL,
                    version = version + 1, updated_at = ?2 WHERE id = ?3",
                params![request.to_stage_id, now(), task.id],
            )?;
            emit(conn, task_event(&task, &actor, "stage", "task.restored", json!({
                "key": task.key(),
                "reason": reason,
                "discard_reason": discard.reason,
                "from": { "id": task.stage.id, "name": task.stage.name },
                "to": { "id": request.to_stage_id, "name": target },
            })))?;
            tracing::info!(task = %task.id, actor = %actor.id, to_stage = %request.to_stage_id, "restored discarded task");
            refresh_claimable(conn, &actor, &with_dependents(conn, &task.id)?)?;
            Ok(Json(to_view(&load_task(conn, &task.id)?, &actor)?))
        })
        .await
}

pub async fn move_stage(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<MoveStageRequest>,
) -> ApiResult<Json<TaskView>> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            let target: Option<String> = conn
                .query_row(
                    "SELECT name FROM stages WHERE id = ?1 AND project_id = ?2",
                    params![request.to_stage_id, task.project_id],
                    |row| row.get(0),
                )
                .optional()?;
            let target = target.ok_or_else(|| ApiError::invalid("stage is not in this project"))?;
            if request.to_stage_id == task.stage.id {
                return Err(ApiError::invalid("the task is already in this stage"));
            }

            let rule: Option<(Option<String>, bool)> = conn
                .query_row(
                    "SELECT required_permission, require_work_done FROM transition_rules
                     WHERE from_stage_id = ?1 AND to_stage_id = ?2",
                    params![task.stage.id, request.to_stage_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let (required, require_done) = rule.unwrap_or((None, false));
            let permission = match required {
                Some(name) => Permission::parse(&name)
                    .ok_or_else(|| ApiError::internal(format!("unknown rule permission {name}")))?,
                None => Permission::Edit,
            };
            actor.require(&task.project_id, permission)?;
            check_version(&task, request.version)?;
            reject_discarded(&task)?;
            reject_occupied(&task)?;
            if require_done && task.work_state != "done" {
                return Err(ApiError::conflict(
                    "transition_condition_unmet",
                    "this transition requires the work of the current stage to be completed",
                )
                .details(json!({ "work_state": task.work_state })));
            }

            conn.execute(
                "UPDATE tasks SET stage_id = ?1, work_state = 'open', hold_reason = NULL,
                    version = version + 1, updated_at = ?2 WHERE id = ?3",
                params![request.to_stage_id, now(), task.id],
            )?;
            emit(conn, task_event(&task, &actor, "stage", "stage.changed", json!({
                "key": task.key(),
                "from": { "id": task.stage.id, "name": task.stage.name },
                "to": { "id": request.to_stage_id, "name": target },
                "previous_work_state": task.work_state,
            })))?;
            refresh_claimable(conn, &actor, &with_dependents(conn, &task.id)?)?;
            Ok(Json(to_view(&load_task(conn, &task.id)?, &actor)?))
        })
        .await
}

fn claims_json(conn: &Connection, task_id: &str) -> ApiResult<Vec<Value>> {
    let mut statement = conn.prepare(
        "SELECT c.holder_id, p.display_name, p.kind, c.activity, c.acquired_at, c.ended_at,
                c.end_reason, c.ended_by
         FROM claims c JOIN principals p ON p.id = c.holder_id
         WHERE c.task_id = ?1 ORDER BY c.acquired_at, c.id",
    )?;
    let claims = statement
        .query_map([task_id], |row| {
            Ok(json!({
                "holder": {
                    "id": row.get::<_, String>(0)?,
                    "display_name": row.get::<_, String>(1)?,
                    "kind": row.get::<_, String>(2)?,
                },
                "activity": row.get::<_, String>(3)?,
                "acquired_at": row.get::<_, String>(4)?,
                "ended_at": row.get::<_, Option<String>>(5)?,
                "end_reason": row.get::<_, Option<String>>(6)?,
                "ended_by": row.get::<_, Option<String>>(7)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(claims)
}

fn comments_json(conn: &Connection, task_id: &str) -> ApiResult<Vec<Value>> {
    let mut statement = conn.prepare(
        "SELECT c.id, c.kind, c.body, c.addressee_id, c.parent_id, c.resolved_at, c.created_at,
                a.id, a.display_name, a.kind
         FROM comments c JOIN principals a ON a.id = c.author_id
         WHERE c.task_id = ?1 ORDER BY c.created_at, c.id",
    )?;
    let comments = statement
        .query_map([task_id], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "kind": row.get::<_, String>(1)?,
                "body": row.get::<_, String>(2)?,
                "addressee_id": row.get::<_, Option<String>>(3)?,
                "parent_id": row.get::<_, Option<String>>(4)?,
                "resolved_at": row.get::<_, Option<String>>(5)?,
                "created_at": row.get::<_, String>(6)?,
                "author": {
                    "id": row.get::<_, String>(7)?,
                    "display_name": row.get::<_, String>(8)?,
                    "kind": row.get::<_, String>(9)?,
                },
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(comments)
}

fn artifacts_json(conn: &Connection, task_id: &str) -> ApiResult<Vec<Value>> {
    let mut statement = conn.prepare(
        "SELECT id, role, name, url, media_type, size, created_by, created_at
         FROM artifacts WHERE task_id = ?1 ORDER BY created_at, id",
    )?;
    let artifacts = statement
        .query_map([task_id], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "role": row.get::<_, String>(1)?,
                "name": row.get::<_, String>(2)?,
                "url": row.get::<_, Option<String>>(3)?,
                "media_type": row.get::<_, Option<String>>(4)?,
                "size": row.get::<_, Option<i64>>(5)?,
                "created_by": row.get::<_, String>(6)?,
                "created_at": row.get::<_, String>(7)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(artifacts)
}

fn related_views(
    conn: &Connection,
    actor: &Actor,
    filter: &str,
    task_id: &str,
) -> ApiResult<Vec<TaskView>> {
    load_tasks(conn, filter, &[SqlValue::Text(task_id.to_string())], "t.number")?
        .iter()
        .filter(|task| actor.can_view_task(&task.project_id, &task.id))
        .map(|task| to_view(task, actor))
        .collect()
}

/// 引継ぎ先が会話履歴なしで作業を始められるだけの文脈をまとめて返す。
pub async fn context(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
) -> ApiResult<Json<Value>> {
    state
        .db
        .read(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            let parent = match &task.parent_id {
                Some(id) => Some(load_task(conn, id)?)
                    .filter(|parent| actor.can_view_task(&parent.project_id, &parent.id))
                    .map(|parent| to_view(&parent, &actor))
                    .transpose()?,
                None => None,
            };
            let dependencies = related_views(
                conn,
                &actor,
                "t.id IN (SELECT depends_on_id FROM task_dependencies WHERE task_id = ?1)",
                &task.id,
            )?;
            let dependents = related_views(
                conn,
                &actor,
                "t.id IN (SELECT task_id FROM task_dependencies WHERE depends_on_id = ?1)",
                &task.id,
            )?;
            let children = related_views(conn, &actor, "t.parent_id = ?1", &task.id)?;
            let links = related_views(
                conn,
                &actor,
                "t.id IN (SELECT high_id FROM task_links WHERE low_id = ?1
                          UNION SELECT low_id FROM task_links WHERE high_id = ?1)",
                &task.id,
            )?;
            let mut statement = conn.prepare(
                "SELECT depends_on_id, reason FROM task_dependencies WHERE task_id = ?1",
            )?;
            let reasons: Map<String, Value> = statement
                .query_map([&task.id], |row| {
                    Ok((row.get::<_, String>(0)?, Value::String(row.get(1)?)))
                })?
                .collect::<Result<_, _>>()?;
            let dependencies: Vec<Value> = dependencies
                .into_iter()
                .map(|view| json!({ "reason": reasons.get(&view.id), "task": view }))
                .collect();
            // 作業者がプロジェクトの目的を別途探さずに済むよう、タスクの文脈に含める。
            let project = conn.query_row(
                "SELECT id, key, name, description FROM projects WHERE id = ?1",
                [&task.project_id],
                |row| {
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "key": row.get::<_, String>(1)?,
                        "name": row.get::<_, String>(2)?,
                        "description": row.get::<_, String>(3)?,
                    }))
                },
            )?;
            Ok(Json(json!({
                "project": project,
                "task": to_view(&task, &actor)?,
                "parent": parent,
                "children": children,
                "dependencies": dependencies,
                "dependents": dependents,
                "links": links,
                "comments": comments_json(conn, &task.id)?,
                "artifacts": artifacts_json(conn, &task.id)?,
                "claims": claims_json(conn, &task.id)?,
                "history": task_history(conn, &actor, &task.id)?,
            })))
        })
        .await
}
