use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

/// API の失敗。`code` は機械判定用、`message` は人間向けの説明。
/// 権限不足(403)・競合(409)・未認証(401)などをステータスとコードの両方で区別する。
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Option<Value>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into(), details: None }
    }

    pub fn details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn unauthenticated(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthenticated", message)
    }

    pub fn forbidden(permission: &str) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "permission_denied",
            format!("this operation requires the '{permission}' permission"),
        )
        .details(json!({ "required_permission": permission }))
    }

    /// 存在しない場合と閲覧権限がない場合を区別しない。存在の有無を漏らさないため。
    pub fn not_found(what: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", format!("{what} not found"))
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        Self::internal(format!("database error: {error}"))
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(format!("serialization error: {error}"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            // 内部の詳細は利用者に返さず、ログにだけ残す。
            tracing::error!(code = self.code, message = %self.message, "request failed");
            let body = json!({ "error": { "code": self.code, "message": "internal server error" } });
            return (self.status, Json(body)).into_response();
        }
        tracing::info!(status = self.status.as_u16(), code = self.code, message = %self.message, "request rejected");
        let mut error = json!({ "code": self.code, "message": self.message });
        if let Some(details) = self.details {
            error["details"] = details;
        }
        (self.status, Json(json!({ "error": error }))).into_response()
    }
}
