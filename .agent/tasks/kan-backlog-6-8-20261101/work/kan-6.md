# Worker — KAN-6

Role: implementation worker; explicit model gpt-5.6-terra. Do not launch or direct other agents.
Owner: kan-6. Report directly to the director, Herdr pane wK:pK, using the reply procedure in the delegation.
Purpose: implement KAN-6 as written in ../cards.json, within ../brief.md.
Branch and worktree: kan-6-principal-work at /home/kino/src/agent-native-kanban/kan-6. Work only there.
Test resources: port 18811 only; state directory ~/.local/state/agent-native-kanban/kan-6/ for databases, logs and screenshots. For a running server use a database in that directory, never main/kanban.sqlite.
Allowed files: src/**, tests/**, web/src/**, web/tests/**, README.md, this work record. No migrations. Do not edit Cargo.toml, Cargo.lock, web/package*.json.
Deliverables: GET /api/v1/principals/work, the 作業 column on 組織の管理 with live updates, guide and README entries, API and browser tests.
Acceptance: every line of acceptance_criteria for KAN-6, each with evidence recorded in this file.
Verification: cargo test for the Rust suite; npm --prefix web run build then python3 web/tests/behavior.py and python3 web/tests/settings_integration.py for the web suite (npm ci first in this worktree; Playwright is already installed for python3). Read each test's own setup before running it, and do not let two runs share state. Use herdr-watch for anything over 30 seconds.
Dependencies: none on the other cards. Do not read or merge the other branches. Read ../brief.md and ../cards.json first, then /home/kino/.codex/skills/agent-coordination/references/worker.md and task-state.md.
Questions: if the card is ambiguous or a card decision does not fit the code, stop that part and ask the director with the facts. Do not change a card decision yourself.

## Execution status
Not started.
