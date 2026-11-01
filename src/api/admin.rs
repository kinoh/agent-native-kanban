//! 主体・権限・認証情報・サインアップリンクの管理。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::delegation::{scope_json, token_is_within};
use super::identity::replace_password;
use super::{ApiJson, AppState, explicit, required_text};
use crate::auth::{Actor, hash_password, hash_secret, new_secret, validate_password};
use crate::db::{after_seconds, new_id, now, parse_time};
use crate::error::{ApiError, ApiResult};
use crate::events::{EventInput, emit};
use crate::model::Permission;

const MAX_SIGNUP_LINK_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignupGrant {
    pub project_id: Option<String>,
    pub permission: Permission,
}

pub fn admin_event<'a>(actor: &'a Actor, kind: &'static str, data: Value) -> EventInput<'a> {
    EventInput {
        org_id: &actor.org_id,
        project_id: None,
        task_id: None,
        actor_id: Some(&actor.id),
        delegated_token_id: actor.delegated_token_id(),
        category: "admin",
        kind,
        data,
    }
}

fn principal_json(conn: &Connection, org_id: &str, id: &str, with_email: bool) -> ApiResult<Value> {
    conn.query_row(
        "SELECT p.id, p.kind, p.display_name, p.description, p.status, p.created_by, p.created_at,
                p.deleted_at, c.email
         FROM principals p LEFT JOIN human_credentials c ON c.principal_id = p.id
         WHERE p.id = ?1 AND p.org_id = ?2",
        params![id, org_id],
        |row| {
            let mut value = json!({
                "id": row.get::<_, String>(0)?,
                "kind": row.get::<_, String>(1)?,
                "display_name": row.get::<_, String>(2)?,
                "description": row.get::<_, String>(3)?,
                "status": row.get::<_, String>(4)?,
                "created_by": row.get::<_, Option<String>>(5)?,
                "created_at": row.get::<_, String>(6)?,
                "deleted_at": row.get::<_, Option<String>>(7)?,
            });
            if with_email {
                value["email"] = json!(row.get::<_, Option<String>>(8)?);
            }
            Ok(value)
        },
    )
    .optional()?
    .ok_or_else(|| ApiError::not_found("principal"))
}

pub async fn list_principals(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Value>> {
    state
        .db
        .read(move |conn| {
            let ids = {
                let mut statement = conn.prepare(
                    "SELECT id FROM principals WHERE org_id = ?1 ORDER BY display_name, id",
                )?;
                statement
                    .query_map([&actor.org_id], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let admin = actor.is_org_admin();
            let principals = ids
                .iter()
                .map(|id| principal_json(conn, &actor.org_id, id, admin))
                .collect::<ApiResult<Vec<_>>>()?;
            Ok(Json(json!({ "principals": principals })))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePrincipalRequest {
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    grants: Vec<SignupGrant>,
}

/// API から直接作れるのはエージェントだけ。人間はサインアップリンクから登録する。
pub async fn create_principal(
    State(state): State<AppState>,
    actor: Actor,
    ApiJson(request): ApiJson<CreatePrincipalRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    actor.require_org_admin()?;
    let display_name = required_text("display_name", &request.display_name, 100)?;
    state
        .db
        .write(move |conn| {
            let id = new_id();
            conn.execute(
                "INSERT INTO principals (id, org_id, kind, display_name, description, status, created_by, created_at)
                 VALUES (?1, ?2, 'agent', ?3, ?4, 'active', ?5, ?6)",
                params![id, actor.org_id, display_name, request.description, actor.id, now()],
            )?;
            for grant in &request.grants {
                insert_grant(conn, &actor.org_id, &id, grant, Some(&actor.id))?;
            }
            emit(conn, admin_event(&actor, "principal.created", json!({ "principal_id": id })))?;
            Ok((StatusCode::CREATED, Json(principal_json(conn, &actor.org_id, &id, true)?)))
        })
        .await
}

pub async fn get_principal(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    state
        .db
        .read(move |conn| {
            let with_email = actor.is_org_admin() || actor.id == id;
            Ok(Json(principal_json(conn, &actor.org_id, &id, with_email)?))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePrincipalRequest {
    display_name: Option<String>,
    description: Option<String>,
    status: Option<String>,
}

fn status_of(conn: &Connection, org_id: &str, id: &str) -> ApiResult<String> {
    conn.query_row(
        "SELECT status FROM principals WHERE id = ?1 AND org_id = ?2",
        params![id, org_id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| ApiError::not_found("principal"))
}

/// 組織を管理できる人間が1人もいなくなる操作を拒否する。
fn ensure_other_admin(conn: &Connection, org_id: &str, principal_id: &str) -> ApiResult<()> {
    let is_admin: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM grants g JOIN principals p ON p.id = g.principal_id
          WHERE g.principal_id = ?1 AND g.project_id IS NULL AND g.permission = 'admin'
            AND p.kind = 'human' AND p.status = 'active')",
        [principal_id],
        |row| row.get(0),
    )?;
    if !is_admin {
        return Ok(());
    }
    let others: i64 = conn.query_row(
        "SELECT COUNT(*) FROM grants g JOIN principals p ON p.id = g.principal_id
         WHERE p.org_id = ?1 AND p.id <> ?2 AND g.project_id IS NULL AND g.permission = 'admin'
           AND p.kind = 'human' AND p.status = 'active'",
        params![org_id, principal_id],
        |row| row.get(0),
    )?;
    if others == 0 {
        return Err(ApiError::conflict(
            "last_admin",
            "the organization must keep at least one active human administrator",
        ));
    }
    Ok(())
}

pub async fn update_principal(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<UpdatePrincipalRequest>,
) -> ApiResult<Json<Value>> {
    actor.require_org_admin()?;
    state
        .db
        .write(move |conn| {
            let current = status_of(conn, &actor.org_id, &id)?;
            if current == "deleted" {
                return Err(ApiError::conflict("principal_deleted", "this principal was deleted"));
            }
            if let Some(name) = &request.display_name {
                let name = required_text("display_name", name, 100)?;
                conn.execute("UPDATE principals SET display_name = ?1 WHERE id = ?2", params![name, id])?;
            }
            if let Some(description) = &request.description {
                conn.execute(
                    "UPDATE principals SET description = ?1 WHERE id = ?2",
                    params![description, id],
                )?;
            }
            if let Some(status) = &request.status {
                if !matches!(status.as_str(), "active" | "disabled") {
                    return Err(ApiError::invalid("status must be active or disabled"));
                }
                if status == "disabled" {
                    ensure_other_admin(conn, &actor.org_id, &id)?;
                }
                conn.execute("UPDATE principals SET status = ?1 WHERE id = ?2", params![status, id])?;
            }
            emit(conn, admin_event(&actor, "principal.updated", json!({
                "principal_id": id,
                "display_name": request.display_name,
                "status": request.status,
            })))?;
            Ok(Json(principal_json(conn, &actor.org_id, &id, true)?))
        })
        .await
}

/// 主体を退職扱いにする。ID・権限以外の作業記録は残す。
/// 認証情報は全て失効させ、人間の個人情報であるメールとパスワードは消す。
/// 占有は外さない。解除は管理者が明示的に行う。
pub async fn delete_principal(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    actor.require_org_admin()?;
    state
        .db
        .write(move |conn| {
            let current = status_of(conn, &actor.org_id, &id)?;
            if current == "deleted" {
                return Ok(StatusCode::NO_CONTENT);
            }
            ensure_other_admin(conn, &actor.org_id, &id)?;
            let moment = now();
            conn.execute(
                "UPDATE principals SET status = 'deleted', deleted_at = ?1 WHERE id = ?2",
                params![moment, id],
            )?;
            conn.execute("DELETE FROM human_credentials WHERE principal_id = ?1", [&id])?;
            conn.execute("DELETE FROM sessions WHERE principal_id = ?1", [&id])?;
            conn.execute("DELETE FROM grants WHERE principal_id = ?1", [&id])?;
            conn.execute(
                "UPDATE api_tokens SET revoked_at = ?1 WHERE principal_id = ?2 AND revoked_at IS NULL",
                params![moment, id],
            )?;
            // 退職者が発行した未使用のリンクで登録が続かないようにする。
            conn.execute(
                "UPDATE signup_links SET revoked_at = ?1 WHERE created_by = ?2 AND revoked_at IS NULL",
                params![moment, id],
            )?;
            emit(conn, admin_event(&actor, "principal.deleted", json!({ "principal_id": id })))?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPasswordRequest {
    password: String,
}

pub async fn set_password(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<SetPasswordRequest>,
) -> ApiResult<StatusCode> {
    actor.require_org_admin()?;
    validate_password(&request.password)?;
    let hash = tokio::task::spawn_blocking(move || hash_password(&request.password))
        .await
        .map_err(|e| ApiError::internal(format!("password hashing failed: {e}")))??;
    state
        .db
        .write(move |conn| {
            status_of(conn, &actor.org_id, &id)?;
            replace_password(conn, &id, &hash)?;
            emit(conn, admin_event(&actor, "principal.password_reset", json!({ "principal_id": id })))?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub fn project_in_org(conn: &Connection, org_id: &str, project_id: &str) -> ApiResult<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1 AND org_id = ?2)",
        params![project_id, org_id],
        |row| row.get(0),
    )?;
    if exists { Ok(()) } else { Err(ApiError::not_found("project")) }
}

pub fn insert_grant(
    conn: &Connection,
    org_id: &str,
    principal_id: &str,
    grant: &SignupGrant,
    granted_by: Option<&str>,
) -> ApiResult<String> {
    if let Some(project_id) = &grant.project_id {
        project_in_org(conn, org_id, project_id)?;
    }
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM grants WHERE principal_id = ?1 AND COALESCE(project_id, '') = ?2
             AND permission = ?3",
            params![
                principal_id,
                grant.project_id.as_deref().unwrap_or(""),
                grant.permission.as_str()
            ],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO grants (id, org_id, principal_id, project_id, permission, granted_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, org_id, principal_id, grant.project_id, grant.permission.as_str(), granted_by, now()],
    )?;
    Ok(id)
}

fn require_grant_authority(actor: &Actor, project_id: Option<&str>) -> ApiResult<()> {
    match project_id {
        None => actor.require_org_admin(),
        Some(project_id) => actor.require(project_id, Permission::Admin),
    }
}

pub async fn list_grants(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    state
        .db
        .read(move |conn| {
            status_of(conn, &actor.org_id, &id)?;
            let mut statement = conn.prepare(
                "SELECT id, project_id, permission, granted_by, created_at FROM grants
                 WHERE principal_id = ?1 ORDER BY created_at",
            )?;
            let rows = statement.query_map([&id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            let mut grants = Vec::new();
            for row in rows {
                let (grant_id, project_id, permission, granted_by, created_at) = row?;
                // 自分と組織管理者以外には、閲覧できるプロジェクトの権限だけ見せる。
                let visible = (actor.id == id && !actor.delegated)
                    || actor.is_org_admin()
                    || project_id.as_deref().is_some_and(|p| actor.can_view(p));
                if visible {
                    grants.push(json!({
                        "id": grant_id,
                        "project_id": project_id,
                        "permission": permission,
                        "granted_by": granted_by,
                        "created_at": created_at,
                    }));
                }
            }
            Ok(Json(json!({ "grants": grants })))
        })
        .await
}

pub async fn add_grant(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<SignupGrant>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_grant_authority(&actor, request.project_id.as_deref())?;
    state
        .db
        .write(move |conn| {
            if status_of(conn, &actor.org_id, &id)? == "deleted" {
                return Err(ApiError::conflict("principal_deleted", "this principal was deleted"));
            }
            let grant_id = insert_grant(conn, &actor.org_id, &id, &request, Some(&actor.id))?;
            emit(conn, admin_event(&actor, "grant.added", json!({
                "grant_id": grant_id,
                "principal_id": id,
                "project_id": request.project_id,
                "permission": request.permission,
            })))?;
            Ok((StatusCode::CREATED, Json(json!({ "id": grant_id }))))
        })
        .await
}

pub async fn delete_grant(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state
        .db
        .write(move |conn| {
            let grant: Option<(String, Option<String>, String)> = conn
                .query_row(
                    "SELECT principal_id, project_id, permission FROM grants WHERE id = ?1 AND org_id = ?2",
                    params![id, actor.org_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            let (principal_id, project_id, permission) =
                grant.ok_or_else(|| ApiError::not_found("grant"))?;
            require_grant_authority(&actor, project_id.as_deref())?;
            if project_id.is_none() && permission == "admin" {
                ensure_other_admin(conn, &actor.org_id, &principal_id)?;
            }
            conn.execute("DELETE FROM grants WHERE id = ?1", [&id])?;
            emit(conn, admin_event(&actor, "grant.removed", json!({
                "grant_id": id,
                "principal_id": principal_id,
                "project_id": project_id,
                "permission": permission,
            })))?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

/// 認証情報の管理は、委譲の範囲を抜ける経路になるので委譲トークンには許さない。
pub fn reject_delegated(actor: &Actor) -> ApiResult<()> {
    if actor.delegated {
        actor.log_delegated_denial("credential management");
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "delegated_token_restricted",
            "delegated tokens cannot manage credentials",
        ));
    }
    Ok(())
}

fn require_token_authority(actor: &Actor, principal_id: &str) -> ApiResult<()> {
    reject_delegated(actor)?;
    if actor.is_org_admin() || (actor.id == principal_id && actor.kind == "human") {
        Ok(())
    } else {
        Err(ApiError::forbidden("admin"))
    }
}

pub async fn list_tokens(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_token_authority(&actor, &id)?;
    state
        .db
        .read(move |conn| {
            status_of(conn, &actor.org_id, &id)?;
            let mut statement = conn.prepare(
                "SELECT id, name, created_by, created_at, expires_at, revoked_at, parent_token_id
                 FROM api_tokens WHERE principal_id = ?1 ORDER BY created_at DESC",
            )?;
            let rows = statement
                .query_map([&id], |row| {
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "name": row.get::<_, String>(1)?,
                        "created_by": row.get::<_, String>(2)?,
                        "created_at": row.get::<_, String>(3)?,
                        "expires_at": row.get::<_, Option<String>>(4)?,
                        "revoked_at": row.get::<_, Option<String>>(5)?,
                        "parent_token_id": row.get::<_, Option<String>>(6)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut tokens = Vec::new();
            for mut token in rows {
                if !token["parent_token_id"].is_null() {
                    let id = token["id"].as_str().unwrap_or_default().to_string();
                    let (scopes, tasks) = scope_json(conn, &id)?;
                    token["scopes"] = scopes;
                    token["tasks"] = tasks;
                }
                tokens.push(token);
            }
            Ok(Json(json!({ "tokens": tokens })))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTokenRequest {
    name: String,
    #[serde(default, deserialize_with = "explicit")]
    expires_at: Option<Option<String>>,
}

pub async fn create_token(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<CreateTokenRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_token_authority(&actor, &id)?;
    let name = required_text("name", &request.name, 100)?;
    let expires_at = request
        .expires_at
        .flatten()
        .map(|value| parse_time("expires_at", &value))
        .transpose()?;
    state
        .db
        .write(move |conn| {
            if status_of(conn, &actor.org_id, &id)? != "active" {
                return Err(ApiError::conflict("principal_inactive", "this principal is not active"));
            }
            let token_id = new_id();
            let token = new_secret("ank_")?;
            conn.execute(
                "INSERT INTO api_tokens (id, principal_id, name, token_hash, created_by, created_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![token_id, id, name, hash_secret(&token), actor.id, now(), expires_at],
            )?;
            emit(conn, admin_event(&actor, "token.created", json!({
                "token_id": token_id,
                "principal_id": id,
                "name": name,
            })))?;
            Ok((
                StatusCode::CREATED,
                Json(json!({ "id": token_id, "token": token, "expires_at": expires_at })),
            ))
        })
        .await
}

pub async fn revoke_token(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state
        .db
        .write(move |conn| {
            let principal_id: Option<String> = conn
                .query_row(
                    "SELECT t.principal_id FROM api_tokens t JOIN principals p ON p.id = t.principal_id
                     WHERE t.id = ?1 AND p.org_id = ?2",
                    params![id, actor.org_id],
                    |row| row.get(0),
                )
                .optional()?;
            let principal_id = principal_id.ok_or_else(|| ApiError::not_found("token"))?;
            // トークンは、自分自身と自分から発行した委譲トークンを失効できる。
            let issued_here = match &actor.token_id {
                Some(token_id) => token_is_within(conn, token_id, &id)?,
                None => false,
            };
            if !issued_here {
                require_token_authority(&actor, &principal_id)?;
            }
            conn.execute(
                "UPDATE api_tokens SET revoked_at = COALESCE(revoked_at, ?1) WHERE id = ?2",
                params![now(), id],
            )?;
            emit(conn, admin_event(&actor, "token.revoked", json!({
                "token_id": id,
                "principal_id": principal_id,
            })))?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub async fn list_signup_links(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Value>> {
    actor.require_org_admin()?;
    state
        .db
        .read(move |conn| {
            let mut statement = conn.prepare(
                "SELECT id, note, grants_json, max_uses, used_count, expires_at, created_by,
                        created_at, revoked_at
                 FROM signup_links WHERE org_id = ?1 ORDER BY created_at DESC",
            )?;
            let links = statement
                .query_map([&actor.org_id], |row| {
                    let grants: String = row.get(2)?;
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "note": row.get::<_, String>(1)?,
                        "grants": serde_json::from_str::<Value>(&grants).unwrap_or(Value::Null),
                        "max_uses": row.get::<_, i64>(3)?,
                        "used_count": row.get::<_, i64>(4)?,
                        "expires_at": row.get::<_, String>(5)?,
                        "created_by": row.get::<_, Option<String>>(6)?,
                        "created_at": row.get::<_, String>(7)?,
                        "revoked_at": row.get::<_, Option<String>>(8)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(json!({ "signup_links": links })))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSignupLinkRequest {
    note: String,
    expires_in_seconds: i64,
    max_uses: i64,
    grants: Vec<SignupGrant>,
}

/// サインアップリンクを作り、秘密を含む URL を返す。URL が見られるのはこの応答の1度だけ。
pub fn create_link(
    conn: &Connection,
    org_id: &str,
    created_by: Option<&str>,
    public_url: &str,
    note: &str,
    expires_in_seconds: i64,
    max_uses: i64,
    grants: &[SignupGrant],
) -> ApiResult<Value> {
    if !(60..=MAX_SIGNUP_LINK_SECONDS).contains(&expires_in_seconds) {
        return Err(ApiError::invalid(format!(
            "expires_in_seconds must be between 60 and {MAX_SIGNUP_LINK_SECONDS}"
        )));
    }
    if max_uses < 1 {
        return Err(ApiError::invalid("max_uses must be at least 1"));
    }
    for grant in grants {
        if let Some(project_id) = &grant.project_id {
            project_in_org(conn, org_id, project_id)?;
        }
    }
    let id = new_id();
    let token = new_secret("snp_")?;
    let expires_at = after_seconds(expires_in_seconds);
    conn.execute(
        "INSERT INTO signup_links (id, org_id, token_hash, note, grants_json, max_uses, expires_at, created_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            org_id,
            hash_secret(&token),
            note,
            serde_json::to_string(grants)?,
            max_uses,
            expires_at,
            created_by,
            now()
        ],
    )?;
    emit(conn, EventInput {
        org_id,
        project_id: None,
        task_id: None,
        actor_id: created_by,
        delegated_token_id: None,
        category: "admin",
        kind: "signup_link.created",
        data: json!({ "signup_link_id": id, "max_uses": max_uses, "expires_at": expires_at }),
    })?;
    Ok(json!({
        "id": id,
        "url": format!("{public_url}/signup/{token}"),
        "expires_at": expires_at,
        "max_uses": max_uses,
    }))
}

pub async fn create_signup_link(
    State(state): State<AppState>,
    actor: Actor,
    ApiJson(request): ApiJson<CreateSignupLinkRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    actor.require_org_admin()?;
    let note = required_text("note", &request.note, 200)?;
    let public_url = state.public_url.clone();
    state
        .db
        .write(move |conn| {
            let link = create_link(
                conn,
                &actor.org_id,
                Some(&actor.id),
                &public_url,
                &note,
                request.expires_in_seconds,
                request.max_uses,
                &request.grants,
            )?;
            Ok((StatusCode::CREATED, Json(link)))
        })
        .await
}

pub async fn revoke_signup_link(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    actor.require_org_admin()?;
    state
        .db
        .write(move |conn| {
            let changed = conn.execute(
                "UPDATE signup_links SET revoked_at = COALESCE(revoked_at, ?1)
                 WHERE id = ?2 AND org_id = ?3",
                params![now(), id, actor.org_id],
            )?;
            if changed == 0 {
                return Err(ApiError::not_found("signup link"));
            }
            emit(conn, admin_event(&actor, "signup_link.revoked", json!({ "signup_link_id": id })))?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}
