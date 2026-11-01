//! 委譲トークンの発行と一覧。主体を増やさず、自分のトークンから権限を絞ったトークンを作る。

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::admin::{admin_event, project_in_org};
use super::{ApiJson, AppState, required_text};
use crate::auth::{Actor, grants_cover, hash_secret, new_secret};
use crate::db::{new_id, now, parse_time};
use crate::error::{ApiError, ApiResult};
use crate::events::emit;
use crate::model::Permission;
use crate::tasks::resolve_task;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeInput {
    project_id: String,
    permission: Permission,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegateRequest {
    name: String,
    expires_at: String,
    scopes: Vec<ScopeInput>,
    /// 限定するタスクの ID またはキー。子孫も範囲に含む。
    tasks: Option<Vec<String>>,
}

/// セッションは親として連鎖失効の起点にならないので、委譲元にしない。
fn current_token(actor: &Actor) -> ApiResult<String> {
    actor.token_id.clone().ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "token_required",
            "delegation starts from an API token, not a browser session",
        )
    })
}

/// 委譲の範囲を API の形で返す。
pub fn scope_json(conn: &Connection, token_id: &str) -> ApiResult<(Value, Value)> {
    let mut statement = conn.prepare_cached(
        "SELECT project_id, permission FROM token_scopes WHERE token_id = ?1
         ORDER BY project_id, permission",
    )?;
    let scopes = statement
        .query_map([token_id], |row| {
            Ok(json!({
                "project_id": row.get::<_, String>(0)?,
                "permission": row.get::<_, String>(1)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut statement =
        conn.prepare_cached("SELECT task_id FROM token_tasks WHERE token_id = ?1 ORDER BY task_id")?;
    let tasks = statement
        .query_map([token_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((json!(scopes), json!(tasks)))
}

/// `token_id` が `ancestor_id` 自身か、その子孫であるか。
pub fn token_is_within(conn: &Connection, ancestor_id: &str, token_id: &str) -> ApiResult<bool> {
    Ok(conn.query_row(
        "WITH RECURSIVE lineage(id) AS (
           SELECT ?1
           UNION SELECT t.parent_token_id FROM api_tokens t JOIN lineage l ON t.id = l.id
                 WHERE t.parent_token_id IS NOT NULL
         )
         SELECT EXISTS(SELECT 1 FROM lineage WHERE id = ?2)",
        params![token_id, ancestor_id],
        |row| row.get(0),
    )?)
}

fn validate_scopes(conn: &Connection, actor: &Actor, scopes: &[ScopeInput]) -> ApiResult<()> {
    if scopes.is_empty() {
        return Err(ApiError::invalid("scopes must not be empty"));
    }
    for scope in scopes {
        if scope.permission == Permission::Admin {
            actor.log_delegated_denial("delegable permission");
            return Err(ApiError::invalid("the admin permission cannot be delegated"));
        }
        project_in_org(conn, &actor.org_id, &scope.project_id)?;
        if !actor.can_view(&scope.project_id) {
            return Err(ApiError::not_found("project"));
        }
        if !grants_cover(&actor.grants, &scope.project_id, scope.permission) {
            tracing::warn!(
                actor = %actor.id,
                project = %scope.project_id,
                permission = scope.permission.as_str(),
                "rejected a delegation wider than the issuer"
            );
            return Err(ApiError::forbidden(scope.permission.as_str()));
        }
    }
    Ok(())
}

/// タスクを解決し、範囲のプロジェクトと過不足なく対応することを確かめる。
fn resolve_tasks(
    conn: &Connection,
    actor: &Actor,
    scopes: &[ScopeInput],
    references: Option<&[String]>,
) -> ApiResult<BTreeSet<String>> {
    let Some(references) = references else {
        if actor.task_scope.is_some() {
            return Err(ApiError::invalid("a token limited to tasks must list tasks when delegating"));
        }
        return Ok(BTreeSet::new());
    };
    if references.is_empty() {
        return Err(ApiError::invalid("tasks must not be empty; omit it to cover whole projects"));
    }
    let mut tasks = BTreeSet::new();
    let mut projects = BTreeSet::new();
    for reference in references {
        let task = resolve_task(conn, actor, reference)?;
        if !scopes.iter().any(|scope| scope.project_id == task.project_id) {
            return Err(ApiError::invalid(format!("no scope covers the project of task {reference}")));
        }
        projects.insert(task.project_id);
        tasks.insert(task.id);
    }
    if let Some(scope) = scopes.iter().find(|scope| !projects.contains(&scope.project_id)) {
        return Err(ApiError::invalid(format!("no listed task is in project {}", scope.project_id)));
    }
    Ok(tasks)
}

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    ApiJson(request): ApiJson<DelegateRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let parent_id = current_token(&actor)?;
    let name = required_text("name", &request.name, 100)?;
    let expires_at = parse_time("expires_at", &request.expires_at)?;
    if expires_at <= now() {
        return Err(ApiError::invalid("expires_at must be in the future"));
    }
    state
        .db
        .write(move |conn| {
            let parent_expiry: Option<String> = conn.query_row(
                "SELECT expires_at FROM api_tokens WHERE id = ?1",
                [&parent_id],
                |row| row.get(0),
            )?;
            if parent_expiry.is_some_and(|limit| expires_at > limit) {
                return Err(ApiError::invalid("expires_at must not exceed the parent token's expiry"));
            }
            validate_scopes(conn, &actor, &request.scopes)?;
            let tasks = resolve_tasks(conn, &actor, &request.scopes, request.tasks.as_deref())?;

            let id = new_id();
            let token = new_secret("ank_")?;
            conn.execute(
                "INSERT INTO api_tokens
                   (id, principal_id, name, token_hash, created_by, created_at, expires_at, parent_token_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, actor.id, name, hash_secret(&token), actor.id, now(), expires_at, parent_id],
            )?;
            for scope in &request.scopes {
                conn.execute(
                    "INSERT OR IGNORE INTO token_scopes (token_id, project_id, permission) VALUES (?1, ?2, ?3)",
                    params![id, scope.project_id, scope.permission.as_str()],
                )?;
            }
            for task_id in &tasks {
                conn.execute(
                    "INSERT INTO token_tasks (token_id, task_id) VALUES (?1, ?2)",
                    params![id, task_id],
                )?;
            }
            let (scopes, task_ids) = scope_json(conn, &id)?;
            emit(conn, admin_event(&actor, "token.delegated", json!({
                "token_id": id,
                "parent_token_id": parent_id,
                "principal_id": actor.id,
                "name": name,
                "scopes": scopes,
                "tasks": task_ids,
                "expires_at": expires_at,
            })))?;
            tracing::info!(actor = %actor.id, token = %id, parent = %parent_id, "delegated token issued");
            Ok((
                StatusCode::CREATED,
                Json(json!({
                    "id": id,
                    "token": token,
                    "parent_token_id": parent_id,
                    "expires_at": expires_at,
                    "scopes": scopes,
                    "tasks": task_ids,
                })),
            ))
        })
        .await
}

/// 認証に使ったトークンから発行された委譲トークンを、子孫まで含めて返す。
pub async fn list(State(state): State<AppState>, actor: Actor) -> ApiResult<Json<Value>> {
    let token_id = current_token(&actor)?;
    state
        .db
        .read(move |conn| {
            let mut statement = conn.prepare(
                "WITH RECURSIVE descendants(id) AS (
                   SELECT id FROM api_tokens WHERE parent_token_id = ?1
                   UNION SELECT t.id FROM api_tokens t JOIN descendants d ON t.parent_token_id = d.id
                 )
                 SELECT t.id, t.name, t.parent_token_id, t.created_at, t.expires_at, t.revoked_at
                 FROM api_tokens t JOIN descendants d ON d.id = t.id
                 ORDER BY t.created_at, t.id",
            )?;
            let rows = statement
                .query_map([&token_id], |row| {
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "name": row.get::<_, String>(1)?,
                        "parent_token_id": row.get::<_, String>(2)?,
                        "created_at": row.get::<_, String>(3)?,
                        "expires_at": row.get::<_, String>(4)?,
                        "revoked_at": row.get::<_, Option<String>>(5)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut delegations = Vec::new();
            for mut delegation in rows {
                let id = delegation["id"].as_str().unwrap_or_default().to_string();
                let (scopes, tasks) = scope_json(conn, &id)?;
                delegation["scopes"] = scopes;
                delegation["tasks"] = tasks;
                delegations.push(delegation);
            }
            Ok(Json(json!({ "delegations": delegations })))
        })
        .await
}
