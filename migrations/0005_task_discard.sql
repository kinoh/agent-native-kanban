-- 実施しないと決めたタスクの記録。最終段階に置いたまま、実施済みの完了と区別する。
ALTER TABLE tasks ADD COLUMN discarded_at TEXT;
ALTER TABLE tasks ADD COLUMN discard_reason TEXT;
ALTER TABLE tasks ADD COLUMN discarded_by TEXT REFERENCES principals(id);
