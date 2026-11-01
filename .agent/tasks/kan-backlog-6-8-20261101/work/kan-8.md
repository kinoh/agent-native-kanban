# Worker — KAN-8

Role: implementation worker; explicit model gpt-5.6-terra. Do not launch or direct other agents.
Owner: kan-8. Report directly to the director, Herdr pane wK:pK, using the reply procedure in the delegation.
Purpose: implement KAN-8 as written in ../cards.json, within ../brief.md.
Branch and worktree: kan-8-resources at /home/kino/src/agent-native-kanban/kan-8. Work only there.
Test resources: port 18813 only; state directory ~/.local/state/agent-native-kanban/kan-8/ for databases, logs and screenshots. For a running server use a database in that directory, never main/kanban.sqlite.
Allowed files: migrations/** (0004 only), src/**, tests/**, web/src/**, web/tests/**, README.md, docs/DRAFT.md, this work record. Do not edit Cargo.toml, Cargo.lock, web/package*.json.
Deliverables: Resources and holds schema and API, event visibility change, リソース screen, guide, README and DRAFT updates, API tests including concurrent acquire, browser tests.
Acceptance: every line of acceptance_criteria for KAN-8, each with evidence recorded in this file.
Verification: cargo test for the Rust suite; npm --prefix web run build then python3 web/tests/behavior.py and python3 web/tests/settings_integration.py for the web suite (npm ci first in this worktree; Playwright is already installed for python3). Read each test's own setup before running it, and do not let two runs share state. Use herdr-watch for anything over 30 seconds.
Dependencies: none on the other cards. Do not read or merge the other branches. Read ../brief.md and ../cards.json first, then /home/kino/.codex/skills/agent-coordination/references/worker.md and task-state.md.
Questions: if the card is ambiguous or a card decision does not fit the code, stop that part and ask the director with the facts. Do not change a card decision yourself.

## Execution status
In progress (2026-11-01T20:55:55+09:00).

### Intent and plan

- Add migration 0004 for organization/project resources and their active/history holds.
- Implement the `/resources` API with scope-aware authorization, explicit resource-busy/stale-hold conflicts, event/audit records, and acquire/release/revoke logs.
- Change organization-event visibility only for resource events, then add the Resources UI, guide and documentation.
- Add isolated Rust API tests (including concurrent acquire) and browser coverage; use only port 18813 and `~/.local/state/agent-native-kanban/kan-8/` for any manual server verification.

No server has been started and the worktree was clean on entry. Next: add the schema/API foundation.

### Progress — 2026-11-01T21:03:00+09:00

- Added `migrations/0004_resources.sql`, migration registration, `/resources` routes, resource/hold API implementation and resource-specific organization-event visibility.
- Added the Resources navigation and screen with create, edit, delete, acquire, release and administrator revoke controls; it reloads from the change-event stream.
- `cargo check` passed. The first web build could not find `vite/client` because this worktree has no installed `web/node_modules`; next operation is the required `npm ci` before re-running its build.

### Progress — 2026-11-01T21:12:00+09:00

- Added `tests/resources.rs`: concurrent capacity-one acquisition, same-holder and cross-holder release checks, inactive-holder revocation, scoped visibility, task-limited delegation exclusion, event visibility, immutable-scope administration and capacity/delete conflicts. `cargo test --test resources` passed (4 tests).
- Updated the API guide, README, and DRAFT resource model. The guide warns that hold reasons are shared with every viewer and must not contain secrets.
- Extended `web/tests/behavior.py` with controlled Resources create/acquire and change-stream refresh coverage, including a screenshot artifact at `~/.local/state/agent-native-kanban-ui-tests/resources.png` when run.
- Pending operation: herdr-watch run `/home/kino/.local/state/herdr-watch/20261101T210003-KAN-8-web-dependencies-npm-ci-09b39d` is installing isolated web dependencies. Do not start a second npm install; resume its result on notification.

### Verification — 2026-11-01T21:17:00+09:00

- `cargo test` passed: 75 Rust tests including the 4 resource API tests. The resource test races 12 claim-capable clients on capacity 1 and records exactly one successful hold.
- `python3 -m py_compile web/tests/behavior.py web/tests/settings_integration.py` passed. No server was started during this verification.
- `git diff --check` passed before the Rust suite. A formatter-created, unrelated diff was reverted immediately; current changes are restricted to KAN-8 files.
- Still pending the watched `npm ci`, then web build and both required isolated browser test scripts.

### Final verification — 2026-11-01T21:27:00+09:00

- Watched `npm --prefix web ci` completed successfully (run record: `/home/kino/.local/state/herdr-watch/20261101T210003-KAN-8-web-dependencies-npm-ci-09b39d/result.json`), and its watcher pane was closed.
- Final commands all passed: `cargo test` (75 tests), `npm --prefix web run build`, `python3 web/tests/behavior.py`, `python3 web/tests/settings_integration.py`, and `git diff --check`.
- Browser verification used only 127.0.0.1:18813. Both test servers stopped; the final `ss -ltn 'sport = :18813'` result is empty. Resource screenshot/result evidence: `~/.local/state/agent-native-kanban-ui-tests/resources.png` and `~/.local/state/agent-native-kanban-ui-tests/results.json`.

### Acceptance evidence

1. Create, edit and delete for organization/project resources: `resources.rs::resource_administration_keeps_scope_name_capacity_and_active_holds_consistent`; browser `resources` result has `created`, `edited`, `deleted` all true.
2. Capacity, busy details and concurrency: `resources.rs::resources_enforce_capacity_concurrently_and_keep_holds_explicit` races 12 clients on capacity 1; exactly one succeeds and all other calls receive `409 resource_busy` with one holder.
3. Mutex and same-subject repeated acquire: the same test verifies a capacity-one resource rejects another acquire; administration test fills two slots with one holder at capacity 2.
4. Holder-only release, stale holds and reuse: `resources.rs::holders_release_only_their_own_hold_and_admin_can_revoke_inactive_holder` checks another holder is rejected, active holder release succeeds, stale release is rejected, and release/reacquire are covered by the browser flow.
5. Administrator revoke: the same API test revokes an inactive holder and rejects its stale hold; browser result `revoked: true` covers the control.
6. Inactive/deleted holder persistence: API test disables then deletes the holder, observes its active resource hold with `status: deleted`, then administrator revokes it.
7. Delete/capacity conflicts: administration test checks `resource_held` and `resource_capacity_in_use` before release, then successful delete.
8. Access isolation and delegation: `resources.rs::resource_visibility_events_and_delegation_boundaries_are_preserved` checks hidden project and other-organization 404s, organization visibility, and task-limited delegation list/get/acquire exclusion.
9. Events/audit/reacquire: resource operations emit `resource.created|updated|deleted|acquired|released|revoked` with actor, resource, reason where applicable and delegated token ID; the access test verifies regular organization event visibility. Browser test verifies change-stream refresh after an external hold and user release/revoke.
10. Logs: resource create/update/delete/acquire/release/revoke and busy/non-holder rejection use structured tracing logs with resource, hold and actor IDs.
11. Realtime screen: browser `resources` result confirms external hold refresh without reload; all create/edit/delete/acquire/release/revoke controls were exercised.
12. Guide/README/DRAFT: implementation documentation was added and the public guide is covered by the passing guide tests; it documents shared hold reasons and forbids secrets.

### Delivery

- Final implementation commit: `0fafda4 共有リソースを安全に確保できるようにする`.
- Worktree was clean immediately after that commit. No server or port remains in use.
- Next: send the director report through the assigned herdr-watch reply path.

### Review follow-up — 2026-11-01T21:30:00+09:00

- Director requested four corrections: restore the integration test's ephemeral OS port; place the Resources link after the project group; move shared-resources documentation to `6.1` without renumbering existing sections; and log delegated organization-resource claim denial.
- Applied all four. The integration test's dynamic port is explicitly approved by the director and is excluded from the work-record port allocation.
- Reverification passed: `cargo test` (75 tests), `npm --prefix web run build`, `python3 web/tests/behavior.py`, `python3 web/tests/settings_integration.py`, and `git diff --check`.
- Follow-up commit: `ee15a00 共有リソースのレビュー指摘を反映する` (no amend).
- Worktree is clean. Next: send the director report through the assigned herdr-watch reply path.
