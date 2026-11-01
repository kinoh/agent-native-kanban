# Worker — KAN-12

Role: implementation worker; explicit model gpt-5.6-terra. Do not launch or direct other agents.
Owner: kan-12. Report directly to the director, Herdr pane wK:pK, using the reply procedure in the delegation.
Purpose: implement KAN-12 as written in ../cards.json, within ../brief.md.
Branch and worktree: kan-12-guide at /home/kino/src/agent-native-kanban/kan-12. Work only there. This record is at .agent/tasks/kan-backlog-11-16-20261102/work/kan-12.md in that worktree; update and commit it there.
Test resources: port 18822 only; state directory ~/.local/state/agent-native-kanban/kan-12/ for databases, logs and screenshots. For a running server use a database in that directory, never main/kanban.sqlite.
Allowed files: src/api/guide.rs, tests/**, README.md, this work record. Other src/** files only if the guide cannot be made correct without them; ask the director first. Do not edit Cargo.toml, Cargo.lock, web/package*.json.
Deliverables:
GET /guide lets a new agent create a project, stages, detailed tasks with purpose, acceptance criteria, priority and parent, and dependencies without reading source. It states required fields, enum values and success status/body shapes of the main operations.
Acceptance: every line of acceptance_criteria for the card, each with evidence recorded in this file.
Verification: Every example and enum value in the guide checked against the real handlers, by a test where practical. Then perform the card's sequence on an isolated server using only the guide text, and record each request and response status in this record. Also cargo test for the Rust suite; npm --prefix web run build then python3 web/tests/behavior.py, python3 web/tests/theme.py and python3 web/tests/settings_integration.py for the web suite (npm ci first in this worktree; Playwright is already installed for python3). Read each test's own setup before running it, and do not let two runs share state: the shared ~/.local/state/agent-native-kanban-ui-tests/ directory is also used by other workers, so give your run its own location if the test allows it, and tell the director if it does not. Use herdr-watch for anything over 30 seconds.
Dependencies: none on the other cards. Do not read or merge the other branches. Read ../brief.md and ../cards.json first, then /home/kino/.codex/skills/agent-coordination/references/worker.md and task-state.md.
Questions: if the card is ambiguous or a card decision does not fit the code, stop that part and ask the director with the facts. Do not change a card decision yourself.

## Execution status
