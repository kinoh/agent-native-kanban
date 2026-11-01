//! タスクの読み出しと、API に見せる形への変換。取得可能性の判定結果はここで組み立てる。

use std::collections::HashMap;

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde::Serialize;
use serde_json::json;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult};
use crate::events::{EventInput, emit};
use crate::model::{Availability, BlockReason, OccupantFacts, Permission, Readiness, WorkFacts, availability};

const TASK_SELECT: &str = "
SELECT t.id, t.org_id, t.project_id, p.key AS project_key, t.number, t.title, t.description,
       t.purpose, t.acceptance_criteria, t.priority, t.due_at, t.parent_id, t.work_state,
       t.hold_reason, t.version, t.last_claimable, t.created_by, t.created_at, t.updated_at,
       s.id AS stage_id, s.name AS stage_name, s.position AS stage_position,
       s.requires_work, s.claim_permission, s.is_final,
       c.id AS claim_id, c.activity, c.acquired_at,
       h.id AS holder_id, h.kind AS holder_kind, h.display_name AS holder_name, h.status AS holder_status,
       r.id AS responsible_id, r.kind AS responsible_kind, r.display_name AS responsible_name, r.status AS responsible_status,
       a.id AS assignee_id, a.kind AS assignee_kind, a.display_name AS assignee_name, a.status AS assignee_status
FROM tasks t
JOIN projects p ON p.id = t.project_id
JOIN stages s ON s.id = t.stage_id
LEFT JOIN claims c ON c.task_id = t.id AND c.ended_at IS NULL
LEFT JOIN principals h ON h.id = c.holder_id
LEFT JOIN principals r ON r.id = t.responsible_id
LEFT JOIN principals a ON a.id = t.assignee_id";

#[derive(Debug, Clone, Serialize)]
pub struct PrincipalRef {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StageRef {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub requires_work: bool,
    pub claim_permission: String,
    pub is_final: bool,
}

#[derive(Debug, Clone)]
pub struct Blocker {
    pub id: String,
    pub key: String,
    pub project_id: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: String,
    pub org_id: String,
    pub project_id: String,
    pub project_key: String,
    pub number: i64,
    pub title: String,
    pub description: String,
    pub purpose: String,
    pub acceptance_criteria: String,
    pub priority: i64,
    pub due_at: Option<String>,
    pub parent_id: Option<String>,
    pub work_state: String,
    pub hold_reason: Option<String>,
    pub version: i64,
    pub last_claimable: bool,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub stage: StageRef,
    pub claim_id: Option<String>,
    pub activity: Option<String>,
    pub acquired_at: Option<String>,
    pub occupant: Option<PrincipalRef>,
    pub responsible: Option<PrincipalRef>,
    pub assignee: Option<PrincipalRef>,
    pub blockers: Vec<Blocker>,
}

fn principal_ref(row: &Row<'_>, prefix: &str) -> rusqlite::Result<Option<PrincipalRef>> {
    let Some(id) = row.get::<_, Option<String>>(format!("{prefix}_id").as_str())? else {
        return Ok(None);
    };
    Ok(Some(PrincipalRef {
        id,
        kind: row.get(format!("{prefix}_kind").as_str())?,
        display_name: row.get(format!("{prefix}_name").as_str())?,
        status: row.get(format!("{prefix}_status").as_str())?,
    }))
}

fn task_row(row: &Row<'_>) -> rusqlite::Result<TaskRow> {
    Ok(TaskRow {
        id: row.get("id")?,
        org_id: row.get("org_id")?,
        project_id: row.get("project_id")?,
        project_key: row.get("project_key")?,
        number: row.get("number")?,
        title: row.get("title")?,
        description: row.get("description")?,
        purpose: row.get("purpose")?,
        acceptance_criteria: row.get("acceptance_criteria")?,
        priority: row.get("priority")?,
        due_at: row.get("due_at")?,
        parent_id: row.get("parent_id")?,
        work_state: row.get("work_state")?,
        hold_reason: row.get("hold_reason")?,
        version: row.get("version")?,
        last_claimable: row.get("last_claimable")?,
        created_by: row.get("created_by")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        stage: StageRef {
            id: row.get("stage_id")?,
            name: row.get("stage_name")?,
            position: row.get("stage_position")?,
            requires_work: row.get("requires_work")?,
            claim_permission: row.get("claim_permission")?,
            is_final: row.get("is_final")?,
        },
        claim_id: row.get("claim_id")?,
        activity: row.get("activity")?,
        acquired_at: row.get("acquired_at")?,
        occupant: principal_ref(row, "holder")?,
        responsible: principal_ref(row, "responsible")?,
        assignee: principal_ref(row, "assignee")?,
        blockers: Vec::new(),
    })
}

/// `filter` は `t` のみを参照する SQL 条件。依存の読み出しにも同じ条件を使う。
pub fn load_tasks(
    conn: &Connection,
    filter: &str,
    args: &[SqlValue],
    order: &str,
) -> ApiResult<Vec<TaskRow>> {
    let sql = format!("{TASK_SELECT} WHERE {filter} ORDER BY {order}");
    let mut statement = conn.prepare(&sql)?;
    let mut tasks = statement
        .query_map(params_from_iter(args.iter()), task_row)?
        .collect::<Result<Vec<_>, _>>()?;

    // 依存先の最終段階到達を「完了」とする。readiness は段階ごとにリセットされるので使えない。
    let blockers_sql = format!(
        "SELECT d.task_id, b.id, bp.key || '-' || b.number, b.project_id, d.reason
         FROM task_dependencies d
         JOIN tasks b ON b.id = d.depends_on_id
         JOIN projects bp ON bp.id = b.project_id
         JOIN stages bs ON bs.id = b.stage_id
         WHERE bs.is_final = 0
           AND d.task_id IN (SELECT t.id FROM tasks t WHERE {filter})"
    );
    let mut statement = conn.prepare(&blockers_sql)?;
    let rows = statement.query_map(params_from_iter(args.iter()), |row| {
        Ok((
            row.get::<_, String>(0)?,
            Blocker {
                id: row.get(1)?,
                key: row.get(2)?,
                project_id: row.get(3)?,
                reason: row.get(4)?,
            },
        ))
    })?;
    let mut by_task: HashMap<String, Vec<Blocker>> = HashMap::new();
    for row in rows {
        let (task_id, blocker) = row?;
        by_task.entry(task_id).or_default().push(blocker);
    }
    for task in &mut tasks {
        if let Some(blockers) = by_task.remove(&task.id) {
            task.blockers = blockers;
        }
    }
    Ok(tasks)
}

pub fn load_task(conn: &Connection, task_id: &str) -> ApiResult<TaskRow> {
    load_tasks(conn, "t.id = ?1", &[SqlValue::Text(task_id.to_string())], "t.id")?
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::not_found("task"))
}

/// タスク間の関係はプロジェクト内に閉じる。
pub fn require_same_project(project_id: &str, other: &TaskRow) -> ApiResult<()> {
    if project_id == other.project_id {
        return Ok(());
    }
    Err(ApiError::new(
        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        "relation_project_mismatch",
        "related tasks must belong to the same project",
    ))
}

/// `PROJ-12` 形式のキーまたは ID からタスクを解決する。閲覧できないタスクは存在しない扱い。
pub fn resolve_task(conn: &Connection, actor: &Actor, reference: &str) -> ApiResult<TaskRow> {
    let by_key = reference
        .rsplit_once('-')
        .and_then(|(key, number)| Some((key, number.parse::<i64>().ok()?)))
        .filter(|(key, _)| key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
    let task_id: Option<String> = match by_key {
        Some((key, number)) => conn
            .query_row(
                "SELECT t.id FROM tasks t JOIN projects p ON p.id = t.project_id
                 WHERE t.org_id = ?1 AND p.key = ?2 AND t.number = ?3",
                params![actor.org_id, key, number],
                |row| row.get(0),
            )
            .optional()?,
        None => Some(reference.to_string()),
    };
    let task_id = task_id.ok_or_else(|| ApiError::not_found("task"))?;
    let task = load_task(conn, &task_id)?;
    if task.org_id != actor.org_id || !actor.can_view(&task.project_id) {
        return Err(ApiError::not_found("task"));
    }
    Ok(task)
}

impl TaskRow {
    pub fn key(&self) -> String {
        format!("{}-{}", self.project_key, self.number)
    }

    pub fn facts(&self) -> WorkFacts {
        WorkFacts {
            work_state: self.work_state.clone(),
            hold_reason: self.hold_reason.clone(),
            stage_requires_work: self.stage.requires_work,
            occupant: self.occupant.as_ref().map(|holder| OccupantFacts {
                activity: self.activity.clone().unwrap_or_else(|| "unknown".to_string()),
                status: holder.status.clone(),
            }),
            open_blockers: self.blockers.len(),
        }
    }

    pub fn availability(&self) -> Availability {
        availability(&self.facts())
    }

    pub fn claim_permission(&self) -> ApiResult<Permission> {
        Permission::parse(&self.stage.claim_permission).ok_or_else(|| {
            ApiError::internal(format!("unknown claim permission on stage {}", self.stage.id))
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockerView {
    pub id: String,
    pub key: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkView {
    pub readiness: Readiness,
    pub claimable: bool,
    /// `claimable` かつ閲覧者が取得権限を持つ
    pub can_claim: bool,
    pub claim_permission: String,
    pub has_claim_permission: bool,
    pub reasons: Vec<BlockReason>,
    pub occupant: Option<PrincipalRef>,
    pub activity: Option<String>,
    /// 占有者が作業中と申告している
    pub is_working: bool,
    pub acquired_at: Option<String>,
    /// 作業権。占有者本人にだけ返す
    pub claim_id: Option<String>,
    pub hold_reason: Option<String>,
    pub blockers: Vec<BlockerView>,
    /// 閲覧者に見えない依存タスクの数
    pub hidden_blockers: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    pub id: String,
    pub key: String,
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub purpose: String,
    pub acceptance_criteria: String,
    pub stage: StageRef,
    pub responsible: Option<PrincipalRef>,
    pub assignee: Option<PrincipalRef>,
    pub priority: &'static str,
    pub due_at: Option<String>,
    pub parent_id: Option<String>,
    pub version: i64,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub work: WorkView,
}

pub fn priority_name(value: i64) -> &'static str {
    match value {
        4 => "urgent",
        3 => "high",
        2 => "medium",
        1 => "low",
        _ => "none",
    }
}

pub fn parse_priority(value: &str) -> ApiResult<i64> {
    match value {
        "urgent" => Ok(4),
        "high" => Ok(3),
        "medium" => Ok(2),
        "low" => Ok(1),
        "none" => Ok(0),
        _ => Err(ApiError::invalid("priority must be urgent, high, medium, low or none")),
    }
}

pub fn to_view(task: &TaskRow, actor: &Actor) -> ApiResult<TaskView> {
    let result = task.availability();
    let permission = task.claim_permission()?;
    let has_claim_permission = actor.can(&task.project_id, permission);
    let (visible, hidden): (Vec<_>, Vec<_>) =
        task.blockers.iter().partition(|blocker| actor.can_view(&blocker.project_id));
    let is_holder = task.occupant.as_ref().is_some_and(|holder| holder.id == actor.id);
    Ok(TaskView {
        id: task.id.clone(),
        key: task.key(),
        project_id: task.project_id.clone(),
        title: task.title.clone(),
        description: task.description.clone(),
        purpose: task.purpose.clone(),
        acceptance_criteria: task.acceptance_criteria.clone(),
        stage: task.stage.clone(),
        responsible: task.responsible.clone(),
        assignee: task.assignee.clone(),
        priority: priority_name(task.priority),
        due_at: task.due_at.clone(),
        parent_id: task.parent_id.clone(),
        version: task.version,
        created_by: task.created_by.clone(),
        created_at: task.created_at.clone(),
        updated_at: task.updated_at.clone(),
        work: WorkView {
            readiness: result.readiness,
            claimable: result.claimable,
            can_claim: result.claimable && has_claim_permission,
            claim_permission: task.stage.claim_permission.clone(),
            has_claim_permission,
            reasons: result.reasons,
            occupant: task.occupant.clone(),
            activity: task.activity.clone(),
            is_working: task.activity.as_deref() == Some("working"),
            acquired_at: task.acquired_at.clone(),
            claim_id: if is_holder { task.claim_id.clone() } else { None },
            hold_reason: task.hold_reason.clone(),
            blockers: visible
                .into_iter()
                .map(|b| BlockerView { id: b.id.clone(), key: b.key.clone(), reason: b.reason.clone() })
                .collect(),
            hidden_blockers: hidden.len(),
        },
    })
}

/// 記録済みの取得可能性と現在の判定を比べ、変化があれば変更イベントを出す。
/// 変更した処理は、影響するタスクと依存元をここへ渡す。
pub fn refresh_claimable(conn: &Connection, actor_id: &str, task_ids: &[String]) -> ApiResult<()> {
    for task_id in task_ids {
        let task = load_task(conn, task_id)?;
        let claimable = task.availability().claimable;
        if claimable == task.last_claimable {
            continue;
        }
        conn.execute(
            "UPDATE tasks SET last_claimable = ?1 WHERE id = ?2",
            params![claimable, task.id],
        )?;
        emit(conn, EventInput {
            org_id: &task.org_id,
            project_id: Some(&task.project_id),
            task_id: Some(&task.id),
            actor_id: Some(actor_id),
            category: "work",
            kind: if claimable { "work.available" } else { "work.unavailable" },
            data: json!({ "key": task.key(), "stage": task.stage.name }),
        })?;
    }
    Ok(())
}

/// 対象タスクと、それに依存するタスクの ID を返す。
pub fn with_dependents(conn: &Connection, task_id: &str) -> ApiResult<Vec<String>> {
    let mut statement =
        conn.prepare_cached("SELECT task_id FROM task_dependencies WHERE depends_on_id = ?1")?;
    let mut ids = vec![task_id.to_string()];
    let rows = statement.query_map([task_id], |row| row.get::<_, String>(0))?;
    for row in rows {
        ids.push(row?);
    }
    Ok(ids)
}

/// 同じ組織の、削除されていない主体であることを確かめる。
pub fn require_assignable(conn: &Connection, org_id: &str, principal_id: &str) -> ApiResult<()> {
    let found: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM principals WHERE id = ?1 AND org_id = ?2 AND status <> 'deleted')",
        params![principal_id, org_id],
        |row| row.get(0),
    )?;
    if found {
        Ok(())
    } else {
        Err(ApiError::invalid(format!("principal {principal_id} cannot be assigned")))
    }
}
