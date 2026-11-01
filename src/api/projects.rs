//! プロジェクトとワークフロー（段階・遷移ルール）。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiJson, AppState, required_text};
use crate::auth::Actor;
use crate::db::{new_id, now};
use crate::error::{ApiError, ApiResult};
use crate::events::{EventInput, emit};
use crate::model::Permission;
use crate::tasks::{refresh_claimable, with_dependents};

const MAX_DESCRIPTION_CHARS: usize = 2000;

fn project_event<'a>(
    org_id: &'a str,
    project_id: &'a str,
    actor_id: &'a str,
    kind: &'static str,
    data: Value,
) -> EventInput<'a> {
    EventInput {
        org_id,
        project_id: Some(project_id),
        task_id: None,
        actor_id: Some(actor_id),
        // admin は委譲できないので、ここへ委譲トークンは届かない。
        delegated_token_id: None,
        category: "admin",
        kind,
        data,
    }
}

/// プロジェクトの概要は空を許す。既存のプロジェクトに入力を強制しないため。
fn project_description(value: &str) -> ApiResult<String> {
    let trimmed = value.trim();
    if trimmed.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(ApiError::invalid(format!(
            "description must be at most {MAX_DESCRIPTION_CHARS} characters"
        )));
    }
    Ok(trimmed.to_string())
}

fn require_visible(conn: &Connection, actor: &Actor, project_id: &str) -> ApiResult<()> {
    let found: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1 AND org_id = ?2)",
        params![project_id, actor.org_id],
        |row| row.get(0),
    )?;
    if found && actor.can_view(project_id) {
        Ok(())
    } else {
        Err(ApiError::not_found("project"))
    }
}

pub fn project_json(conn: &Connection, project_id: &str) -> ApiResult<Value> {
    let mut project = conn.query_row(
        "SELECT id, key, name, description, created_at FROM projects WHERE id = ?1",
        [project_id],
        |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "key": row.get::<_, String>(1)?,
                "name": row.get::<_, String>(2)?,
                "description": row.get::<_, String>(3)?,
                "created_at": row.get::<_, String>(4)?,
            }))
        },
    )?;
    let mut statement = conn.prepare(
        "SELECT id, name, position, requires_work, claim_permission, is_final
         FROM stages WHERE project_id = ?1 ORDER BY position, id",
    )?;
    let stages = statement
        .query_map([project_id], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "position": row.get::<_, i64>(2)?,
                "requires_work": row.get::<_, bool>(3)?,
                "claim_permission": row.get::<_, String>(4)?,
                "is_final": row.get::<_, bool>(5)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut statement = conn.prepare(
        "SELECT id, from_stage_id, to_stage_id, required_permission, require_work_done
         FROM transition_rules WHERE project_id = ?1",
    )?;
    let rules = statement
        .query_map([project_id], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "from_stage_id": row.get::<_, String>(1)?,
                "to_stage_id": row.get::<_, String>(2)?,
                "required_permission": row.get::<_, Option<String>>(3)?,
                "require_work_done": row.get::<_, bool>(4)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    project["stages"] = json!(stages);
    project["transition_rules"] = json!(rules);
    Ok(project)
}

pub async fn list(State(state): State<AppState>, actor: Actor) -> ApiResult<Json<Value>> {
    state
        .db
        .read(move |conn| {
            let mut statement =
                conn.prepare("SELECT id FROM projects WHERE org_id = ?1 ORDER BY name, id")?;
            let ids = statement
                .query_map([&actor.org_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let projects = ids
                .iter()
                .filter(|id| actor.can_view(id))
                .map(|id| project_json(conn, id))
                .collect::<ApiResult<Vec<_>>>()?;
            Ok(Json(json!({ "projects": projects })))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageInput {
    name: String,
    requires_work: bool,
    claim_permission: Permission,
    is_final: bool,
}

fn validate_claim_permission(permission: Permission) -> ApiResult<&'static str> {
    match permission {
        Permission::Claim | Permission::Review | Permission::Approve => Ok(permission.as_str()),
        _ => Err(ApiError::invalid("claim_permission must be claim, review or approve")),
    }
}

fn insert_stage(
    conn: &Connection,
    project_id: &str,
    position: i64,
    stage: &StageInput,
) -> ApiResult<String> {
    let name = required_text("stage name", &stage.name, 100)?;
    let permission = validate_claim_permission(stage.claim_permission)?;
    let duplicate: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM stages WHERE project_id = ?1 AND name = ?2)",
        params![project_id, name],
        |row| row.get(0),
    )?;
    if duplicate {
        return Err(ApiError::conflict("duplicate_stage", format!("stage '{name}' already exists")));
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO stages (id, project_id, name, position, requires_work, claim_permission, is_final)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, project_id, name, position, stage.requires_work, permission, stage.is_final],
    )?;
    Ok(id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProjectRequest {
    key: String,
    name: String,
    #[serde(default)]
    description: String,
    stages: Vec<StageInput>,
}

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    ApiJson(request): ApiJson<CreateProjectRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    actor.require_org_admin()?;
    let name = required_text("name", &request.name, 100)?;
    let key = request.key.trim().to_string();
    let valid_key = (1..=10).contains(&key.len())
        && key.starts_with(|c: char| c.is_ascii_uppercase())
        && key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if !valid_key {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_project_key",
            "key must be 1-10 characters: an uppercase letter, then uppercase letters or digits",
        ));
    }
    if request.stages.is_empty() {
        return Err(ApiError::invalid("a project needs at least one stage"));
    }
    let description = project_description(&request.description)?;
    state
        .db
        .write(move |conn| {
            let taken: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE org_id = ?1 AND key = ?2)",
                params![actor.org_id, key],
                |row| row.get(0),
            )?;
            if taken {
                return Err(ApiError::conflict("duplicate_project_key", format!("key {key} is in use")));
            }
            let id = new_id();
            conn.execute(
                "INSERT INTO projects (id, org_id, key, name, description, created_by, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, actor.org_id, key, name, description, actor.id, now()],
            )?;
            for (position, stage) in request.stages.iter().enumerate() {
                insert_stage(conn, &id, position as i64, stage)?;
            }
            emit(conn, project_event(&actor.org_id, &id, &actor.id, "project.created", json!({
                "key": key,
                "name": name,
            })))?;
            Ok((StatusCode::CREATED, Json(project_json(conn, &id)?)))
        })
        .await
}

pub async fn get(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    state
        .db
        .read(move |conn| {
            require_visible(conn, &actor, &id)?;
            Ok(Json(project_json(conn, &id)?))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateProjectRequest {
    name: Option<String>,
    description: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<UpdateProjectRequest>,
) -> ApiResult<Json<Value>> {
    let description = request.description.as_deref().map(project_description).transpose()?;
    state
        .db
        .write(move |conn| {
            require_visible(conn, &actor, &id)?;
            actor.require(&id, Permission::Admin)?;
            if let Some(name) = &request.name {
                let name = required_text("name", name, 100)?;
                conn.execute("UPDATE projects SET name = ?1 WHERE id = ?2", params![name, id])?;
            }
            if let Some(description) = &description {
                conn.execute(
                    "UPDATE projects SET description = ?1 WHERE id = ?2",
                    params![description, id],
                )?;
            }
            emit(conn, project_event(&actor.org_id, &id, &actor.id, "project.updated", json!({
                "name": request.name,
                "description": description,
            })))?;
            tracing::info!(project = %id, actor = %actor.id, description_changed = description.is_some(), "updated project");
            Ok(Json(project_json(conn, &id)?))
        })
        .await
}

pub async fn add_stage(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<StageInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    state
        .db
        .write(move |conn| {
            require_visible(conn, &actor, &id)?;
            actor.require(&id, Permission::Admin)?;
            let position: i64 = conn.query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM stages WHERE project_id = ?1",
                [&id],
                |row| row.get(0),
            )?;
            let stage_id = insert_stage(conn, &id, position, &request)?;
            emit(conn, project_event(&actor.org_id, &id, &actor.id, "stage.created", json!({
                "stage_id": stage_id,
                "name": request.name,
            })))?;
            Ok((StatusCode::CREATED, Json(project_json(conn, &id)?)))
        })
        .await
}

fn stage_project(conn: &Connection, actor: &Actor, stage_id: &str) -> ApiResult<String> {
    let project_id: Option<String> = conn
        .query_row("SELECT project_id FROM stages WHERE id = ?1", [stage_id], |row| row.get(0))
        .optional()?;
    let project_id = project_id.ok_or_else(|| ApiError::not_found("stage"))?;
    require_visible(conn, actor, &project_id).map_err(|_| ApiError::not_found("stage"))?;
    Ok(project_id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateStageRequest {
    name: Option<String>,
    position: Option<i64>,
    requires_work: Option<bool>,
    claim_permission: Option<Permission>,
    is_final: Option<bool>,
}

pub async fn update_stage(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<UpdateStageRequest>,
) -> ApiResult<Json<Value>> {
    state
        .db
        .write(move |conn| {
            let project_id = stage_project(conn, &actor, &id)?;
            actor.require(&project_id, Permission::Admin)?;
            if let Some(name) = &request.name {
                let name = required_text("stage name", name, 100)?;
                let duplicate: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM stages WHERE project_id = ?1 AND name = ?2 AND id <> ?3)",
                    params![project_id, name, id],
                    |row| row.get(0),
                )?;
                if duplicate {
                    return Err(ApiError::conflict("duplicate_stage", format!("stage '{name}' already exists")));
                }
                conn.execute("UPDATE stages SET name = ?1 WHERE id = ?2", params![name, id])?;
            }
            if let Some(position) = request.position {
                conn.execute("UPDATE stages SET position = ?1 WHERE id = ?2", params![position, id])?;
            }
            if let Some(requires_work) = request.requires_work {
                conn.execute(
                    "UPDATE stages SET requires_work = ?1 WHERE id = ?2",
                    params![requires_work, id],
                )?;
            }
            if let Some(permission) = request.claim_permission {
                let permission = validate_claim_permission(permission)?;
                conn.execute(
                    "UPDATE stages SET claim_permission = ?1 WHERE id = ?2",
                    params![permission, id],
                )?;
            }
            if let Some(is_final) = request.is_final {
                // 破棄されたタスクは最終段階にある前提で、依存元のブロックもこの前提に立つ。
                let discarded: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM tasks WHERE stage_id = ?1 AND discarded_at IS NOT NULL",
                    [&id],
                    |row| row.get(0),
                )?;
                if !is_final && discarded > 0 {
                    return Err(ApiError::conflict(
                        "stage_has_discarded_tasks",
                        format!("{discarded} discarded tasks are in this stage; restore them first"),
                    ));
                }
                conn.execute("UPDATE stages SET is_final = ?1 WHERE id = ?2", params![is_final, id])?;
            }
            emit(conn, project_event(&actor.org_id, &project_id, &actor.id, "stage.updated", json!({
                "stage_id": id,
                "name": request.name,
                "position": request.position,
                "requires_work": request.requires_work,
                "claim_permission": request.claim_permission,
                "is_final": request.is_final,
            })))?;

            // requires_work と is_final の変更は、段階内のタスクと依存元の取得可能性を変える。
            let mut statement = conn.prepare("SELECT id FROM tasks WHERE stage_id = ?1")?;
            let in_stage = statement
                .query_map([&id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut affected = Vec::new();
            for task_id in &in_stage {
                affected.extend(with_dependents(conn, task_id)?);
            }
            affected.sort();
            affected.dedup();
            refresh_claimable(conn, &actor, &affected)?;
            Ok(Json(project_json(conn, &project_id)?))
        })
        .await
}

pub async fn delete_stage(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state
        .db
        .write(move |conn| {
            let project_id = stage_project(conn, &actor, &id)?;
            actor.require(&project_id, Permission::Admin)?;
            let used: i64 =
                conn.query_row("SELECT COUNT(*) FROM tasks WHERE stage_id = ?1", [&id], |row| row.get(0))?;
            if used > 0 {
                return Err(ApiError::conflict(
                    "stage_in_use",
                    format!("{used} tasks are in this stage; move them first"),
                ));
            }
            conn.execute(
                "DELETE FROM transition_rules WHERE from_stage_id = ?1 OR to_stage_id = ?1",
                [&id],
            )?;
            conn.execute("DELETE FROM stages WHERE id = ?1", [&id])?;
            emit(conn, project_event(&actor.org_id, &project_id, &actor.id, "stage.deleted", json!({
                "stage_id": id,
            })))?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleInput {
    from_stage_id: String,
    to_stage_id: String,
    required_permission: Option<Permission>,
    require_work_done: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PutRulesRequest {
    rules: Vec<RuleInput>,
}

/// 遷移ルールを丸ごと置き換える。
pub async fn put_rules(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<PutRulesRequest>,
) -> ApiResult<Json<Value>> {
    state
        .db
        .write(move |conn| {
            require_visible(conn, &actor, &id)?;
            actor.require(&id, Permission::Admin)?;
            let before = project_json(conn, &id)?["transition_rules"].clone();
            conn.execute("DELETE FROM transition_rules WHERE project_id = ?1", [&id])?;
            for rule in &request.rules {
                for stage_id in [&rule.from_stage_id, &rule.to_stage_id] {
                    let found: bool = conn.query_row(
                        "SELECT EXISTS(SELECT 1 FROM stages WHERE id = ?1 AND project_id = ?2)",
                        params![stage_id, id],
                        |row| row.get(0),
                    )?;
                    if !found {
                        return Err(ApiError::invalid(format!("stage {stage_id} is not in this project")));
                    }
                }
                let permission = match rule.required_permission {
                    None => None,
                    Some(
                        p @ (Permission::Edit
                        | Permission::Review
                        | Permission::Approve
                        | Permission::Admin),
                    ) => Some(p.as_str()),
                    Some(_) => {
                        return Err(ApiError::invalid(
                            "required_permission must be edit, review, approve or admin",
                        ));
                    }
                };
                let inserted = conn.execute(
                    "INSERT OR IGNORE INTO transition_rules
                     (id, project_id, from_stage_id, to_stage_id, required_permission, require_work_done)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        new_id(),
                        id,
                        rule.from_stage_id,
                        rule.to_stage_id,
                        permission,
                        rule.require_work_done
                    ],
                )?;
                if inserted == 0 {
                    return Err(ApiError::invalid("each transition may have only one rule"));
                }
            }
            let project = project_json(conn, &id)?;
            emit(conn, project_event(&actor.org_id, &id, &actor.id, "workflow.rules_replaced", json!({
                "rule_count": request.rules.len(),
                "from": before,
                "to": project["transition_rules"],
            })))?;
            Ok(Json(project))
        })
        .await
}
