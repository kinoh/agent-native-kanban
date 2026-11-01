# KAN-1–5 implementation

## User requirements and authorization
User requested: 「優先度をつけて、あとカード内容も整理して作業可能にして」 then 「実装subagent x2で作業させて」.
Implement the five prepared cards with exactly two independent implementation workers through Herdr. Acceptance criteria are preserved in cards.json and the live board at http://127.0.0.1:18787/projects/KAN/board. Priority high: KAN-5 and KAN-4; medium: KAN-2/3; low: KAN-1.
Director owns user communication, assignment boundaries, live Kanban mutations, review and final acceptance. Workers implement and verify; they may not spawn or direct other agents.

## Observed facts
Repository /home/kino/src/agent-native-kanban, branch main, initially clean. Rust/axum/SQLite backend; React/Vite frontend. Admin principal rename and self password change APIs already exist. Organization rename and self display-name APIs do not exist. All five cards are in 実装 and claimable at assignment creation.

## Director decisions / integration contract
Split by backend and frontend ownership to avoid concurrent edits of shared files.
- PATCH /api/v1/organization with {name}: org admin only; own organization; trimmed 1–100 characters; response {id,name}; event organization.updated with old/new name; no IDs or memberships changed.
- PATCH /api/v1/me with {display_name}: authenticated human self only; reject delegated credentials; trimmed 1–100 characters; deny unknown fields; response uses GET /me shape. Preserve existing principal admin restrictions. Emit principal.updated with actor/subject and changed name.
- Existing PATCH /principals/{id} and PUT /me/password are reused unchanged by UI.
- GET /api/v1/guide: unauthenticated UTF-8 text/markdown, no organization data or secrets; worker A owns guide and README accuracy, including new APIs.
These are implementation decisions, not verbatim user requirements. Report incompatibilities before changing contract.

## Applicable user constraints
Respond in Japanese, casual; user 主さま, self うち. Read actual code and existing documents first. Preserve agreed scope; no unapproved workaround, implicit fallbacks, unnecessary dependencies or unrelated features. Design explicit errors and useful audit/debug logs from the start. Tests must verify user behavior. No secrets in logs, files, prompts or commits; do not decrypt encrypted data. No externally metered requests, publication, push, PRs or external messages. Do not modify .env. Use ~/.local/state/agent-native-kanban/ for temporary artifacts, never /tmp. Do not restart or replace the user's running server or modify its business data. Use independent test DBs and ports. Ignore Codex hook errors.

All work must be committed before handoff or stopping. Never amend/rebase/force-push. Stage exact owned paths only. Since workers share main and working tree, serialize ALL Git index/commit mutations using flock ~/.local/state/agent-native-kanban/git.lock. No stash, reset, checkout or cleaning another worker's changes. Commit messages describe purpose. Director and each worker own separate task-state files.

Herdr: use installed herdr skill. Long operations (>30 sec expected) use herdr-watch from the beginning. Other commands start with 30-second wait; if still running, monitor that SAME process using herdr-watch; no polling/sleep/write_stdin loops. Delegate replies use herdr-watch reply; no built-in subagents. Read /home/kino/.codex/skills/agent-coordination/SKILL.md and relevant worker/task-state references. UI editing requires /home/kino/.agents/skills/stop-describing-ui/SKILL.md.

## Completion
All card criteria implemented, relevant API/browser checks passed, integrated changes reviewed by director, all intended changes committed, live cards updated with evidence. Worker success alone does not establish whole-task completion. Live deployment/restart is not included without user agreement.
