# Worker — KAN-13

Role: implementation worker; explicit model gpt-5.6-terra. Do not launch or direct other agents.
Owner: kan-13. Report directly to the director, Herdr pane wK:pK, using the reply procedure in the delegation.
Purpose: implement KAN-13 as written in ../cards.json, within ../brief.md.
Branch and worktree: kan-13-occupant-reason at /home/kino/src/agent-native-kanban/kan-13. Work only there. This record is at .agent/tasks/kan-backlog-11-16-20261102/work/kan-13.md in that worktree; update and commit it there.
Test resources: port 18823 only; state directory ~/.local/state/agent-native-kanban/kan-13/ for databases, logs and screenshots. For a running server use a database in that directory, never main/kanban.sqlite.
Allowed files: src/**, tests/**, web/src/** only for places that display the reason, src/api/guide.rs and README.md lines for this change, this work record. Do not edit Cargo.toml, Cargo.lock, web/package*.json.
Deliverables:
Reason text that differs for the occupant and for others, per the card. The starting point is src/model.rs:145. can_claim stays false, reclaiming stays rejected, and claim_id stays visible to its owner only.
Acceptance: every line of acceptance_criteria for the card, each with evidence recorded in this file.
Verification: API tests with principals A and B after claim, start, pause, resume and transfer, and on GET task and GET context. Check list responses too, and cover the delegated-token case of the same principal. Also cargo test for the Rust suite; npm --prefix web run build then python3 web/tests/behavior.py, python3 web/tests/theme.py and python3 web/tests/settings_integration.py for the web suite (npm ci first in this worktree; Playwright is already installed for python3). Read each test's own setup before running it, and do not let two runs share state: the shared ~/.local/state/agent-native-kanban-ui-tests/ directory is also used by other workers, so give your run its own location if the test allows it, and tell the director if it does not. Use herdr-watch for anything over 30 seconds.
Dependencies: none on the other cards. Do not read or merge the other branches. Read ../brief.md and ../cards.json first, then /home/kino/.codex/skills/agent-coordination/references/worker.md and task-state.md.
Questions: if the card is ambiguous or a card decision does not fit the code, stop that part and ask the director with the facts. Do not change a card decision yourself.

## Execution status
