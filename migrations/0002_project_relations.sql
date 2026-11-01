-- タスク間の依存・関連・親子関係を同一プロジェクト内に限定する。

CREATE TRIGGER task_dependencies_project_insert
BEFORE INSERT ON task_dependencies
WHEN (SELECT project_id FROM tasks WHERE id = NEW.task_id)
  <> (SELECT project_id FROM tasks WHERE id = NEW.depends_on_id)
BEGIN
  SELECT RAISE(ABORT, 'related tasks must belong to the same project');
END;

CREATE TRIGGER task_dependencies_project_update
BEFORE UPDATE ON task_dependencies
WHEN (SELECT project_id FROM tasks WHERE id = NEW.task_id)
  <> (SELECT project_id FROM tasks WHERE id = NEW.depends_on_id)
BEGIN
  SELECT RAISE(ABORT, 'related tasks must belong to the same project');
END;

CREATE TRIGGER task_links_project_insert
BEFORE INSERT ON task_links
WHEN (SELECT project_id FROM tasks WHERE id = NEW.low_id)
  <> (SELECT project_id FROM tasks WHERE id = NEW.high_id)
BEGIN
  SELECT RAISE(ABORT, 'related tasks must belong to the same project');
END;

CREATE TRIGGER task_links_project_update
BEFORE UPDATE ON task_links
WHEN (SELECT project_id FROM tasks WHERE id = NEW.low_id)
  <> (SELECT project_id FROM tasks WHERE id = NEW.high_id)
BEGIN
  SELECT RAISE(ABORT, 'related tasks must belong to the same project');
END;

CREATE TRIGGER tasks_parent_project_insert
BEFORE INSERT ON tasks
WHEN NEW.parent_id IS NOT NULL
  AND NEW.project_id <> (SELECT project_id FROM tasks WHERE id = NEW.parent_id)
BEGIN
  SELECT RAISE(ABORT, 'parent task must belong to the same project');
END;

CREATE TRIGGER tasks_parent_project_update
BEFORE UPDATE ON tasks
WHEN NEW.parent_id IS NOT NULL
  AND NEW.project_id <> (SELECT project_id FROM tasks WHERE id = NEW.parent_id)
BEGIN
  SELECT RAISE(ABORT, 'parent task must belong to the same project');
END;

-- 関係を持つタスクのプロジェクト変更でも制約を維持する。
CREATE TRIGGER tasks_relations_project_update
BEFORE UPDATE OF project_id ON tasks
WHEN EXISTS (
  SELECT 1 FROM tasks child WHERE child.parent_id = OLD.id AND child.project_id <> NEW.project_id
  UNION ALL
  SELECT 1 FROM task_dependencies d JOIN tasks other
    ON other.id = CASE WHEN d.task_id = OLD.id THEN d.depends_on_id ELSE d.task_id END
    WHERE (d.task_id = OLD.id OR d.depends_on_id = OLD.id) AND other.project_id <> NEW.project_id
  UNION ALL
  SELECT 1 FROM task_links l JOIN tasks other
    ON other.id = CASE WHEN l.low_id = OLD.id THEN l.high_id ELSE l.low_id END
    WHERE (l.low_id = OLD.id OR l.high_id = OLD.id) AND other.project_id <> NEW.project_id
)
BEGIN
  SELECT RAISE(ABORT, 'related tasks must belong to the same project');
END;
