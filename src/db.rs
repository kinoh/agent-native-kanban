use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, TransactionBehavior};
use time::OffsetDateTime;
use time::macros::format_description;
use tokio::sync::watch;

use crate::error::{ApiError, ApiResult};

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_project_relations.sql"),
    include_str!("../migrations/0003_token_delegation.sql"),
];

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
        if version == 2 {
            validate_project_relations(&tx)?;
        }
        tx.execute_batch(sql).map_err(|e| format!("migration {version}: {e}"))?;
        tx.pragma_update(None, "user_version", version).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        tracing::info!(version, "applied database migration");
    }
    Ok(())
}

/// 既存の関係を勝手に削除・無効化しない。移行前に明示的な解消を求める。
fn validate_project_relations(conn: &Connection) -> Result<(), String> {
    let mut statement = conn.prepare(
        "SELECT 'dependency', a.id, b.id FROM task_dependencies d
           JOIN tasks a ON a.id = d.task_id JOIN tasks b ON b.id = d.depends_on_id
           WHERE a.project_id <> b.project_id
         UNION ALL
         SELECT 'link', a.id, b.id FROM task_links l
           JOIN tasks a ON a.id = l.low_id JOIN tasks b ON b.id = l.high_id
           WHERE a.project_id <> b.project_id
         UNION ALL
         SELECT 'parent', a.id, b.id FROM tasks a JOIN tasks b ON b.id = a.parent_id
           WHERE a.project_id <> b.project_id"
    ).map_err(|e| e.to_string())?;
    let violations = statement.query_map([], |row| {
        Ok(format!("{}: {} -> {}", row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    if violations.is_empty() {
        return Ok(());
    }
    let message = format!(
        "migration 2 blocked: cross-project relations must be removed before upgrading; no relations were changed: {}",
        violations.join(", ")
    );
    tracing::error!(%message, "database migration rejected");
    Err(message)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA user_version = 1;
            INSERT INTO organizations VALUES ('org', 'Org', 'now');
            INSERT INTO projects (id, org_id, key, name, created_by, created_at)
              VALUES ('p1','org','ONE','One','actor','now'), ('p2','org','TWO','Two','actor','now');
            INSERT INTO stages VALUES ('s1','p1','Work',0,1,'claim',0), ('s2','p2','Work',0,1,'claim',0);
            INSERT INTO tasks (id,org_id,project_id,number,title,stage_id,priority,work_state,created_by,created_at,updated_at)
              VALUES ('a','org','p1',1,'A','s1',0,'open','actor','now','now'),
                     ('b','org','p1',2,'B','s1',0,'open','actor','now','now'),
                     ('c','org','p2',1,'C','s2',0,'open','actor','now','now');
        ").unwrap();
        conn
    }

    #[test]
    fn upgrading_refuses_each_legacy_cross_project_relation_without_changing_data() {
        for (kind, insert, remove) in [
            ("dependency", "INSERT INTO task_dependencies VALUES ('a','c','reason','actor','now')", "DELETE FROM task_dependencies"),
            ("link", "INSERT INTO task_links VALUES ('a','c','actor','now')", "DELETE FROM task_links"),
            ("parent", "UPDATE tasks SET parent_id = 'c' WHERE id = 'a'", "UPDATE tasks SET parent_id = NULL"),
        ] {
            let mut conn = legacy_database();
            conn.execute_batch(insert).unwrap();
            let before = conn.total_changes();
            let error = migrate(&mut conn).unwrap_err();
            assert!(error.contains(&format!("{kind}: a -> c")), "{error}");
            assert_eq!(conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), 1);
            assert_eq!(conn.total_changes(), before);
            conn.execute_batch(remove).unwrap();
            migrate(&mut conn).unwrap();
            assert_eq!(
                conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(),
                MIGRATIONS.len() as i64
            );
        }
    }

    #[test]
    fn database_constraints_preserve_project_boundaries_on_inserts_updates_and_moves() {
        let mut conn = legacy_database();
        conn.execute_batch("INSERT INTO task_dependencies VALUES ('a','b','reason','actor','now');
            INSERT INTO task_links VALUES ('a','b','actor','now');
            UPDATE tasks SET parent_id = 'b' WHERE id = 'a';").unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(conn.query_row("SELECT parent_id FROM tasks WHERE id='a'", [], |r|r.get::<_,String>(0)).unwrap(), "b");
        for sql in [
            "INSERT INTO task_dependencies VALUES ('a','c','reason','actor','now')",
            "UPDATE task_dependencies SET depends_on_id='c'",
            "INSERT INTO task_links VALUES ('a','c','actor','now')",
            "UPDATE task_links SET high_id='c'",
            "UPDATE tasks SET parent_id='c' WHERE id='a'",
            "UPDATE tasks SET project_id='p2', number=3 WHERE id='b'",
            "INSERT INTO tasks (id,org_id,project_id,number,title,stage_id,priority,work_state,parent_id,created_by,created_at,updated_at)
             VALUES ('d','org','p1',4,'D','s1',0,'open','c','actor','now','now')",
        ] {
            let error = conn.execute_batch(sql).unwrap_err();
            assert!(error.to_string().contains("same project"), "{sql}: {error}");
        }
        // 正常な関係は解消でき、制約のないタスクは別プロジェクトへ移せる。
        conn.execute_batch("DELETE FROM task_dependencies; DELETE FROM task_links;
            UPDATE tasks SET parent_id=NULL WHERE id='a';
            UPDATE tasks SET project_id='p2', stage_id='s2', number=3 WHERE id='b';").unwrap();
    }
}
