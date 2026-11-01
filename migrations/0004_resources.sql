-- 組織またはプロジェクトが共有する、明示的に確保・解放する資源。
CREATE TABLE resources (
  id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES organizations(id),
  project_id TEXT REFERENCES projects(id),
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  capacity INTEGER NOT NULL CHECK (capacity > 0),
  created_by TEXT NOT NULL REFERENCES principals(id),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX resources_scope_name ON resources(org_id, COALESCE(project_id, ''), name);
CREATE INDEX resources_org_project ON resources(org_id, project_id);

-- 終了した確保も残し、現在の状態は ended_at IS NULL で区別する。リソース削除時には
-- 確保履歴を消すが、監査イベントは残る。
CREATE TABLE resource_holds (
  id TEXT PRIMARY KEY,
  resource_id TEXT NOT NULL REFERENCES resources(id) ON DELETE CASCADE,
  holder_id TEXT NOT NULL REFERENCES principals(id),
  reason TEXT NOT NULL,
  acquired_at TEXT NOT NULL,
  ended_at TEXT,
  end_reason TEXT CHECK (end_reason IN ('released', 'revoked')),
  ended_by TEXT REFERENCES principals(id),
  revoke_reason TEXT
);
CREATE INDEX resource_holds_active ON resource_holds(resource_id) WHERE ended_at IS NULL;
CREATE INDEX resource_holds_holder ON resource_holds(holder_id) WHERE ended_at IS NULL;
