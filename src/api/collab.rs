//! コメント・質問・成果物・タスク間の関係。

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::tasks::task_event;
use super::{ApiJson, ApiQuery, AppState, required_text};
use crate::auth::Actor;
use crate::db::{new_id, now};
use crate::error::{ApiError, ApiResult};
use crate::events::emit;
use crate::model::Permission;
use crate::tasks::{TaskRow, refresh_claimable, require_same_project, resolve_task};

const MAX_BODY_CHARS: usize = 20_000;

fn require_contribute(actor: &Actor, task: &TaskRow) -> ApiResult<()> {
    if actor.can_contribute(&task.project_id) {
        Ok(())
    } else {
        Err(ApiError::forbidden("edit"))
    }
}

pub fn insert_comment(
    conn: &Connection,
    task: &TaskRow,
    actor: &Actor,
    kind: &str,
    body: &str,
    addressee_id: Option<&str>,
    parent_id: Option<&str>,
) -> ApiResult<String> {
    let id = new_id();
    conn.execute(
        "INSERT INTO comments (id, task_id, author_id, kind, body, addressee_id, parent_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, task.id, actor.id, kind, body, addressee_id, parent_id, now()],
    )?;
    emit(conn, task_event(task, actor, "collab", "comment.added", json!({
        "key": task.key(),
        "comment_id": id,
        "kind": kind,
        "addressee_id": addressee_id,
    })))?;
    Ok(id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddCommentRequest {
    kind: String,
    body: String,
    addressee_id: Option<String>,
    parent_id: Option<String>,
}

/// 依頼先がそのプロジェクトを見られることを確かめる。
fn require_addressee(conn: &Connection, task: &TaskRow, principal_id: &str) -> ApiResult<()> {
    let allowed: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM principals p WHERE p.id = ?1 AND p.org_id = ?2 AND p.status = 'active'
           AND EXISTS(SELECT 1 FROM grants g WHERE g.principal_id = p.id
                      AND (g.project_id IS NULL OR g.project_id = ?3)))",
        params![principal_id, task.org_id, task.project_id],
        |row| row.get(0),
    )?;
    if allowed {
        Ok(())
    } else {
        Err(ApiError::invalid("the addressee cannot access this project"))
    }
}

pub async fn add_comment(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<AddCommentRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let body = required_text("body", &request.body, MAX_BODY_CHARS)?;
    if !matches!(
        request.kind.as_str(),
        "comment" | "progress" | "decision" | "problem" | "question" | "review_request" | "answer"
            | "review_result"
    ) {
        return Err(ApiError::invalid(format!("comment kind '{}' cannot be posted", request.kind)));
    }
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            require_contribute(&actor, &task)?;

            let is_request = matches!(request.kind.as_str(), "question" | "review_request");
            match (&request.addressee_id, is_request) {
                (None, true) => {
                    return Err(ApiError::invalid("questions and review requests need an addressee"));
                }
                (Some(_), false) => {
                    return Err(ApiError::invalid("only questions and review requests have an addressee"));
                }
                (Some(id), true) => require_addressee(conn, &task, id)?,
                (None, false) => {}
            }

            if request.kind == "answer" {
                let parent = request
                    .parent_id
                    .as_deref()
                    .ok_or_else(|| ApiError::invalid("an answer needs parent_id"))?;
                let changed = conn.execute(
                    "UPDATE comments SET resolved_at = ?1 WHERE id = ?2 AND task_id = ?3
                     AND kind IN ('question', 'review_request') AND resolved_at IS NULL",
                    params![now(), parent, task.id],
                )?;
                if changed == 0 {
                    return Err(ApiError::conflict(
                        "request_not_open",
                        "the referenced request does not exist or is already answered",
                    ));
                }
            } else if request.parent_id.is_some() {
                return Err(ApiError::invalid("only answers have parent_id"));
            }

            let id = insert_comment(
                conn,
                &task,
                &actor,
                &request.kind,
                &body,
                request.addressee_id.as_deref(),
                request.parent_id.as_deref(),
            )?;
            Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
        })
        .await
}

/// 自分宛の未回答の質問・レビュー依頼。
pub async fn inbox(State(state): State<AppState>, actor: Actor) -> ApiResult<Json<Value>> {
    state
        .db
        .read(move |conn| {
            let mut statement = conn.prepare(
                "SELECT c.id, c.kind, c.body, c.created_at, t.id, p.key || '-' || t.number, t.title,
                        t.project_id, a.id, a.display_name, a.kind
                 FROM comments c
                 JOIN tasks t ON t.id = c.task_id
                 JOIN projects p ON p.id = t.project_id
                 JOIN principals a ON a.id = c.author_id
                 WHERE c.addressee_id = ?1 AND c.resolved_at IS NULL
                   AND c.kind IN ('question', 'review_request')
                 ORDER BY c.created_at, c.id",
            )?;
            let rows = statement.query_map([&actor.id], |row| {
                let project_id: String = row.get(7)?;
                let item = json!({
                    "id": row.get::<_, String>(0)?,
                    "kind": row.get::<_, String>(1)?,
                    "body": row.get::<_, String>(2)?,
                    "created_at": row.get::<_, String>(3)?,
                    "task": {
                        "id": row.get::<_, String>(4)?,
                        "key": row.get::<_, String>(5)?,
                        "title": row.get::<_, String>(6)?,
                    },
                    "author": {
                        "id": row.get::<_, String>(8)?,
                        "display_name": row.get::<_, String>(9)?,
                        "kind": row.get::<_, String>(10)?,
                    },
                });
                Ok((project_id, item))
            })?;
            let mut items = Vec::new();
            for row in rows {
                let (project_id, item) = row?;
                if actor.can_view(&project_id) {
                    items.push(item);
                }
            }
            Ok(Json(json!({ "requests": items })))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceInput {
    name: String,
    url: String,
}

fn validate_role(role: &str) -> ApiResult<()> {
    if matches!(role, "attachment" | "artifact") {
        Ok(())
    } else {
        Err(ApiError::invalid("role must be attachment or artifact"))
    }
}

pub fn insert_link_artifact(
    conn: &Connection,
    task: &TaskRow,
    actor: &Actor,
    role: &str,
    input: &EvidenceInput,
) -> ApiResult<String> {
    let name = required_text("name", &input.name, 300)?;
    // UI がそのままリンクにするので、スクリプトを実行できるスキームを受け付けない。
    if !(input.url.starts_with("https://") || input.url.starts_with("http://")) {
        return Err(ApiError::invalid("url must be http or https"));
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO artifacts (id, task_id, role, name, url, created_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, task.id, role, name, input.url, actor.id, now()],
    )?;
    emit(conn, task_event(task, actor, "collab", "artifact.added", json!({
        "key": task.key(),
        "artifact_id": id,
        "role": role,
        "name": name,
    })))?;
    Ok(id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddLinkRequest {
    role: String,
    name: String,
    url: String,
}

pub async fn add_link(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<AddLinkRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    validate_role(&request.role)?;
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            require_contribute(&actor, &task)?;
            let input = EvidenceInput { name: request.name, url: request.url };
            let id = insert_link_artifact(conn, &task, &actor, &request.role, &input)?;
            Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadQuery {
    role: String,
    name: String,
}

/// 本文をそのままファイルとして保存する。形式は Content-Type で受け取る。
pub async fn upload_file(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiQuery(query): ApiQuery<UploadQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Value>)> {
    validate_role(&query.role)?;
    let name = required_text("name", &query.name, 300)?;
    if body.is_empty() {
        return Err(ApiError::invalid("file must not be empty"));
    }
    let media_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            require_contribute(&actor, &task)?;
            let id = new_id();
            conn.execute(
                "INSERT INTO artifacts (id, task_id, role, name, media_type, size, content, created_by, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![id, task.id, query.role, name, media_type, body.len() as i64, body.as_ref(), actor.id, now()],
            )?;
            emit(conn, task_event(&task, &actor, "collab", "artifact.added", json!({
                "key": task.key(),
                "artifact_id": id,
                "role": query.role,
                "name": name,
                "size": body.len(),
            })))?;
            Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
        })
        .await
}

/// RFC 5987 の形式でファイル名を渡す。非 ASCII の名前も安全なヘッダーになる。
fn content_disposition(name: &str) -> String {
    let encoded: String = name
        .bytes()
        .map(|byte| match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect();
    format!("attachment; filename*=UTF-8''{encoded}")
}

/// ブラウザ内で実行されないよう、必ずダウンロードとして返す。
pub async fn download(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let (name, media_type, content) = state
        .db
        .read(move |conn| {
            let row: Option<(String, String, String, Option<String>, Option<Vec<u8>>)> = conn
                .query_row(
                    "SELECT t.org_id, t.project_id, a.name, a.media_type, a.content
                     FROM artifacts a JOIN tasks t ON t.id = a.task_id WHERE a.id = ?1",
                    [&id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                )
                .optional()?;
            match row {
                Some((org_id, project_id, name, media_type, Some(content)))
                    if org_id == actor.org_id && actor.can_view(&project_id) =>
                {
                    Ok((name, media_type, content))
                }
                _ => Err(ApiError::not_found("artifact")),
            }
        })
        .await?;
    let media_type = media_type
        .and_then(|value| HeaderValue::from_str(&value).ok())
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    let disposition = HeaderValue::from_str(&content_disposition(&name))
        .map_err(|e| ApiError::internal(format!("content disposition: {e}")))?;
    Ok((
        [
            (CONTENT_TYPE, media_type),
            (CONTENT_DISPOSITION, disposition),
            (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        ],
        content,
    )
        .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddDependencyRequest {
    depends_on: String,
    reason: String,
}

fn creates_cycle(conn: &Connection, task_id: &str, depends_on: &str) -> ApiResult<bool> {
    let found: bool = conn.query_row(
        "WITH RECURSIVE reach(id) AS (
           SELECT ?1
           UNION SELECT d.depends_on_id FROM task_dependencies d JOIN reach r ON d.task_id = r.id
         )
         SELECT EXISTS(SELECT 1 FROM reach WHERE id = ?2)",
        params![depends_on, task_id],
        |row| row.get(0),
    )?;
    Ok(found)
}

pub async fn add_dependency(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<AddDependencyRequest>,
) -> ApiResult<StatusCode> {
    let reason = required_text("reason", &request.reason, 2_000)?;
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Edit)?;
            let other = resolve_task(conn, &actor, &request.depends_on)?;
            require_same_project(&task.project_id, &other)?;
            if creates_cycle(conn, &task.id, &other.id)? {
                return Err(ApiError::conflict("dependency_cycle", "this dependency would create a cycle"));
            }
            let changed = conn.execute(
                "INSERT OR IGNORE INTO task_dependencies (task_id, depends_on_id, reason, created_by, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![task.id, other.id, reason, actor.id, now()],
            )?;
            if changed == 0 {
                return Err(ApiError::conflict("dependency_exists", "this dependency already exists"));
            }
            emit(conn, task_event(&task, &actor, "task", "dependency.added", json!({
                "key": task.key(),
                "depends_on": other.key(),
                "reason": reason,
            })))?;
            refresh_claimable(conn, &actor.id, std::slice::from_ref(&task.id))?;
            Ok(StatusCode::CREATED)
        })
        .await
}

pub async fn remove_dependency(
    State(state): State<AppState>,
    actor: Actor,
    Path((reference, other)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Edit)?;
            let other = resolve_task(conn, &actor, &other)?;
            let changed = conn.execute(
                "DELETE FROM task_dependencies WHERE task_id = ?1 AND depends_on_id = ?2",
                params![task.id, other.id],
            )?;
            if changed == 0 {
                return Err(ApiError::not_found("dependency"));
            }
            emit(conn, task_event(&task, &actor, "task", "dependency.removed", json!({
                "key": task.key(),
                "depends_on": other.key(),
            })))?;
            refresh_claimable(conn, &actor.id, std::slice::from_ref(&task.id))?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddLinkedTaskRequest {
    related: String,
}

fn ordered<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
    if a < b { (a, b) } else { (b, a) }
}

pub async fn add_task_link(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
    ApiJson(request): ApiJson<AddLinkedTaskRequest>,
) -> ApiResult<StatusCode> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Edit)?;
            let other = resolve_task(conn, &actor, &request.related)?;
            require_same_project(&task.project_id, &other)?;
            if task.id == other.id {
                return Err(ApiError::invalid("a task cannot be linked to itself"));
            }
            let (low, high) = ordered(&task.id, &other.id);
            conn.execute(
                "INSERT OR IGNORE INTO task_links (low_id, high_id, created_by, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![low, high, actor.id, now()],
            )?;
            emit(conn, task_event(&task, &actor, "task", "link.added", json!({
                "key": task.key(),
                "related": other.key(),
            })))?;
            Ok(StatusCode::CREATED)
        })
        .await
}

pub async fn remove_task_link(
    State(state): State<AppState>,
    actor: Actor,
    Path((reference, other)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    state
        .db
        .write(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            actor.require(&task.project_id, Permission::Edit)?;
            let other = resolve_task(conn, &actor, &other)?;
            let (low, high) = ordered(&task.id, &other.id);
            let changed = conn.execute(
                "DELETE FROM task_links WHERE low_id = ?1 AND high_id = ?2",
                params![low, high],
            )?;
            if changed == 0 {
                return Err(ApiError::not_found("link"));
            }
            emit(conn, task_event(&task, &actor, "task", "link.removed", json!({
                "key": task.key(),
                "related": other.key(),
            })))?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::content_disposition;

    #[test]
    fn download_names_keep_non_ascii_and_cannot_break_the_header() {
        assert_eq!(content_disposition("report.txt"), "attachment; filename*=UTF-8''report.txt");
        assert_eq!(content_disposition("報告.txt"), "attachment; filename*=UTF-8''%E5%A0%B1%E5%91%8A.txt");
        assert!(!content_disposition("a\"\r\nX-Evil: 1").contains(['"', '\r', '\n']));
    }
}
