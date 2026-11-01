# Status

Last updated: 2026-11-02 JST. Director: wK:pK. State: DONE — KAN-17, 19, 20 are in 完了 on the live board by user request ("17,19,20 完了させて").

## Delivered

- KAN-19, KAN-20: merged into main at 3e09190 (same tree as reviewed 4b8b9b4). main/web/dist was rebuilt, and the live server serves it from disk, so both are live. The server was not restarted; no backend change.
- KAN-17: agent-config 417a901 (skills/kanban/SKILL.md, scripts/run_as.py). Committed locally and not pushed: agent-config is a public repository and publishing needs approval. The skill is symlinked into ~/.claude/skills, so it is already in effect here.

## Decisions

- User decision: delegation needs no human judgement. An agent with organization-admin authority may create principals and tokens at any time. Non-admin principals can only narrow their own token with /me/delegations.
- KAN-19 removes the whole review block from cards. The API field latest_review_result stays; the result is read on the task detail.

## Verification

- Live cycle: kan-worker and kan-reviewer each used their own run_as.py token. KAN-19 and 20 claim history is kan-worker → kan-reviewer. KAN-17 was implemented by agent and reviewed by kan-reviewer.
- Rejection was not exercised live because all reviews passed. It was run against a .backup copy on port 18824 to avoid a test card on the live board: ~/.local/state/agent-native-kanban/backlog-17-20/rework/result.json.
- Post-deploy check of the live UI in both themes: ~/.local/state/agent-native-kanban/backlog-17-20/live/.

## State left behind

kan-worker and kan-reviewer stay as principals for their history; their tokens are revoked. Worktrees, panes and DB copies for this work are removed; branch kan-19-20 remains. Reports: ~/.local/state/herdr-watch/20261102T020418-KAN-19-20-7dafb5/, 20261102T021012-watch-87f654/, 20261102T021529-KAN-17-00fb2a/.
