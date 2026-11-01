# Worker — KAN-7

Role: implementation worker; explicit model gpt-5.6-terra. Do not launch or direct other agents.
Owner: kan-7. Report directly to the director, Herdr pane wK:pK, using the reply procedure in the delegation.
Purpose: implement KAN-7 as written in ../cards.json, within ../brief.md.
Branch and worktree: kan-7-theme at /home/kino/src/agent-native-kanban/kan-7. Work only there.
Test resources: port 18812 only; state directory ~/.local/state/agent-native-kanban/kan-7/ for databases, logs and screenshots. For a running server use a database in that directory, never main/kanban.sqlite.
Allowed files: web/src/**, web/index.html, web/tests/**, this work record. No server, API or DB change. Do not edit web/package*.json.
Deliverables: Light palette, sidebar toggle, persistence, pre-paint application, colour variables throughout styles.css, browser tests and screenshots of both themes in the state directory.
Acceptance: every line of acceptance_criteria for KAN-7, each with evidence recorded in this file.
Verification: cargo test for the Rust suite; npm --prefix web run build then python3 web/tests/behavior.py and python3 web/tests/settings_integration.py for the web suite (npm ci first in this worktree; Playwright is already installed for python3). Read each test's own setup before running it, and do not let two runs share state. Use herdr-watch for anything over 30 seconds.
Dependencies: none on the other cards. Do not read or merge the other branches. Read ../brief.md and ../cards.json first, then /home/kino/.codex/skills/agent-coordination/references/worker.md and task-state.md.
Questions: if the card is ambiguous or a card decision does not fit the code, stop that part and ask the director with the facts. Do not change a card decision yourself.

## Execution status
Not started.
