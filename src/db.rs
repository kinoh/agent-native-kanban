use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, TransactionBehavior};
use time::OffsetDateTime;
use time::macros::format_description;
use tokio::sync::watch;

use crate::error::{ApiError, ApiResult};

const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_init.sql")];

/// 単一コネクションを直列に使う。書き込みは `BEGIN IMMEDIATE` なので、
/// 判定と更新の間に他の書き込みが割り込むことはない。
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    latest_seq: Arc<watch::Sender<i64>>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        Self::init(Connection::open_in_memory().map_err(|e| e.to_string())?)
    }

    fn init(mut conn: Connection) -> Result<Self, String> {
        conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON").map_err(|e| e.to_string())?;
        conn.pragma_update(None, "busy_timeout", 5000).map_err(|e| e.to_string())?;
        migrate(&mut conn)?;
        let seq = current_seq(&conn).map_err(|e| e.to_string())?;
        let (sender, _) = watch::channel(seq);
        Ok(Self { conn: Arc::new(Mutex::new(conn)), latest_seq: Arc::new(sender) })
    }

    pub fn subscribe(&self) -> watch::Receiver<i64> {
        self.latest_seq.subscribe()
    }

    pub async fn read<T, F>(&self, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> ApiResult<T> + Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|_| ApiError::internal("database lock poisoned"))?;
            f(&conn)
        })
        .await
        .map_err(|e| ApiError::internal(format!("database task failed: {e}")))?
    }

    /// `f` が Ok を返したときだけコミットする。Err ならその操作の変更は全て残らない。
    pub async fn write<T, F>(&self, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> ApiResult<T> + Send + 'static,
    {
        let db = self.clone();
        tokio::task::spawn_blocking(move || db.write_blocking(f))
            .await
            .map_err(|e| ApiError::internal(format!("database task failed: {e}")))?
    }

    pub fn write_blocking<T, F>(&self, f: F) -> ApiResult<T>
    where
        F: FnOnce(&Connection) -> ApiResult<T>,
    {
        let mut conn = self.conn.lock().map_err(|_| ApiError::internal("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = f(&tx)?;
        tx.commit()?;
        let seq = current_seq(&conn)?;
        self.latest_seq.send_replace(seq);
        Ok(value)
    }
}

fn current_seq(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |row| row.get(0))
}

fn migrate(conn: &mut Connection) -> Result<(), String> {
    let applied: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let applied = applied as usize;
    if applied > MIGRATIONS.len() {
        return Err(format!(
            "database schema version {applied} is newer than this binary ({})",
            MIGRATIONS.len()
        ));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
        let version = index as i64 + 1;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch(sql).map_err(|e| format!("migration {version}: {e}"))?;
        tx.pragma_update(None, "user_version", version).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        tracing::info!(version, "applied database migration");
    }
    Ok(())
}

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// 固定幅の UTC 形式で保存する。文字列比較がそのまま時刻比較になる。
pub fn format_time(value: OffsetDateTime) -> String {
    let format = format_description!(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
    );
    value
        .to_offset(time::UtcOffset::UTC)
        .format(format)
        .expect("timestamp formatting cannot fail for in-range dates")
}

pub fn now() -> String {
    format_time(OffsetDateTime::now_utc())
}

pub fn after_seconds(seconds: i64) -> String {
    format_time(OffsetDateTime::now_utc() + time::Duration::seconds(seconds))
}

/// API から受け取った RFC 3339 の時刻を保存形式に正規化する。
pub fn parse_time(field: &str, value: &str) -> ApiResult<String> {
    let parsed = OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| ApiError::invalid(format!("{field} must be an RFC 3339 timestamp")))?;
    Ok(format_time(parsed))
}
