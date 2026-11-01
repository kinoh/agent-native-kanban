//! 履歴と変更イベントの配信。カーソル取得と SSE は同じ読み出しを共有する。

use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};
use futures_core::Stream;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ApiQuery, AppState};
use crate::auth::{Actor, load_actor};
use crate::error::{ApiError, ApiResult};
use crate::tasks::resolve_task;

const MAX_BATCH: usize = 500;
const SCAN_BATCH: i64 = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct EventView {
    pub seq: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub category: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub actor: Option<Value>,
    /// 委譲トークンによる操作のとき、そのトークンの ID
    pub delegated_token_id: Option<String>,
    pub data: Value,
    pub created_at: String,
}

const EVENT_SELECT: &str = "SELECT e.seq, e.type, e.category, e.project_id, e.task_id, e.data, e.created_at,
    a.id, a.display_name, a.kind, e.delegated_token_id
    FROM events e LEFT JOIN principals a ON a.id = e.actor_id";

fn event_view(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventView> {
    let data: String = row.get(5)?;
    let actor = row.get::<_, Option<String>>(7)?.map(|id| -> rusqlite::Result<Value> {
        Ok(json!({
            "id": id,
            "display_name": row.get::<_, String>(8)?,
            "kind": row.get::<_, String>(9)?,
        }))
    });
    Ok(EventView {
        seq: row.get(0)?,
        kind: row.get(1)?,
        category: row.get(2)?,
        project_id: row.get(3)?,
        task_id: row.get(4)?,
        actor: actor.transpose()?,
        delegated_token_id: row.get(10)?,
        data: serde_json::from_str(&data)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        created_at: row.get(6)?,
    })
}

/// プロジェクトに属するイベントは閲覧権限のある主体に、組織全体のものは組織管理者にだけ見せる。
fn visible(actor: &Actor, event: &EventView) -> bool {
    match (&event.project_id, &event.task_id) {
        (Some(project_id), Some(task_id)) => actor.can_view_task(project_id, task_id),
        (Some(project_id), None) => actor.sees_whole_project(project_id),
        (None, _) => actor.is_org_admin(),
    }
}

/// 旧版の横断関係の履歴も、参照先の権限を確認してから配信する。
/// 判定不能な参照を含むイベントは公開しない。保存された履歴自体は変更しない。
fn references_visible(conn: &Connection, actor: &Actor, event: &EventView) -> ApiResult<bool> {
    let references: Vec<&Value> = match event.kind.as_str() {
        "dependency.added" | "dependency.removed" => vec![&event.data["depends_on"]],
        "link.added" | "link.removed" => vec![&event.data["related"]],
        "task.updated" => match event.data["changes"].get("parent_id") {
            Some(change) => vec![&change["from"], &change["to"]]
                .into_iter().filter(|value| !value.is_null()).collect(),
            None => Vec::new(),
        },
        _ => Vec::new(),
    };
    for reference in references {
        let Some(reference) = reference.as_str() else {
            tracing::warn!(seq = event.seq, "withheld event with an invalid task reference");
            return Ok(false);
        };
        match resolve_task(conn, actor, reference) {
            Ok(_) => {}
            Err(error) if error.status == axum::http::StatusCode::NOT_FOUND => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

/// `after` より後のイベントを見える分だけ返す。2つ目の値は次回の開始位置で、
/// 見えないイベントだけが続く区間も読み飛ばして進む。
fn read_events(
    conn: &Connection,
    actor: &Actor,
    after: i64,
    project: Option<&str>,
    limit: usize,
) -> ApiResult<(Vec<EventView>, i64)> {
    let sql = format!("{EVENT_SELECT} WHERE e.org_id = ?1 AND e.seq > ?2 ORDER BY e.seq LIMIT ?3");
    let mut statement = conn.prepare_cached(&sql)?;
    let mut cursor = after;
    let mut events = Vec::new();
    loop {
        let batch = statement
            .query_map(params![actor.org_id, cursor, SCAN_BATCH], event_view)?
            .collect::<Result<Vec<_>, _>>()?;
        let scanned = batch.len();
        for event in batch {
            if events.len() == limit {
                return Ok((events, cursor));
            }
            cursor = event.seq;
            let in_scope = project.is_none_or(|p| event.project_id.as_deref() == Some(p));
            if in_scope && visible(actor, &event) && references_visible(conn, actor, &event)? {
                events.push(event);
            }
        }
        if (scanned as i64) < SCAN_BATCH || events.len() == limit {
            return Ok((events, cursor));
        }
    }
}

fn latest_seq(conn: &Connection, org_id: &str) -> ApiResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) FROM events WHERE org_id = ?1",
        [org_id],
        |row| row.get(0),
    )?)
}

fn parse_after(conn: &Connection, actor: &Actor, value: &str) -> ApiResult<i64> {
    if value == "latest" {
        return latest_seq(conn, &actor.org_id);
    }
    value
        .parse::<i64>()
        .ok()
        .filter(|seq| *seq >= 0)
        .ok_or_else(|| ApiError::invalid("after must be a sequence number or 'latest'"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    after: String,
    project: Option<String>,
    limit: Option<usize>,
}

pub async fn list(
    State(state): State<AppState>,
    actor: Actor,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(MAX_BATCH);
    if limit == 0 || limit > MAX_BATCH {
        return Err(ApiError::invalid(format!("limit must be between 1 and {MAX_BATCH}")));
    }
    state
        .db
        .read(move |conn| {
            let after = parse_after(conn, &actor, &query.after)?;
            let (events, cursor) =
                read_events(conn, &actor, after, query.project.as_deref(), limit)?;
            Ok(Json(json!({ "events": events, "cursor": cursor })))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamQuery {
    after: Option<String>,
    project: Option<String>,
}

/// SSE の配信。再接続時はブラウザが送る Last-Event-ID から続ける。
/// 配信中も毎回 認証情報を確かめるので、失効や権限の変更はその時点で反映される。
pub async fn stream(
    State(state): State<AppState>,
    actor: Actor,
    ApiQuery(query): ApiQuery<StreamQuery>,
    headers: HeaderMap,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let resume = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or(query.after)
        .ok_or_else(|| ApiError::invalid("after or Last-Event-ID is required"))?;
    let start_actor = actor.clone();
    let mut cursor = state.db.read(move |conn| parse_after(conn, &start_actor, &resume)).await?;
    let mut changes = state.db.subscribe();
    let project = query.project;
    let credential = actor.credential.clone();
    tracing::info!(actor = %actor.id, cursor, "event stream opened");

    let events = async_stream::stream! {
        loop {
            let credential = credential.clone();
            let project = project.clone();
            let batch = state.db.read(move |conn| {
                let actor = load_actor(conn, &credential)?;
                read_events(conn, &actor, cursor, project.as_deref(), MAX_BATCH)
            }).await;
            let (events, next) = match batch {
                Ok(batch) => batch,
                Err(error) => {
                    tracing::info!(code = error.code, "event stream closed");
                    break;
                }
            };
            cursor = next;
            let idle = events.is_empty();
            for event in events {
                let payload = serde_json::to_string(&event).expect("event view is serializable");
                yield Ok(Event::default().id(event.seq.to_string()).data(payload));
            }
            if idle {
                // 通知がなくても定期的に起きて、失効した認証情報の接続を閉じる。
                let changed = tokio::time::timeout(Duration::from_secs(30), changes.changed()).await;
                if matches!(changed, Ok(Err(_))) {
                    break;
                }
            }
        }
    };
    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

pub fn task_history(conn: &Connection, actor: &Actor, task_id: &str) -> ApiResult<Vec<EventView>> {
    let sql = format!("{EVENT_SELECT} WHERE e.task_id = ?1 ORDER BY e.seq");
    let mut statement = conn.prepare_cached(&sql)?;
    let events = statement.query_map([task_id], event_view)?.collect::<Result<Vec<_>, _>>()?;
    let mut visible_events = Vec::new();
    for event in events {
        if visible(actor, &event) && references_visible(conn, actor, &event)? {
            visible_events.push(event);
        }
    }
    Ok(visible_events)
}

pub async fn history(
    State(state): State<AppState>,
    actor: Actor,
    Path(reference): Path<String>,
) -> ApiResult<Json<Value>> {
    state
        .db
        .read(move |conn| {
            let task = resolve_task(conn, &actor, &reference)?;
            Ok(Json(json!({ "history": task_history(conn, &actor, &task.id)? })))
        })
        .await
}
