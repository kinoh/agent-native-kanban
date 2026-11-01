# Worker A — backend and agent guide

Role: implementation worker; explicit model gpt-5.6-terra.
Owner: kan-backend (Herdr pane recorded in status.md). Report directly to director wK:p1.
Purpose: implement KAN-5 end-to-end and the backend contracts needed by KAN-1/KAN-3; preserve KAN-2/KAN-4 existing backend behavior.
Allowed files: src/**, tests/** (Rust integration tests), README.md, docs/**, and this work record only. Do not edit web/**, Cargo.toml/Cargo.lock, other .agent files without director agreement.
Deliverables: unauthenticated guide, organization/self rename APIs and audit events, meaningful API tests and README guidance. No speculative changes to work/auth permission semantics.
Order: implement new API contracts promptly so frontend can integrate; prioritize guide and relevant API checks. Coordinate ordinary contract observations by reporting to director.
Acceptance: all backend criteria in KAN-1/3/5 and no regression of existing rename/password behavior; guide workflow executable against isolated test data, input/auth boundaries tested. Run relevant Rust tests; use herdr-watch for whole suite. Director will review integration.
Dependencies: frontend worker consumes the explicit contract in brief.md; no frontend edits needed. Read src/api/mod.rs, identity.rs, admin.rs, README.md, docs/DRAFT.md and tests/common/mod.rs.

## Constraints and workflow
Read ../brief.md and ../cards.json first, then the agent-coordination worker reference. No other agents. Do not change assignment scope or unowned files; report needs directly to director. Same checkout/main shared by both workers; preserve concurrent changes. Record intent/outcome and verification in this file. Commit only owned files, with Git mutations serialized via flock ~/.local/state/agent-native-kanban/git.lock. A safe pattern is flock <lock> bash -c 'git add -- <exact owned paths> && git commit -m <purpose>'. Do not hold the Git lock during tests. Use herdr-watch for long builds/tests and its supplied reply instructions to report. No live Kanban writes; director owns those. Do not expose KANBAN_TOKEN. Keep artifacts under ~/.local/state/agent-native-kanban/.

## Execution status
In progress. Last updated 2026-11-01 JST. Initial tree clean; no pre-existing modifications.

Intent: add the agreed `PATCH /organization`, `PATCH /me`, and unauthenticated `GET /guide` contracts; document the guide from the static API contract; add API integration tests covering success, validation, authorization/delegation boundaries, audit events, and unauthenticated guide content. Affected owned files: `src/api/**`, `tests/**`, `README.md`, and this record. Existing principal rename/password behavior is not being changed.

Observed: `GET /me` already owns the response shape; `required_text` implements the agreed trimmed 1–100 character validation; delegated credentials are rejected by the existing `reject_delegated` helper.

Implemented: added `GET /api/v1/guide` with a static UTF-8 Markdown contract; `PATCH /api/v1/organization` (organization-admin-only, inferred organization); and `PATCH /api/v1/me` (non-delegated human self only). Both rename operations trim/validate 1–100 characters and emit audit events with actor, target, and old/new names. `PATCH /me` returns the existing `/me` shape. README now links the guide and lists the new API endpoints. Added API tests for public guide headers/content and protected data, rename success, validation, organization/principal/auth boundaries, audit events, and delegated self-update rejection.

Verification completed: `cargo test --test access` (19 passed), `cargo test --test delegation` (19 passed), and full `cargo test` through herdr-watch (exit 0; watcher record `~/.local/state/herdr-watch/20261101T202758-KAN-backend-full-Rust-test-suite-a1e25a/`; work tests 11 passed; doc tests 0). The watcher pane `wK:pE` was closed after result inspection.

Committed implementation and tests as `66a1b2c Add agent API guide and rename endpoints`, staging only this worker's owned paths while preserving the concurrent frontend and unrelated backend working-tree changes. Current state: worker scope complete; next action is report to director for integration review.
