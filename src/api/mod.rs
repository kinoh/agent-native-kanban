pub mod admin;
pub mod delegation;
pub mod feed;
pub mod identity;
pub mod projects;
pub mod tasks;
pub mod work;
pub mod collab;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::auth::LoginLimiter;
use crate::db::Db;
use crate::error::{ApiError, ApiResult};

pub const MAX_FILE_BYTES: usize = 25 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// サインアップ URL の組み立てに使う。末尾のスラッシュは持たない。
    pub public_url: String,
    /// `public_url` の origin。Cookie 認証の発信元検証に使う。
    pub public_origin: String,
    pub login_limiter: Arc<Mutex<LoginLimiter>>,
}

impl AppState {
    pub fn new(db: Db, public_url: &str) -> Result<Self, String> {
        let public_url = public_url.trim_end_matches('/').to_string();
        let (scheme, rest) = public_url
            .split_once("://")
            .filter(|(scheme, _)| matches!(*scheme, "http" | "https"))
            .ok_or_else(|| format!("public url must start with http:// or https://: {public_url}"))?;
        let host = rest.split('/').next().filter(|host| !host.is_empty());
        let host = host.ok_or_else(|| format!("public url has no host: {public_url}"))?;
        let public_origin = format!("{scheme}://{host}");
        Ok(Self { db, public_url, public_origin, login_limiter: Arc::default() })
    }

    pub fn secure_cookies(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}

/// JSON 本文の拒否も API 共通のエラー形式で返す。
pub struct ApiJson<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection @ JsonRejection::MissingJsonContentType(_)) => {
                Err(ApiError::invalid(rejection.body_text()))
            }
            Err(rejection) => Err(ApiError::invalid(rejection.body_text())),
        }
    }
}

pub struct ApiQuery<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequestParts<S> for ApiQuery<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map(|query| Self(query.0))
            .map_err(|rejection: QueryRejection| ApiError::invalid(rejection.body_text()))
    }
}

/// PATCH で「省略」と「null で消す」を区別する。
pub fn explicit<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

pub fn required_text(field: &str, value: &str, max_chars: usize) -> ApiResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiError::invalid(format!("{field} must not be empty")));
    }
    if trimmed.chars().count() > max_chars {
        return Err(ApiError::invalid(format!("{field} must be at most {max_chars} characters")));
    }
    Ok(trimmed.to_string())
}

async fn api_not_found() -> ApiError {
    ApiError::not_found("endpoint")
}

fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(identity::login))
        .route("/auth/logout", post(identity::logout))
        .route("/signup/{token}", get(identity::signup_info).post(identity::signup))
        .route("/me", get(identity::me))
        .route("/me/password", put(identity::change_password))
        .route("/me/inbox", get(collab::inbox))
        .route("/me/delegations", get(delegation::list).post(delegation::create))
        .route("/principals", get(admin::list_principals).post(admin::create_principal))
        .route(
            "/principals/{id}",
            get(admin::get_principal)
                .patch(admin::update_principal)
                .delete(admin::delete_principal),
        )
        .route("/principals/{id}/password", put(admin::set_password))
        .route("/principals/{id}/grants", get(admin::list_grants).post(admin::add_grant))
        .route("/principals/{id}/tokens", get(admin::list_tokens).post(admin::create_token))
        .route("/grants/{id}", delete(admin::delete_grant))
        .route("/tokens/{id}", delete(admin::revoke_token))
        .route("/signup-links", get(admin::list_signup_links).post(admin::create_signup_link))
        .route("/signup-links/{id}", delete(admin::revoke_signup_link))
        .route("/projects", get(projects::list).post(projects::create))
        .route("/projects/{id}", get(projects::get).patch(projects::update))
        .route("/projects/{id}/stages", post(projects::add_stage))
        .route("/projects/{id}/transition-rules", put(projects::put_rules))
        .route("/stages/{id}", patch(projects::update_stage).delete(projects::delete_stage))
        .route("/tasks", get(tasks::list).post(tasks::create))
        .route("/tasks/{task}", get(tasks::get).patch(tasks::update))
        .route("/tasks/{task}/context", get(tasks::context))
        .route("/tasks/{task}/history", get(feed::history))
        .route("/tasks/{task}/stage", post(tasks::move_stage))
        .route("/tasks/{task}/work/claim", post(work::claim))
        .route("/tasks/{task}/work/start", post(work::start))
        .route("/tasks/{task}/work/pause", post(work::pause))
        .route("/tasks/{task}/work/resume", post(work::resume))
        .route("/tasks/{task}/work/release", post(work::release))
        .route("/tasks/{task}/work/transfer", post(work::transfer))
        .route("/tasks/{task}/work/complete", post(work::complete))
        .route("/tasks/{task}/work/reopen", post(work::reopen))
        .route("/tasks/{task}/work/hold", post(work::hold))
        .route("/tasks/{task}/work/unhold", post(work::unhold))
        .route("/tasks/{task}/work/revoke", post(work::revoke))
        .route("/tasks/{task}/work/reassign", post(work::reassign))
        .route("/tasks/{task}/comments", post(collab::add_comment))
        .route("/tasks/{task}/artifacts", post(collab::add_link))
        .route(
            "/tasks/{task}/files",
            post(collab::upload_file).layer(DefaultBodyLimit::max(MAX_FILE_BYTES)),
        )
        .route("/artifacts/{id}/content", get(collab::download))
        .route("/tasks/{task}/dependencies", post(collab::add_dependency))
        .route("/tasks/{task}/dependencies/{other}", delete(collab::remove_dependency))
        .route("/tasks/{task}/links", post(collab::add_task_link))
        .route("/tasks/{task}/links/{other}", delete(collab::remove_task_link))
        .route("/events", get(feed::list))
        .route("/events/stream", get(feed::stream))
        .fallback(api_not_found)
}

pub fn router(state: AppState, web_dist: Option<PathBuf>) -> Router {
    let mut app = Router::new().nest("/api/v1", api_routes());
    if let Some(dist) = web_dist {
        // クライアント側ルーティングのパスは index.html に解決する。
        let index = ServeFile::new(dist.join("index.html"));
        app = app.fallback_service(ServeDir::new(dist).fallback(index));
    }
    app.layer(TraceLayer::new_for_http()).with_state(state)
}
