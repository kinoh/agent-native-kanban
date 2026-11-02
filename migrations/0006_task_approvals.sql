-- 担当者は作業の取得で表し、責任者は明示的な承認者に置き換える。
-- 既存の責任者は承認者へ引き継ぎ、担当者の事前割当は削除する。
ALTER TABLE tasks RENAME COLUMN responsible_id TO approver_id;
ALTER TABLE tasks DROP COLUMN assignee_id;

-- 既存プロジェクトは承認者を任意のまま移行する。再承認は安全側の既定値にする。
ALTER TABLE projects ADD COLUMN approver_required INTEGER NOT NULL DEFAULT 0 CHECK (approver_required IN (0, 1));
ALTER TABLE projects ADD COLUMN reapproval_required INTEGER NOT NULL DEFAULT 1 CHECK (reapproval_required IN (0, 1));

-- 承認の要否は個々の業務段階の遷移に設定する。
ALTER TABLE transition_rules ADD COLUMN require_approval INTEGER NOT NULL DEFAULT 0 CHECK (require_approval IN (0, 1));

-- 同じ段階へ差し戻された後の承認を区別する。業務段階を移るたびに増やす。
ALTER TABLE tasks ADD COLUMN approval_cycle INTEGER NOT NULL DEFAULT 0;

CREATE TABLE task_approvals (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id),
  from_stage_id TEXT NOT NULL REFERENCES stages(id),
  to_stage_id TEXT NOT NULL REFERENCES stages(id),
  approved_by TEXT NOT NULL REFERENCES principals(id),
  approval_cycle INTEGER NOT NULL,
  approved_at TEXT NOT NULL,
  UNIQUE (task_id, from_stage_id, to_stage_id, approved_by, approval_cycle)
);
CREATE INDEX task_approvals_lookup ON task_approvals(task_id, from_stage_id, to_stage_id, approved_by);
