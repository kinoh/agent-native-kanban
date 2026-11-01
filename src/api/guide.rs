//! 認証前に API の利用手順を示す静的な入口。

use axum::http::header::CONTENT_TYPE;

const GUIDE: &str = r#"# Agent-Native Kanban API guide

Base URL: `https://your-kanban.example/api/v1` (replace the host with this server).

## Start here

`GET /guide` is public and returns this UTF-8 Markdown document. All endpoints below except signup and login require authentication. Do not put a token in a URL, a task comment, or a log.

An organization administrator creates an agent at `POST /principals`, then creates its token at `POST /principals/{principal_id}/tokens`. The token is returned exactly once. Send it on every API request:

```sh
API=https://your-kanban.example/api/v1
TOKEN=ank_replace_with_the_value_returned_once
H="Authorization: Bearer $TOKEN"
J='Content-Type: application/json'
curl -H "$H" "$API/me"
```

Humans register through an administrator-issued `GET|POST /signup/{token}` URL, then use `POST /auth/login` and its secure cookie session. `GET /me` identifies the current principal, organization, effective grants, and whether the credential is delegated.

## Find and do work

Project endpoints use project IDs. First list visible projects, then retain the returned `id`:

```sh
curl -H "$H" "$API/projects"
curl -H "$H" "$API/tasks?project=<project-id>&can_claim=true&sort=priority&limit=50&cursor=0"
curl -H "$H" "$API/tasks/PROJ-12/context"
```

Tasks accept either their ID or a stable `PROJ-12` key. `GET /tasks` supports filters such as `project`, `stage`, `assignee`, `readiness`, `can_claim`, `q`, `due_before`, sorting, `limit`, and `cursor`; use its `next_cursor` with `cursor` for pagination. Read `GET /tasks/{task}/context` before starting work: it collects the task, dependencies, comments, artifacts, links, and history.

Claiming returns the `work.claim_id` that proves the holder may operate on that task in its current stage. Pass it to every lifecycle action:

```sh
curl -H "$H" -H "$J" -X POST "$API/tasks/PROJ-12/work/claim"
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>"}' "$API/tasks/PROJ-12/work/start"
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>"}' "$API/tasks/PROJ-12/work/pause"
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>"}' "$API/tasks/PROJ-12/work/resume"
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>"}' "$API/tasks/PROJ-12/work/release"
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>","summary":"what changed","evidence":[{"name":"report","url":"https://example.invalid/report"}]}' "$API/tasks/PROJ-12/work/complete"
```

`work/complete` records that the current work is done; it does **not** move the task to a final business stage. Use `POST /tasks/{task}/stage` with `to_stage_id` and the task's current `version` when the workflow permits that move. A stage that requires work opens a fresh claim cycle. Use `work/reopen`, `work/hold`, and `work/unhold` with edit authority; administrators use `work/revoke` and `work/reassign`.

## Record context and collaborate

Use `POST /tasks/{task}/comments` for comments, progress, and questions; use `POST /tasks/{task}/artifacts` for links and `POST /tasks/{task}/files` for uploads. Add and remove task dependencies with `/dependencies` and related-task links with `/links`. A dependency blocks claimability until its task reaches a final stage. `GET /tasks/{task}/history` and `GET /events?after=<seq|latest>` expose audit history; retain each event response's `cursor` for the next request. `GET /events/stream?after=<seq|latest>` is the SSE equivalent.

## API map and authority

| Area | Endpoints | Authority |
| --- | --- | --- |
| Identity | `GET|PATCH /me`, `PUT /me/password`, `GET /me/inbox` | authenticated human for self changes |
| Organization | `PATCH /organization` | organization administrator |
| Principals and credentials | `/principals`, `/grants`, `/tokens`, `/signup-links` | organization or permitted project administrator |
| Projects and workflow | `/projects`, `/stages`, `/transition-rules` | create/edit/admin grants as applicable |
| Tasks and work | `/tasks`, `/tasks/{task}/work/*` | visible project grant and the operation's required permission |
| Delegation | `GET|POST /me/delegations`, `DELETE /tokens/{id}` | a non-delegated bearer token, narrowed from its own scope |
| Events | `/events`, `/events/stream` | only events visible to the current credential |

Permissions are `view`, `create`, `edit`, `claim`, `assign`, `review`, `approve`, and `admin`; `admin` includes all permissions in its organization or project scope. Delegations never add authority: their scopes and optional task list can only narrow the parent token, they expire with or before it, and revoking any ancestor revokes its descendants. Delegated tokens cannot manage credentials, change a self profile, or become organization administrators.

## Errors and recovery

Errors are JSON: `{ "error": { "code": "…", "message": "…", "details": { … } } }`.

- `401 unauthenticated`: authenticate again or replace an expired/revoked token.
- `403 permission_denied`: obtain the required grant; `details.required_permission` names it. `403 delegated_token_restricted` means use a non-delegated credential for identity or credential management.
- `404 not_found`: the resource does not exist or is outside your visible scope.
- `409`: resolve concurrent state (`version_conflict`, `stale_claim`, `task_occupied`, or `not_claimable`) by rereading the task and using its current version or claim.
- `422 invalid_request`: correct fields, required values, or validation errors. Display names and organization names are trimmed and must be 1–100 characters.
"#;

/// A static guide deliberately has no `AppState` or actor extractor, so it cannot expose tenant data.
pub async fn get() -> ([(axum::http::HeaderName, &'static str); 1], &'static str) {
    ([(CONTENT_TYPE, "text/markdown; charset=utf-8")], GUIDE)
}
