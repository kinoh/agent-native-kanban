# Worker — KAN-16

Role: implementation worker; explicit model gpt-5.6-terra. Do not launch or direct other agents.
Owner: kan-16. Report directly to the director, Herdr pane wK:pK, using the reply procedure in the delegation.
Purpose: implement KAN-16 as written in ../cards.json, within ../brief.md.
Branch and worktree: kan-16-review-card at /home/kino/src/agent-native-kanban/kan-16. Work only there. This record is at .agent/tasks/kan-backlog-11-16-20261102/work/kan-16.md in that worktree; update and commit it there.
Test resources: port 18825 only; state directory ~/.local/state/agent-native-kanban/kan-16/ for databases, logs and screenshots. For a running server use a database in that directory, never main/kanban.sqlite.
Allowed files: src/**, tests/**, web/src/**, web/tests/**, src/api/guide.rs and README.md lines for this change, this work record. Do not edit Cargo.toml, Cargo.lock, web/package*.json.
Deliverables:
The board reaches the latest review_result body, author and time and the related detail, per the card. Do not derive pass or fail from the body. Distinguish no review yet. If GET /tasks must gain a field, keep the query cost bounded (no per-card request from the board, no N+1 query) and apply the same visibility as comments, including task-limited delegated tokens.
Acceptance: every line of acceptance_criteria for the card, each with evidence recorded in this file.
Verification: API tests for visibility and for the delegated-token scope if the API changes, and browser tests for latest of several reviews, no review, a free-form body, and live update; both themes. Also cargo test for the Rust suite; npm --prefix web run build then python3 web/tests/behavior.py, python3 web/tests/theme.py and python3 web/tests/settings_integration.py for the web suite (npm ci first in this worktree; Playwright is already installed for python3). Read each test's own setup before running it, and do not let two runs share state: the shared ~/.local/state/agent-native-kanban-ui-tests/ directory is also used by other workers, so give your run its own location if the test allows it, and tell the director if it does not. Use herdr-watch for anything over 30 seconds.
Dependencies: none on the other cards. Do not read or merge the other branches. Read ../brief.md and ../cards.json first, then /home/kino/.codex/skills/agent-coordination/references/worker.md and task-state.md.
Questions: if the card is ambiguous or a card decision does not fit the code, stop that part and ask the director with the facts. Do not change a card decision yourself.

## Execution status
