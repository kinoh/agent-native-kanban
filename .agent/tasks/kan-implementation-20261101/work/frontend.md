# Worker B — account and organization UI

Role: implementation worker; explicit model gpt-5.6-terra.
Owner: kan-frontend (Herdr pane recorded in status.md). Report directly to director wK:p1.
Purpose: implement frontend behavior of KAN-1/2/3/4 with existing APIs and the new backend contracts in brief.md.
Allowed files: web/** and this work record only. Do not edit package manifests/locks to add dependencies without director agreement. Do not edit src/**, tests/** (root Rust tests), README, other .agent files.
Deliverables: human self settings (display name/password), admin organization and principal rename controls, immediate refreshed identity/organization displays, error and pending states, meaningful UI behavior checks. Read stop-describing-ui skill before UI work.
Order: self password and profile settings, admin principal rename, organization rename. Reuse existing UI styles/components. Existing Session/App identity refresh may need a callback; own that frontend integration. Do not weaken existing API semantics.
Acceptance: all UI criteria of KAN-1–4; general users can access self settings, admin-only controls remain restricted, failure/cancel behavior correct, latest names reflected without reload, password secret handling and session behavior respected. Run build and browser checks using isolated fixtures/server, never mutate live data. Contract responses and endpoints are in brief.md. Backend worker owns server APIs.
Dependencies: new organization and self-name APIs are implemented by backend worker. Mock those exact contracts while developing, report any mismatch to director. Read web/src/App.tsx, views/Admin.tsx, api.ts, ui.tsx, types.ts and web/tests/behavior.py.

## Constraints and workflow
Read ../brief.md and ../cards.json first, then the agent-coordination worker reference. No other agents. Do not change assignment scope or unowned files; report needs directly to director. Same checkout/main shared by both workers; preserve concurrent changes. Record intent/outcome and verification in this file. Commit only owned files, with Git mutations serialized via flock ~/.local/state/agent-native-kanban/git.lock. A safe pattern is flock <lock> bash -c 'git add -- <exact owned paths> && git commit -m <purpose>'. Do not hold the Git lock during tests. Use herdr-watch for long builds/tests and its supplied reply instructions to report. No live Kanban writes; director owns those. Do not expose KANBAN_TOKEN. Keep artifacts under ~/.local/state/agent-native-kanban/.

## Execution status
Assigned; not started. Last updated 2026-11-01 JST. Initial tree clean; no pre-existing modifications. Record commits, commands, results, pending operations, and next action here.
