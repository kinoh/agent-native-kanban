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
Assigned; not started. Last updated 2026-11-01 JST. Initial tree clean; no pre-existing modifications. Record commits, commands, results, pending operations, and next action here.
