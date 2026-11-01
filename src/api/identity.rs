//! ログイン・サインアップ・自分自身の情報。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::admin::{SignupGrant, insert_grant, reject_delegated};
use super::{ApiJson, AppState, required_text};
use crate::auth::{
    self, Actor, Credential, SESSION_COOKIE, SESSION_TTL_SECONDS, burn_password_check, hash_password,
    hash_secret, new_secret, session_cookie_value, validate_password, verify_password,
};
use crate::db::{after_seconds, new_id, now};
use crate::error::{ApiError, ApiResult};
use crate::events::{EventInput, emit};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    email: String,
    password: String,
}

fn session_cookie(state: &AppState, value: &str, max_age: i64) -> ApiResult<HeaderValue> {
    let secure = if state.secure_cookies() { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
    ))
    .map_err(|e| ApiError::internal(format!("session cookie: {e}")))
}

fn issue_session(conn: &Connection, principal_id: &str) -> ApiResult<String> {
    let secret = new_secret("ses_")?;
    conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", [now()])?;
    conn.execute(
        "INSERT INTO sessions (token_hash, principal_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
        params![hash_secret(&secret), principal_id, now(), after_seconds(SESSION_TTL_SECONDS)],
    )?;
    Ok(secret)
}

fn invalid_login() -> ApiError {
    ApiError::unauthenticated("email or password is incorrect")
}

pub async fn login(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<LoginRequest>,
) -> ApiResult<Response> {
    let email = request.email.trim().to_lowercase();
    let blocked = state
        .login_limiter
        .lock()
        .map_err(|_| ApiError::internal("login limiter lock poisoned"))?
        .is_blocked(&email);
    if blocked {
        tracing::warn!(%email, "login blocked after repeated failures");
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            "too many failed logins; try again later",
        ));
    }

    let lookup = email.clone();
    let found: Option<(String, String, String)> = state
        .db
        .read(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT c.principal_id, c.password_hash, p.status FROM human_credentials c
                     JOIN principals p ON p.id = c.principal_id WHERE c.email = ?1",
                    [&lookup],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?)
        })
        .await?;

    let password = request.password;
    let verified = tokio::task::spawn_blocking(move || match found {
        Some((id, hash, status)) if verify_password(&password, &hash) && status == "active" => {
            Some(id)
        }
        Some(_) => None,
        None => {
            burn_password_check(&password);
            None
        }
    })
    .await
    .map_err(|e| ApiError::internal(format!("password check failed: {e}")))?;

    {
        let mut limiter = state
            .login_limiter
            .lock()
            .map_err(|_| ApiError::internal("login limiter lock poisoned"))?;
        match &verified {
            Some(_) => limiter.clear(&email),
            None => limiter.record_failure(&email),
        }
    }
    let Some(principal_id) = verified else {
        tracing::warn!(%email, "login failed");
        return Err(invalid_login());
    };

    let id = principal_id.clone();
    let secret = state.db.write(move |conn| issue_session(conn, &id)).await?;
    tracing::info!(principal = %principal_id, "login succeeded");
    let cookie = session_cookie(&state, &secret, SESSION_TTL_SECONDS)?;
    Ok(([(SET_COOKIE, cookie)], Json(json!({ "principal_id": principal_id }))).into_response())
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(value) = session_cookie_value(&headers) {
        let hash = hash_secret(&value);
        state
            .db
            .write(move |conn| {
                conn.execute("DELETE FROM sessions WHERE token_hash = ?1", [&hash])?;
                Ok(())
            })
            .await?;
    }
    let cookie = session_cookie(&state, "", 0)?;
    Ok(([(SET_COOKIE, cookie)], StatusCode::NO_CONTENT).into_response())
}

struct SignupLink {
    id: String,
    org_id: String,
    org_name: String,
    note: String,
    grants: Vec<SignupGrant>,
    expires_at: String,
}

/// 有効なリンクだけを返す。無効な理由は区別して返さず、リンクの存在を漏らさない。
fn valid_link(conn: &Connection, token: &str) -> ApiResult<SignupLink> {
    let row: Option<(String, String, String, String, String, String)> = conn
        .query_row(
            "SELECT l.id, l.org_id, o.name, l.note, l.grants_json, l.expires_at
             FROM signup_links l JOIN organizations o ON o.id = l.org_id
             WHERE l.token_hash = ?1 AND l.revoked_at IS NULL
               AND l.used_count < l.max_uses AND l.expires_at > ?2",
            params![hash_secret(token), now()],
            |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
            },
        )
        .optional()?;
    let (id, org_id, org_name, note, grants, expires_at) = row.ok_or_else(|| {
        tracing::warn!("signup link rejected");
        ApiError::new(
            StatusCode::GONE,
            "signup_link_invalid",
            "this signup link is invalid, expired or already used",
        )
    })?;
    Ok(SignupLink { id, org_id, org_name, note, grants: serde_json::from_str(&grants)?, expires_at })
}

pub async fn signup_info(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> ApiResult<Json<Value>> {
    let link = state.db.read(move |conn| valid_link(conn, &token)).await?;
    Ok(Json(json!({
        "organization": link.org_name,
        "note": link.note,
        "expires_at": link.expires_at,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignupRequest {
    email: String,
    password: String,
    display_name: String,
}

pub async fn signup(
    State(state): State<AppState>,
    Path(token): Path<String>,
    ApiJson(request): ApiJson<SignupRequest>,
) -> ApiResult<Response> {
    let email = request.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(ApiError::invalid("email is not a valid address"));
    }
    let display_name = required_text("display_name", &request.display_name, 100)?;
    validate_password(&request.password)?;

    // 計算の重いハッシュの前にリンクを確かめる。確定は下のトランザクションで再度判定する。
    let check = token.clone();
    state.db.read(move |conn| valid_link(conn, &check)).await?;
    let password = request.password;
    let password_hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|e| ApiError::internal(format!("password hashing failed: {e}")))??;

    let (principal_id, secret) = state
        .db
        .write(move |conn| {
            let link = valid_link(conn, &token)?;
            let taken: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM human_credentials WHERE email = ?1)",
                [&email],
                |row| row.get(0),
            )?;
            if taken {
                return Err(ApiError::conflict("email_taken", "this email is already registered"));
            }
            let id = new_id();
            conn.execute(
                "INSERT INTO principals (id, org_id, kind, display_name, status, created_at)
                 VALUES (?1, ?2, 'human', ?3, 'active', ?4)",
                params![id, link.org_id, display_name, now()],
            )?;
            conn.execute(
                "INSERT INTO human_credentials (principal_id, email, password_hash) VALUES (?1, ?2, ?3)",
                params![id, email, password_hash],
            )?;
            for grant in &link.grants {
                insert_grant(conn, &link.org_id, &id, grant, None)?;
            }
            conn.execute(
                "UPDATE signup_links SET used_count = used_count + 1 WHERE id = ?1",
                [&link.id],
            )?;
            emit(conn, EventInput {
                org_id: &link.org_id,
                project_id: None,
                task_id: None,
                actor_id: Some(&id),
                delegated_token_id: None,
                category: "admin",
                kind: "principal.signed_up",
                data: json!({ "principal_id": id, "signup_link_id": link.id }),
            })?;
            let secret = issue_session(conn, &id)?;
            Ok((id, secret))
        })
        .await?;

    let cookie = session_cookie(&state, &secret, SESSION_TTL_SECONDS)?;
    Ok((
        StatusCode::CREATED,
        [(SET_COOKIE, cookie)],
        Json(json!({ "principal_id": principal_id })),
    )
        .into_response())
}

pub async fn me(State(state): State<AppState>, actor: Actor) -> ApiResult<Json<Value>> {
    state
        .db
        .read(move |conn| {
            let (org_name, email): (String, Option<String>) = conn.query_row(
                "SELECT o.name, c.email FROM organizations o
                 LEFT JOIN human_credentials c ON c.principal_id = ?1 WHERE o.id = ?2",
                params![actor.id, actor.org_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let grants: Vec<Value> = actor
                .grants
                .iter()
                .map(|g| json!({ "project_id": g.project_id, "permission": g.permission }))
                .collect();
            Ok(Json(json!({
                "id": actor.id,
                "kind": actor.kind,
                "display_name": actor.display_name,
                "email": email,
                "organization": { "id": actor.org_id, "name": org_name },
                "is_org_admin": actor.is_org_admin(),
                "grants": grants,
                "delegated_token_id": actor.delegated_token_id(),
            })))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    actor: Actor,
    ApiJson(request): ApiJson<ChangePasswordRequest>,
) -> ApiResult<StatusCode> {
    reject_delegated(&actor)?;
    validate_password(&request.new_password)?;
    let id = actor.id.clone();
    let stored: Option<String> = state
        .db
        .read(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT password_hash FROM human_credentials WHERE principal_id = ?1",
                    [&id],
                    |row| row.get(0),
                )
                .optional()?)
        })
        .await?;
    let stored = stored.ok_or_else(|| ApiError::invalid("this principal has no password"))?;

    let hashed = tokio::task::spawn_blocking(move || {
        if verify_password(&request.current_password, &stored) {
            hash_password(&request.new_password).map(Some)
        } else {
            Ok(None)
        }
    })
    .await
    .map_err(|e| ApiError::internal(format!("password check failed: {e}")))??
    .ok_or_else(|| ApiError::unauthenticated("current password is incorrect"))?;

    state
        .db
        .write(move |conn| {
            replace_password(conn, &actor.id, &hashed)?;
            // 操作中のセッションだけを残し、他の端末のセッションを無効にする。
            if let Credential::Session(hash) = &actor.credential {
                conn.execute(
                    "INSERT INTO sessions (token_hash, principal_id, created_at, expires_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![hash, actor.id, now(), after_seconds(auth::SESSION_TTL_SECONDS)],
                )?;
            }
            emit(conn, EventInput {
                org_id: &actor.org_id,
                project_id: None,
                task_id: None,
                actor_id: Some(&actor.id),
                delegated_token_id: None,
                category: "admin",
                kind: "principal.password_changed",
                data: json!({ "principal_id": actor.id }),
            })?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

/// パスワードの更新と、それまでのセッションの失効を同時に行う。
pub fn replace_password(conn: &Connection, principal_id: &str, hash: &str) -> ApiResult<()> {
    let changed = conn.execute(
        "UPDATE human_credentials SET password_hash = ?1 WHERE principal_id = ?2",
        params![hash, principal_id],
    )?;
    if changed == 0 {
        return Err(ApiError::invalid("this principal has no password"));
    }
    conn.execute("DELETE FROM sessions WHERE principal_id = ?1", [principal_id])?;
    Ok(())
}
