//! 認証情報の扱いと、リクエストごとの主体（Actor）の解決。

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::header::{AUTHORIZATION, COOKIE, ORIGIN};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, StatusCode};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::api::AppState;
use crate::db::now;
use crate::error::{ApiError, ApiResult};
use crate::model::Permission;

pub const SESSION_COOKIE: &str = "ank_session";
pub const SESSION_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
pub const MIN_PASSWORD_LENGTH: usize = 12;

const MAX_LOGIN_FAILURES: u32 = 10;
const LOGIN_FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);

/// 暗号学的乱数の秘密を発行する。DB にはハッシュだけを保存する。
pub fn new_secret(prefix: &str) -> ApiResult<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| ApiError::internal(format!("random source: {e}")))?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{prefix}{hex}"))
}

pub fn hash_secret(secret: &str) -> String {
    Sha256::digest(secret.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hash_password(password: &str) -> ApiResult<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|e| ApiError::internal(format!("password hashing: {e}")))
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        tracing::error!("stored password hash could not be parsed");
        return false;
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

/// 未登録のメールでも同じ計算量を消費し、応答時間から登録の有無を判別させない。
pub fn burn_password_check(password: &str) {
    static DUMMY: OnceLock<String> = OnceLock::new();
    let hash = DUMMY.get_or_init(|| {
        hash_password("dummy-password-for-timing").expect("hashing a constant password")
    });
    verify_password(password, hash);
}

pub fn validate_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < MIN_PASSWORD_LENGTH {
        return Err(ApiError::invalid(format!(
            "password must be at least {MIN_PASSWORD_LENGTH} characters"
        )));
    }
    Ok(())
}

/// メールアドレス単位のログイン失敗回数。プロセス内の記録で、再起動で消える。
#[derive(Default)]
pub struct LoginLimiter {
    failures: HashMap<String, (u32, Instant)>,
}

impl LoginLimiter {
    pub fn is_blocked(&mut self, email: &str) -> bool {
        self.failures.retain(|_, (_, since)| since.elapsed() < LOGIN_FAILURE_WINDOW);
        self.failures.get(email).is_some_and(|(count, _)| *count >= MAX_LOGIN_FAILURES)
    }

    pub fn record_failure(&mut self, email: &str) {
        let entry = self.failures.entry(email.to_string()).or_insert((0, Instant::now()));
        entry.0 += 1;
    }

    pub fn clear(&mut self, email: &str) {
        self.failures.remove(email);
    }
}

#[derive(Debug, Clone)]
pub enum Credential {
    Session(String),
    Token(String),
}

#[derive(Debug, Clone)]
pub struct Grant {
    pub project_id: Option<String>,
    pub permission: Permission,
}

/// 認証済みの主体。人間もエージェントも同じ型で、権限判定に種別を使わない。
#[derive(Debug, Clone)]
pub struct Actor {
    pub id: String,
    pub org_id: String,
    pub kind: String,
    pub display_name: String,
    pub grants: Vec<Grant>,
    pub credential: Credential,
}

impl Actor {
    pub fn is_org_admin(&self) -> bool {
        self.grants.iter().any(|g| g.project_id.is_none() && g.permission == Permission::Admin)
    }

    fn grants_on<'a>(&'a self, project_id: &'a str) -> impl Iterator<Item = &'a Grant> + 'a {
        self.grants.iter().filter(move |g| match &g.project_id {
            None => true,
            Some(id) => id == project_id,
        })
    }

    /// admin はそのスコープの全権限を含む。
    pub fn can(&self, project_id: &str, permission: Permission) -> bool {
        self.grants_on(project_id)
            .any(|g| g.permission == permission || g.permission == Permission::Admin)
    }

    /// プロジェクトに何らかの権限を持つ主体は閲覧できる。
    pub fn can_view(&self, project_id: &str) -> bool {
        self.grants_on(project_id).next().is_some()
    }

    /// 進捗・コメント・成果物を記録できるか。作業に関わる権限のいずれかがあればよい。
    pub fn can_contribute(&self, project_id: &str) -> bool {
        [Permission::Edit, Permission::Claim, Permission::Review, Permission::Approve]
            .into_iter()
            .any(|p| self.can(project_id, p))
    }

    pub fn require(&self, project_id: &str, permission: Permission) -> ApiResult<()> {
        if self.can(project_id, permission) {
            Ok(())
        } else {
            Err(ApiError::forbidden(permission.as_str()))
        }
    }

    pub fn require_org_admin(&self) -> ApiResult<()> {
        if self.is_org_admin() { Ok(()) } else { Err(ApiError::forbidden("admin")) }
    }
}

pub fn load_grants(conn: &Connection, principal_id: &str) -> ApiResult<Vec<Grant>> {
    let mut statement =
        conn.prepare_cached("SELECT project_id, permission FROM grants WHERE principal_id = ?1")?;
    let rows = statement.query_map([principal_id], |row| {
        Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut grants = Vec::new();
    for row in rows {
        let (project_id, permission) = row?;
        let permission = Permission::parse(&permission).ok_or_else(|| {
            ApiError::internal(format!("unknown permission in database: {permission}"))
        })?;
        grants.push(Grant { project_id, permission });
    }
    Ok(grants)
}

pub fn load_actor(conn: &Connection, credential: &Credential) -> ApiResult<Actor> {
    let moment = now();
    let principal_id: Option<String> = match credential {
        Credential::Session(hash) => conn
            .query_row(
                "SELECT principal_id FROM sessions WHERE token_hash = ?1 AND expires_at > ?2",
                params![hash, moment],
                |row| row.get(0),
            )
            .optional()?,
        Credential::Token(hash) => conn
            .query_row(
                "SELECT principal_id FROM api_tokens WHERE token_hash = ?1 AND revoked_at IS NULL
                 AND (expires_at IS NULL OR expires_at > ?2)",
                params![hash, moment],
                |row| row.get(0),
            )
            .optional()?,
    };
    let Some(principal_id) = principal_id else {
        tracing::warn!("rejected an unknown, expired or revoked credential");
        return Err(ApiError::unauthenticated("credential is invalid or expired"));
    };
    let principal: Option<(String, String, String, String)> = conn
        .query_row(
            "SELECT org_id, kind, display_name, status FROM principals WHERE id = ?1",
            [&principal_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((org_id, kind, display_name, status)) = principal else {
        return Err(ApiError::internal(format!("credential for missing principal {principal_id}")));
    };
    if status != "active" {
        tracing::warn!(principal = %principal_id, %status, "rejected an inactive principal");
        return Err(ApiError::unauthenticated("this principal is not active"));
    }
    let grants = load_grants(conn, &principal_id)?;
    Ok(Actor {
        id: principal_id,
        org_id,
        kind,
        display_name,
        grants,
        credential: credential.clone(),
    })
}

pub fn session_cookie_value(headers: &HeaderMap) -> Option<String> {
    let cookies = headers.get(COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == SESSION_COOKIE).then(|| value.to_string())
    })
}

fn credential_from(headers: &HeaderMap) -> ApiResult<Option<Credential>> {
    if let Some(value) = headers.get(AUTHORIZATION) {
        let token = value
            .to_str()
            .ok()
            .and_then(|text| text.strip_prefix("Bearer "))
            .ok_or_else(|| ApiError::unauthenticated("Authorization must be a Bearer token"))?;
        return Ok(Some(Credential::Token(hash_secret(token.trim()))));
    }
    Ok(session_cookie_value(headers).map(|value| Credential::Session(hash_secret(&value))))
}

impl FromRequestParts<AppState> for Actor {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let credential = credential_from(&parts.headers)?
            .ok_or_else(|| ApiError::unauthenticated("authentication is required"))?;

        // Cookie はブラウザが自動で送るので、状態を変える要求は発信元も確かめる。
        let unsafe_method =
            !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
        if unsafe_method && matches!(credential, Credential::Session(_)) {
            let origin = parts.headers.get(ORIGIN).and_then(|value| value.to_str().ok());
            if origin.is_some_and(|origin| origin != state.public_origin) {
                tracing::warn!(?origin, "rejected a cross-origin session request");
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "origin_rejected",
                    "request origin does not match this server",
                ));
            }
        }

        state.db.read(move |conn| load_actor(conn, &credential)).await
    }
}
