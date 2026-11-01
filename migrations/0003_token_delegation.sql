-- 主体の権限を絞って渡す委譲トークン。親を持つトークンが委譲トークンで、
-- 範囲は発行後に変更しない。実効権限はリクエストごとに親までたどって判定する。

ALTER TABLE api_tokens ADD COLUMN parent_token_id TEXT REFERENCES api_tokens(id);
CREATE INDEX api_tokens_parent ON api_tokens(parent_token_id);

-- admin は委譲できない。
CREATE TABLE token_scopes (
  token_id TEXT NOT NULL REFERENCES api_tokens(id),
  project_id TEXT NOT NULL REFERENCES projects(id),
  permission TEXT NOT NULL CHECK (permission IN
    ('view', 'create', 'edit', 'claim', 'assign', 'review', 'approve')),
  PRIMARY KEY (token_id, project_id, permission)
);

-- 行があれば、そのタスクと子孫だけに限定する。
CREATE TABLE token_tasks (
  token_id TEXT NOT NULL REFERENCES api_tokens(id),
  task_id TEXT NOT NULL REFERENCES tasks(id),
  PRIMARY KEY (token_id, task_id)
);

CREATE TRIGGER delegated_tokens_expire
BEFORE INSERT ON api_tokens
WHEN NEW.parent_token_id IS NOT NULL AND NEW.expires_at IS NULL
BEGIN
  SELECT RAISE(ABORT, 'delegated tokens must expire');
END;

-- 主体は委譲元のままなので、どの委譲トークンの操作かは別に記録する。
ALTER TABLE events ADD COLUMN delegated_token_id TEXT;
