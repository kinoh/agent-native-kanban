CREATE TABLE organizations (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL
);

-- 人間とエージェントは同じ主体テーブルに載せる。created_by は記録であり所有ではないので外部キーにしない。
CREATE TABLE principals (
  id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES organizations(id),
  kind TEXT NOT NULL CHECK (kind IN ('human', 'agent')),
  display_name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL CHECK (status IN ('active', 'disabled', 'deleted')),
  created_by TEXT,
  created_at TEXT NOT NULL,
  deleted_at TEXT
);
CREATE INDEX principals_org ON principals(org_id);

CREATE TABLE human_credentials (
  principal_id TEXT PRIMARY KEY REFERENCES principals(id),
  email TEXT NOT NULL UNIQUE COLLATE NOCASE,
  password_hash TEXT NOT NULL
);

CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  principal_id TEXT NOT NULL REFERENCES principals(id),
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);
CREATE INDEX sessions_principal ON sessions(principal_id);

CREATE TABLE api_tokens (
  id TEXT PRIMARY KEY,
  principal_id TEXT NOT NULL REFERENCES principals(id),
  name TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT,
  revoked_at TEXT
);
CREATE INDEX api_tokens_principal ON api_tokens(principal_id);

CREATE TABLE signup_links (
  id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES organizations(id),
  token_hash TEXT NOT NULL UNIQUE,
  note TEXT NOT NULL,
  grants_json TEXT NOT NULL,
  max_uses INTEGER NOT NULL CHECK (max_uses > 0),
  used_count INTEGER NOT NULL DEFAULT 0,
  expires_at TEXT NOT NULL,
  created_by TEXT,
  created_at TEXT NOT NULL,
  revoked_at TEXT
);

CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES organizations(id),
  key TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  next_task_number INTEGER NOT NULL DEFAULT 1,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (org_id, key)
);

CREATE TABLE grants (
  id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES organizations(id),
  principal_id TEXT NOT NULL REFERENCES principals(id),
  project_id TEXT REFERENCES projects(id),
  permission TEXT NOT NULL CHECK (permission IN
    ('view', 'create', 'edit', 'claim', 'assign', 'review', 'approve', 'admin')),
  granted_by TEXT,
  created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX grants_unique ON grants(principal_id, COALESCE(project_id, ''), permission);

CREATE TABLE stages (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  name TEXT NOT NULL,
  position INTEGER NOT NULL,
  requires_work INTEGER NOT NULL CHECK (requires_work IN (0, 1)),
  claim_permission TEXT NOT NULL CHECK (claim_permission IN ('claim', 'review', 'approve')),
  is_final INTEGER NOT NULL CHECK (is_final IN (0, 1)),
  UNIQUE (project_id, name)
);

CREATE TABLE transition_rules (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  from_stage_id TEXT NOT NULL REFERENCES stages(id),
  to_stage_id TEXT NOT NULL REFERENCES stages(id),
  required_permission TEXT CHECK (required_permission IN ('edit', 'review', 'approve', 'admin')),
  require_work_done INTEGER NOT NULL CHECK (require_work_done IN (0, 1)),
  UNIQUE (from_stage_id, to_stage_id)
);

-- work_state は「現段階の作業」の状態。占有者と活動は claims にあり、ここには持たない。
CREATE TABLE tasks (
  id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES organizations(id),
  project_id TEXT NOT NULL REFERENCES projects(id),
  number INTEGER NOT NULL,
  title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  purpose TEXT NOT NULL DEFAULT '',
  acceptance_criteria TEXT NOT NULL DEFAULT '',
  stage_id TEXT NOT NULL REFERENCES stages(id),
  responsible_id TEXT REFERENCES principals(id),
  assignee_id TEXT REFERENCES principals(id),
  priority INTEGER NOT NULL CHECK (priority BETWEEN 0 AND 4),
  due_at TEXT,
  parent_id TEXT REFERENCES tasks(id),
  work_state TEXT NOT NULL CHECK (work_state IN ('open', 'on_hold', 'done')),
  hold_reason TEXT,
  version INTEGER NOT NULL DEFAULT 1,
  last_claimable INTEGER NOT NULL DEFAULT 0,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (project_id, number)
);
CREATE INDEX tasks_org_project ON tasks(org_id, project_id);
CREATE INDEX tasks_parent ON tasks(parent_id);

-- 同時に有効な占有がタスクに2つ存在しないことを DB の制約で保証する。
CREATE TABLE claims (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id),
  holder_id TEXT NOT NULL REFERENCES principals(id),
  activity TEXT NOT NULL CHECK (activity IN ('claimed', 'working', 'paused')),
  acquired_at TEXT NOT NULL,
  ended_at TEXT,
  end_reason TEXT CHECK (end_reason IN ('released', 'completed', 'transferred', 'revoked', 'reassigned')),
  ended_by TEXT
);
CREATE UNIQUE INDEX claims_one_active ON claims(task_id) WHERE ended_at IS NULL;
CREATE INDEX claims_task ON claims(task_id);

CREATE TABLE task_dependencies (
  task_id TEXT NOT NULL REFERENCES tasks(id),
  depends_on_id TEXT NOT NULL REFERENCES tasks(id),
  reason TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (task_id, depends_on_id),
  CHECK (task_id <> depends_on_id)
);
CREATE INDEX task_dependencies_rev ON task_dependencies(depends_on_id);

CREATE TABLE task_links (
  low_id TEXT NOT NULL REFERENCES tasks(id),
  high_id TEXT NOT NULL REFERENCES tasks(id),
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (low_id, high_id),
  CHECK (low_id < high_id)
);

CREATE TABLE comments (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id),
  author_id TEXT NOT NULL REFERENCES principals(id),
  kind TEXT NOT NULL CHECK (kind IN ('comment', 'progress', 'decision', 'problem', 'question',
    'review_request', 'answer', 'review_result', 'handoff', 'completion')),
  body TEXT NOT NULL,
  addressee_id TEXT REFERENCES principals(id),
  parent_id TEXT REFERENCES comments(id),
  resolved_at TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX comments_task ON comments(task_id);
CREATE INDEX comments_addressee ON comments(addressee_id) WHERE resolved_at IS NULL;

CREATE TABLE artifacts (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id),
  role TEXT NOT NULL CHECK (role IN ('attachment', 'artifact')),
  name TEXT NOT NULL,
  url TEXT,
  media_type TEXT,
  size INTEGER,
  content BLOB,
  created_by TEXT NOT NULL REFERENCES principals(id),
  created_at TEXT NOT NULL,
  CHECK ((url IS NULL) <> (content IS NULL))
);
CREATE INDEX artifacts_task ON artifacts(task_id);

-- 履歴と変更フィードの共通の実体。業務段階の変更は category = 'stage'、作業は 'work' で区別する。
CREATE TABLE events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  org_id TEXT NOT NULL,
  project_id TEXT,
  task_id TEXT,
  actor_id TEXT,
  category TEXT NOT NULL CHECK (category IN ('stage', 'work', 'task', 'collab', 'admin')),
  type TEXT NOT NULL,
  data TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX events_org_seq ON events(org_id, seq);
CREATE INDEX events_task ON events(task_id, seq);
