//! 組織・プロジェクトの共有リソースと、その明示的な確保。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiJson, ApiQuery, AppState, required_text};
use crate::auth::Actor;
use crate::db::{new_id, now};
use crate::error::{ApiError, ApiResult};
use crate::events::{EventInput, emit};
use crate::model::Permission;

fn resource_event<'a>(
    actor: &'a Actor,
    project_id: Option<&'a str>,
    kind: &'static str,
    data: Value,
) -> EventInput<'a> {
    EventInput {
        org_id: &actor.org_id,
        project_id,
        task_id: None,
        actor_id: Some(&actor.id),
        delegated_token_id: actor.delegated_token_id(),
        // events.category の既存 CHECK 制約を維持しつつ、type で resource を識別する。
        category: "admin",
        kind,
        data,
    }
}

#[derive(Clone)]
struct ResourceRow {
    id: String,
    org_id: String,
    project_id: Option<String>,
    name: String,
    description: String,
    capacity: i64,
    created_at: String,
    updated_at: String,
}

fn load_resource(conn: &Connection, id: &str) -> ApiResult<ResourceRow> {
    conn.query_row(
        "SELECT id, org_id, project_id, name, description, capacity, created_at, updated_at
         FROM resources WHERE id = ?1",
        [id],
        |row| {
            Ok(ResourceRow {
                id: row.get(0)?,
                org_id: row.get(1)?,
                project_id: row.get(2)?,
                name: row.get(3)?,
                description: row.get(4)?,
                capacity: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| ApiError::not_found("resource"))
}

/// タスク限定の委譲はリソースを操作対象に持たない。存在・件数も返さない。
fn require_visible(actor: &Actor, resource: &ResourceRow) -> ApiResult<()> {
    if resource.org_id != actor.org_id || actor.task_scope.is_some() {
        return Err(ApiError::not_found("resource"));
    }
    if let Some(project_id) = &resource.project_id {
        if !actor.sees_whole_project(project_id) {
            return Err(ApiError::not_found("resource"));
        }
    }
    Ok(())
}

fn require_project_known_and_visible(
    conn: &Connection,
    actor: &Actor,
    project_id: &str,
) -> ApiResult<()> {
    let known: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1 AND org_id = ?2)",
        params![project_id, actor.org_id],
        |row| row.get(0),
    )?;
    if !known || actor.task_scope.is_some() || !actor.sees_whole_project(project_id) {
        Err(ApiError::not_found("project"))
    } else {
        Ok(())
    }
}

fn require_admin(conn: &Connection, actor: &Actor, project_id: Option<&str>) -> ApiResult<()> {
    match project_id {
        Some(project_id) => {
            require_project_known_and_visible(conn, actor, project_id)?;
            actor.require(project_id, Permission::Admin)
        }
        None => actor.require_org_admin(),
    }
}

fn can_acquire_org_resource(actor: &Actor) -> bool {
    actor.is_org_admin()
        || actor
            .grants
            .iter()
            .any(|grant| matches!(grant.permission, Permission::Claim | Permission::Admin))
}

fn require_acquire(actor: &Actor, resource: &ResourceRow) -> ApiResult<()> {
    require_visible(actor, resource)?;
    match resource.project_id.as_deref() {
        Some(project_id) => actor.require(project_id, Permission::Claim),
        None if can_acquire_org_resource(actor) => Ok(()),
        None => Err(ApiError::forbidden("claim")),
    }
}

fn holds_json(conn: &Connection, resource_id: &str) -> ApiResult<Vec<Value>> {
    let mut statement = conn.prepare(
        "SELECT h.id, h.reason, h.acquired_at, p.id, p.kind, p.display_name, p.status
         FROM resource_holds h JOIN principals p ON p.id = h.holder_id
         WHERE h.resource_id = ?1 AND h.ended_at IS NULL ORDER BY h.acquired_at, h.id",
    )?;
    statement.query_map([resource_id], |row| Ok(json!({
        "id": row.get::<_, String>(0)?, "reason": row.get::<_, String>(1)?, "acquired_at": row.get::<_, String>(2)?,
        "holder": { "id": row.get::<_, String>(3)?, "kind": row.get::<_, String>(4)?,
                    "display_name": row.get::<_, String>(5)?, "status": row.get::<_, String>(6)? },
    })))?.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn resource_json(conn: &Connection, resource: &ResourceRow) -> ApiResult<Value> {
    Ok(json!({
        "id": resource.id, "org_id": resource.org_id, "project_id": resource.project_id,
        "name": resource.name, "description": resource.description, "capacity": resource.capacity,
        "created_at": resource.created_at, "updated_at": resource.updated_at,
        "holds": holds_json(conn, &resource.id)?,
    }))
}

fn scope_name_taken(
    conn: &Connection,
    org_id: &str,
    project_id: Option<&str>,
    name: &str,
    except: Option<&str>,
) -> ApiResult<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM resources WHERE org_id = ?1 AND project_id IS ?2 AND name = ?3
          AND (?4 IS NULL OR id <> ?4))",
        params![org_id, project_id, name, except],
        |row| row.get(0),
    )?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    project_id: Option<String>,
    scope: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    actor: Actor,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> ApiResult<Json<Value>> {
    state.db.read(move |conn| {
        if actor.task_scope.is_some() { return Ok(Json(json!({ "resources": [] }))); }
        if let Some(project_id) = query.project_id.as_deref() { require_project_known_and_visible(conn, &actor, project_id)?; }
        if let Some(scope) = query.scope.as_deref() {
            if !matches!(scope, "organization" | "project") { return Err(ApiError::invalid("scope must be organization or project")); }
            if scope == "organization" && query.project_id.is_some() { return Err(ApiError::invalid("organization scope cannot include project_id")); }
        }
        let mut statement = conn.prepare(
            "SELECT id FROM resources WHERE org_id = ?1 ORDER BY project_id IS NOT NULL, project_id, name, id",
        )?;
        let ids = statement.query_map([&actor.org_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut resources = Vec::new();
        for id in ids {
            let resource = load_resource(conn, &id)?;
            let matches_project = query.project_id.as_ref().is_none_or(|project| resource.project_id.as_ref() == Some(project));
            let matches_scope = match query.scope.as_deref() {
                Some("organization") => resource.project_id.is_none(), Some("project") => resource.project_id.is_some(), _ => true,
            };
            if matches_project && matches_scope && require_visible(&actor, &resource).is_ok() {
                resources.push(resource_json(conn, &resource)?);
            }
        }
        Ok(Json(json!({ "resources": resources })))
    }).await
}

pub async fn get(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    state
        .db
        .read(move |conn| {
            let resource = load_resource(conn, &id)?;
            require_visible(&actor, &resource)?;
            Ok(Json(resource_json(conn, &resource)?))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    project_id: Option<String>,
    name: String,
    #[serde(default)]
    description: String,
    capacity: i64,
}

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    ApiJson(request): ApiJson<CreateRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let name = required_text("name", &request.name, 100)?;
    if request.capacity < 1 {
        return Err(ApiError::invalid("capacity must be at least 1"));
    }
    state.db.write(move |conn| {
        require_admin(conn, &actor, request.project_id.as_deref())?;
        if scope_name_taken(conn, &actor.org_id, request.project_id.as_deref(), &name, None)? {
            return Err(ApiError::conflict("duplicate_resource_name", format!("resource '{name}' already exists in this scope")));
        }
        let id = new_id(); let moment = now();
        conn.execute("INSERT INTO resources (id, org_id, project_id, name, description, capacity, created_by, created_at, updated_at)
                      VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![id, actor.org_id, request.project_id, name, request.description, request.capacity, actor.id, moment])?;
        let resource = load_resource(conn, &id)?;
        emit(conn, resource_event(&actor, resource.project_id.as_deref(), "resource.created", json!({
            "resource_id": id, "name": resource.name, "capacity": resource.capacity, "project_id": resource.project_id,
        })))?;
        tracing::info!(resource = %id, actor = %actor.id, "created shared resource");
        Ok((StatusCode::CREATED, Json(resource_json(conn, &resource)?)))
    }).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    name: Option<String>,
    description: Option<String>,
    capacity: Option<i64>,
}

pub async fn update(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<UpdateRequest>,
) -> ApiResult<Json<Value>> {
    state.db.write(move |conn| {
        let resource = load_resource(conn, &id)?; require_visible(&actor, &resource)?;
        require_admin(conn, &actor, resource.project_id.as_deref())?;
        if request.name.is_none() && request.description.is_none() && request.capacity.is_none() { return Err(ApiError::invalid("provide name, description or capacity")); }
        let name = match request.name.as_deref() { Some(name) => required_text("name", name, 100)?, None => resource.name.clone() };
        let capacity = request.capacity.unwrap_or(resource.capacity);
        if capacity < 1 { return Err(ApiError::invalid("capacity must be at least 1")); }
        let held = holds_json(conn, &resource.id)?.len() as i64;
        if capacity < held { return Err(ApiError::conflict("resource_capacity_in_use", "capacity cannot be below active holds").details(json!({ "active_holds": held, "capacity": resource.capacity }))); }
        if name != resource.name && scope_name_taken(conn, &resource.org_id, resource.project_id.as_deref(), &name, Some(&resource.id))? {
            return Err(ApiError::conflict("duplicate_resource_name", format!("resource '{name}' already exists in this scope")));
        }
        let description = request.description.unwrap_or(resource.description.clone());
        conn.execute("UPDATE resources SET name = ?1, description = ?2, capacity = ?3, updated_at = ?4 WHERE id = ?5",
            params![name, description, capacity, now(), resource.id])?;
        let changed = load_resource(conn, &resource.id)?;
        emit(conn, resource_event(&actor, changed.project_id.as_deref(), "resource.updated", json!({
            "resource_id": changed.id, "name": changed.name, "capacity": changed.capacity,
        })))?;
        tracing::info!(resource = %changed.id, actor = %actor.id, "updated shared resource");
        Ok(Json(resource_json(conn, &changed)?))
    }).await
}

pub async fn delete(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state
        .db
        .write(move |conn| {
            let resource = load_resource(conn, &id)?;
            require_visible(&actor, &resource)?;
            require_admin(conn, &actor, resource.project_id.as_deref())?;
            if !holds_json(conn, &resource.id)?.is_empty() {
                return Err(ApiError::conflict(
                    "resource_held",
                    "release or revoke all holds before deleting this resource",
                ));
            }
            conn.execute("DELETE FROM resources WHERE id = ?1", [&resource.id])?;
            emit(
                conn,
                resource_event(
                    &actor,
                    resource.project_id.as_deref(),
                    "resource.deleted",
                    json!({ "resource_id": resource.id, "name": resource.name }),
                ),
            )?;
            tracing::info!(resource = %id, actor = %actor.id, "deleted shared resource");
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquireRequest {
    reason: String,
}

pub async fn acquire(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<AcquireRequest>,
) -> ApiResult<Json<Value>> {
    let reason = required_text("reason", &request.reason, 500)?;
    state.db.write(move |conn| {
        let resource = load_resource(conn, &id)?; require_acquire(&actor, &resource)?;
        let holds = holds_json(conn, &resource.id)?;
        if holds.len() as i64 >= resource.capacity {
            tracing::info!(resource = %resource.id, actor = %actor.id, capacity = resource.capacity, "rejected resource acquire because it is busy");
            return Err(ApiError::conflict("resource_busy", "all resource slots are currently held").details(json!({ "holders": holds, "capacity": resource.capacity })));
        }
        let hold_id = new_id(); let acquired_at = now();
        conn.execute("INSERT INTO resource_holds (id, resource_id, holder_id, reason, acquired_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![hold_id, resource.id, actor.id, reason, acquired_at])?;
        emit(conn, resource_event(&actor, resource.project_id.as_deref(), "resource.acquired", json!({
            "resource_id": resource.id, "resource_name": resource.name, "hold_id": hold_id, "reason": reason,
        })))?;
        tracing::info!(resource = %resource.id, hold = %hold_id, actor = %actor.id, "acquired shared resource");
        Ok(Json(json!({ "hold_id": hold_id, "resource": resource_json(conn, &resource)? })))
    }).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldRef {
    hold_id: String,
}

fn active_hold_owner(conn: &Connection, resource_id: &str, hold_id: &str) -> ApiResult<String> {
    conn.query_row("SELECT holder_id FROM resource_holds WHERE id = ?1 AND resource_id = ?2 AND ended_at IS NULL", params![hold_id, resource_id], |row| row.get(0))
        .optional()?.ok_or_else(|| ApiError::conflict("stale_hold", "this hold is no longer active"))
}

pub async fn release(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<HoldRef>,
) -> ApiResult<Json<Value>> {
    state.db.write(move |conn| {
        let resource = load_resource(conn, &id)?; require_visible(&actor, &resource)?;
        let holder = active_hold_owner(conn, &resource.id, &request.hold_id)?;
        if holder != actor.id { tracing::warn!(resource = %resource.id, hold = %request.hold_id, actor = %actor.id, "rejected release by non-holder"); return Err(ApiError::conflict("hold_not_owned", "only the holder can release this hold")); }
        conn.execute("UPDATE resource_holds SET ended_at = ?1, end_reason = 'released', ended_by = ?2 WHERE id = ?3", params![now(), actor.id, request.hold_id])?;
        emit(conn, resource_event(&actor, resource.project_id.as_deref(), "resource.released", json!({
            "resource_id": resource.id, "resource_name": resource.name, "hold_id": request.hold_id,
        })))?;
        tracing::info!(resource = %resource.id, hold = %request.hold_id, actor = %actor.id, "released shared resource");
        Ok(Json(resource_json(conn, &resource)?))
    }).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeRequest {
    hold_id: String,
    reason: String,
}

pub async fn revoke(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<RevokeRequest>,
) -> ApiResult<Json<Value>> {
    let reason = required_text("reason", &request.reason, 500)?;
    state.db.write(move |conn| {
        let resource = load_resource(conn, &id)?; require_visible(&actor, &resource)?; require_admin(conn, &actor, resource.project_id.as_deref())?;
        let holder = active_hold_owner(conn, &resource.id, &request.hold_id)?;
        conn.execute("UPDATE resource_holds SET ended_at = ?1, end_reason = 'revoked', ended_by = ?2, revoke_reason = ?3 WHERE id = ?4", params![now(), actor.id, reason, request.hold_id])?;
        emit(conn, resource_event(&actor, resource.project_id.as_deref(), "resource.revoked", json!({
            "resource_id": resource.id, "resource_name": resource.name, "hold_id": request.hold_id, "holder_id": holder, "reason": reason,
        })))?;
        tracing::info!(resource = %resource.id, hold = %request.hold_id, actor = %actor.id, "revoked shared resource hold");
        Ok(Json(resource_json(conn, &resource)?))
    }).await
}
