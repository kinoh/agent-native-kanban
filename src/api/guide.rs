//! 認証前に API の利用手順を示す静的な入口。

use axum::http::header::CONTENT_TYPE;

const GUIDE: &str = r#"# Agent-Native Kanban API guide

Base URL: `https://your-kanban.example/api/v1` (replace the host with this server).

## Start here

`GET /guide` is public and returns this UTF-8 Markdown document. All endpoints below except guide, signup, and login require authentication. Do not put a token in a URL, a task comment, or a log.

An organization administrator creates an agent and issues its token once:

```sh
# Use an administrator credential for these two calls.
API=https://your-kanban.example/api/v1
H="Authorization: Bearer <administrator-token>"
J='Content-Type: application/json'
curl -H "$H" -H "$J" -d '{
  "display_name":"Build agent",
  "grants":[
    {"project_id":"<project-id>","permission":"claim"},
    {"project_id":"<project-id>","permission":"edit"}
  ]
}' "$API/principals"
curl -H "$H" -H "$J" -d '{"name":"build-agent-cli"}' \
  "$API/principals/<principal-id>/tokens"
```

The second response contains its `token` exactly once. Send it on every API request:

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

Claiming returns the `work.claim_id` that proves the holder may operate on that task in its current stage. A successful path is claim → start → record progress/evidence → complete. Keep the returned `claim_id` unchanged until completion:

```sh
curl -H "$H" -H "$J" -X POST "$API/tasks/PROJ-12/work/claim"
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>"}' "$API/tasks/PROJ-12/work/start"
curl -H "$H" -H "$J" -d '{"kind":"progress","body":"implementation is ready"}' \
  "$API/tasks/PROJ-12/comments"
curl -H "$H" -H "$J" -d '{"role":"artifact","name":"report","url":"https://example.invalid/report"}' \
  "$API/tasks/PROJ-12/artifacts"
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>","summary":"what changed","evidence":[{"name":"report","url":"https://example.invalid/report"}]}' "$API/tasks/PROJ-12/work/complete"
```

`work/complete` records that the current work is done; it does **not** move the task to a final business stage. Use `POST /tasks/{task}/stage` with `to_stage_id` and the task's current `version` when the workflow permits that move. A stage that requires work opens a fresh claim cycle.

Pause/resume, release, and transfer are alternatives to finishing the current claim, not steps before `complete`:

```sh
# A temporary pause keeps the same holder and claim.
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>"}' "$API/tasks/PROJ-12/work/pause"
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>"}' "$API/tasks/PROJ-12/work/resume"

# Releasing ends this claim. A later holder must claim again and use its new claim_id.
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>","note":"handoff notes"}' "$API/tasks/PROJ-12/work/release"

# Transfer ends this claim and creates a claimed work item for an eligible target.
curl -H "$H" -H "$J" -d '{"claim_id":"<claim-id>","to":"<principal-id>","note":"handoff notes"}' \
  "$API/tasks/PROJ-12/work/transfer"
```

## Share limited resources

Resources represent shared capacity such as a test environment. An organization resource is visible to every principal in the organization; a project resource is visible to principals who can see that whole project. Task-limited delegated tokens cannot read or acquire resources. A claim-capable principal can acquire one slot, and an administrator creates, changes, deletes, or revokes holds. Do not put secrets in `reason`: every principal who can view the resource can read it in its hold and event history.

```sh
# An organization administrator creates an organization resource. Use project_id instead
# for a project resource; the scope cannot be changed later.
curl -H "$H" -H "$J" -d '{"name":"test-environment","description":"integration tests","capacity":1}' \
  "$API/resources"
curl -H "$H" "$API/resources"

# A claim-capable principal acquires one slot. Keep hold_id until it is released.
curl -H "$H" -H "$J" -d '{"reason":"run acceptance tests"}' \
  "$API/resources/<resource-id>/acquire"
curl -H "$H" -H "$J" -d '{"hold_id":"<hold-id>"}' \
  "$API/resources/<resource-id>/release"

# An administrator explicitly recovers a forgotten hold.
curl -H "$H" -H "$J" -d '{"hold_id":"<hold-id>","reason":"holder is inactive"}' \
  "$API/resources/<resource-id>/revoke"
```

There is no queue, lease, timeout, or automatic release. When `acquire` returns `409 resource_busy`, inspect `error.details.holders` or subscribe to events and retry after a release. A resource with active holds cannot be deleted, and capacity cannot be reduced below its active hold count.

## Record context and collaborate

Use `POST /tasks/{task}/comments` with `kind` and `body` for comments, progress, decisions, problems, and answers. Questions and review requests also need `addressee_id`; answers need `parent_id`. Use `POST /tasks/{task}/artifacts` with `{ "role":"artifact|attachment", "name":"…", "url":"https://…" }` for links, or `POST /tasks/{task}/files?role=artifact&name=report.txt` for uploads. Add and remove dependencies with `POST /tasks/{task}/dependencies` (`{ "depends_on":"<id-or-key>", "reason":"…" }`) and `DELETE /tasks/{task}/dependencies/{other}`; use `POST /tasks/{task}/links` with `{ "related":"<id-or-key>" }` and `DELETE /tasks/{task}/links/{other}` for related tasks. A dependency blocks claimability until its task reaches a final stage. `GET /tasks/{task}/history` and `GET /events?after=<seq|latest>` expose audit history; retain each event response's `cursor` for the next request. `GET /events/stream?after=<seq|latest>` is the SSE equivalent.

## Profile and password

Use `PATCH /me` with `{ "display_name":"New name" }` for a human self-name change, `PUT /me/password` with `{ "current_password":"…", "new_password":"…" }` for a human password change, and `PATCH /principals/{id}` with `{ "display_name":"New name" }` for an organization-admin rename. Passwords must be at least 12 characters. A successful self password change keeps the acting cookie session and invalidates other sessions.

## Create, update, and move tasks

Creating a task requires `create`; assigning people also requires `assign`. Updating a task requires its current `version` so conflicting edits return `409 version_conflict` with the current version.

```sh
curl -H "$H" -H "$J" -d '{"project_id":"<project-id>","stage_id":"<stage-id>","title":"Implement guide"}' "$API/tasks"
curl -H "$H" -H "$J" -X PATCH -d '{"version":3,"title":"Revised guide","priority":"high"}' "$API/tasks/PROJ-12"
curl -H "$H" -H "$J" -d '{"to_stage_id":"<stage-id>","version":4}' "$API/tasks/PROJ-12/stage"
```

`POST /tasks/{task}/work/reopen` and `POST /tasks/{task}/work/hold` require `{ "reason":"…" }` and edit authority; `POST /tasks/{task}/work/unhold` takes `{}`. Administrators can use `work/revoke` (`{ "reason":"…" }`) and `work/reassign` (`{ "to":"<principal-id>", "reason":"…" }`).

## API map and authority

| Area | Endpoints | Authority |
| --- | --- | --- |
| Identity | `GET|PATCH /me`, `PUT /me/password`, `GET /me/inbox` | any authenticated principal may read `/me`; only a non-delegated human may update its own name/password |
| Organization | `PATCH /organization` with `{ "name":"…" }` | organization administrator |
| Principals and credentials | `GET|POST /principals`; `GET /principals/work`; `GET|PATCH|DELETE /principals/{id}`; `PUT /principals/{id}/password`; `GET|POST /principals/{id}/grants`; `DELETE /grants/{id}`; `GET|POST /principals/{id}/tokens`; `DELETE /tokens/{id}`; `GET|POST /signup-links`; `DELETE /signup-links/{id}` | principals are readable within the organization; `/principals/work` lists every principal with its current reported claims, but includes only tasks visible to the credential (and never exposes omitted-task counts); principal changes and signup links require organization admin; grants require admin in the target scope; normal token listing/issuance requires organization admin or the non-delegated human owner |
| Projects and workflow | `GET|POST /projects`; `GET|PATCH /projects/{id}`; `POST /projects/{id}/stages`; `PATCH|DELETE /stages/{id}`; `PUT /projects/{id}/transition-rules` with `{ "rules":[{ "from_stage_id":"…", "to_stage_id":"…", "required_permission":"edit", "require_work_done":true }] }` | reading requires project visibility; creating a project requires organization admin; changing a project, its stages or transition rules requires admin for that project |
| Resources | `GET|POST /resources`; `GET|PATCH|DELETE /resources/{id}`; `POST /resources/{id}/{acquire,release,revoke}` | project resources: visible to whole-project viewers, acquired with `claim`, administered with project `admin`; organization resources: visible to all organization principals, acquired by any claim-capable principal, administered by organization admin; task-limited delegations cannot access resources |
| Tasks | `GET|POST /tasks`; `GET|PATCH /tasks/{task}`; `GET /tasks/{task}/context`; `GET /tasks/{task}/history`; `POST /tasks/{task}/stage` | visible project grant and operation-specific permission |
| Work | `POST /tasks/{task}/work/{claim,start,pause,resume,release,transfer,complete,reopen,hold,unhold,revoke,reassign}` | holder, edit, or admin as described above |
| Collaboration | `POST /tasks/{task}/{comments,artifacts,files,dependencies,links}`; `DELETE /tasks/{task}/{dependencies,links}/{other}`; `GET /artifacts/{id}/content` | visible project grant and operation-specific permission |
| Delegation | `GET|POST /me/delegations`; `DELETE /tokens/{id}` | a bearer token, including a delegated token when the requested child scope/tasks are no wider than its own |
| Events | `GET /events`, `GET /events/stream` | only events visible to the current credential |

Permissions are `view`, `create`, `edit`, `claim`, `assign`, `review`, `approve`, and `admin`; `admin` includes all permissions in its organization or project scope. Delegations never add authority: their scopes and optional task list can only narrow the parent token, they expire with or before it, and revoking any ancestor revokes its descendants. Delegated tokens cannot list or issue ordinary tokens, change a self profile/password, or acquire admin authority. They may create narrower delegations and revoke themselves or descendant tokens. Choose an expiry in the future that does not exceed the parent expiry; the date below is illustrative.

```sh
# The current bearer token (root or delegated) may issue this narrower child.
curl -H "$H" -H "$J" -d '{
  "name":"subtask-worker",
  "expires_at":"2030-01-01T00:00:00Z",
  "scopes":[{"project_id":"<project-id>","permission":"claim"}],
  "tasks":["PROJ-12"]
}' "$API/me/delegations"
curl -H "$H" "$API/me/delegations"       # descendants issued from this token
curl -H "$H" -X DELETE "$API/tokens/<token-id>" # revoke this token or a descendant
```

## Errors and recovery

Errors are JSON: `{ "error": { "code": "…", "message": "…", "details": { … } } }`.

- `401 unauthenticated`: authenticate again or replace an expired/revoked token.
- `403 permission_denied`: obtain the required grant; `details.required_permission` names it. `403 delegated_token_restricted` means use a non-delegated credential for identity or credential management.
- `404 not_found`: the resource does not exist or is outside your visible scope.
- `409 not_claimable`: inspect `details.reasons`; common states are no work required, an unfinished dependency, work already done, or an occupied task. `task_occupied` rejects a stage move while a holder remains.
- `409 stale_claim`: the holder changed or released/completed work; claim again and use the returned claim ID. `409 version_conflict` means reread the task and retry with its current version. `409 invalid_activity_transition` means start/pause/resume was attempted from the wrong activity.
- `409 resource_busy`: all capacity is held; `details.holders` identifies the current holders. `stale_hold` means a hold was released or revoked; `hold_not_owned` means another principal holds it.
- `422 invalid_request`: correct fields, required values, or validation errors. Display names and organization names are trimmed and must be 1–100 characters.
"#;

/// A static guide deliberately has no `AppState` or actor extractor, so it cannot expose tenant data.
pub async fn get() -> ([(axum::http::HeaderName, &'static str); 1], &'static str) {
    ([(CONTENT_TYPE, "text/markdown; charset=utf-8")], GUIDE)
}
