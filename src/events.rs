//! 履歴と変更フィードの書き込み口。変更と同じトランザクションで記録する。

use rusqlite::{Connection, params};
use serde_json::Value;

use crate::db::now;
use crate::error::ApiResult;

pub struct EventInput<'a> {
    pub org_id: &'a str,
    pub project_id: Option<&'a str>,
    pub task_id: Option<&'a str>,
    pub actor_id: Option<&'a str>,
    /// 委譲トークンによる操作のとき、そのトークンの ID
    pub delegated_token_id: Option<&'a str>,
    pub category: &'static str,
    pub kind: &'static str,
    pub data: Value,
}

pub fn emit(conn: &Connection, event: EventInput<'_>) -> ApiResult<i64> {
    conn.prepare_cached(
        "INSERT INTO events (org_id, project_id, task_id, actor_id, delegated_token_id, category, type, data, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?
    .execute(params![
        event.org_id,
        event.project_id,
        event.task_id,
        event.actor_id,
        event.delegated_token_id,
        event.category,
        event.kind,
        event.data.to_string(),
        now(),
    ])?;
    tracing::info!(
        event = event.kind,
        actor = ?event.actor_id,
        delegated_token = ?event.delegated_token_id,
        task = ?event.task_id,
        "recorded event"
    );
    Ok(conn.last_insert_rowid())
}
