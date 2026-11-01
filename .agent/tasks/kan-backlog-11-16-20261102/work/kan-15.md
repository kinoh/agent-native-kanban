# Worker — KAN-15

Role: implementation worker; explicit model gpt-5.6-terra. Do not launch or direct other agents.
Owner: kan-15. Report directly to the director, Herdr pane wK:pK, using the reply procedure in the delegation.
Purpose: implement KAN-15 as written in ../cards.json, within ../brief.md.
Branch and worktree: kan-15-blocker-card at /home/kino/src/agent-native-kanban/kan-15. Work only there. This record is at .agent/tasks/kan-backlog-11-16-20261102/work/kan-15.md in that worktree; update and commit it there.
Test resources: port 18824 only; state directory ~/.local/state/agent-native-kanban/kan-15/ for databases, logs and screenshots. For a running server use a database in that directory, never main/kanban.sqlite.
Allowed files: web/src/**, web/tests/**, this work record. No src/** change is expected, because work.blockers is already in GET /tasks. Do not edit Cargo.toml, Cargo.lock, web/package*.json.
Deliverables:
The 条件待ち card shows the dependency keys and reasons the viewer may see, and moves to the dependency task, per the card.
Acceptance: every line of acceptance_criteria for the card, each with evidence recorded in this file.
Verification: Browser tests with a real or fixed response for visible blockers, hidden_blockers (no key, title or reason leak), navigating to the dependency, and the card updating after the dependency completes; both themes. Also cargo test for the Rust suite; npm --prefix web run build then python3 web/tests/behavior.py, python3 web/tests/theme.py and python3 web/tests/settings_integration.py for the web suite (npm ci first in this worktree; Playwright is already installed for python3). Read each test's own setup before running it, and do not let two runs share state: the shared ~/.local/state/agent-native-kanban-ui-tests/ directory is also used by other workers, so give your run its own location if the test allows it, and tell the director if it does not. Use herdr-watch for anything over 30 seconds.
Dependencies: none on the other cards. Do not read or merge the other branches. Read ../brief.md and ../cards.json first, then /home/kino/.codex/skills/agent-coordination/references/worker.md and task-state.md.
Questions: if the card is ambiguous or a card decision does not fit the code, stop that part and ask the director with the facts. Do not change a card decision yourself.

## Execution status
