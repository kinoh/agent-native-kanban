# Status

Last updated: 2026-11-01 JST. State: IMPLEMENTED — KAN-10 and KAN-9 are implemented and verified on their branches, and are in レビュー on the live board. Not merged into main; the live server has not been rebuilt or restarted.

User request: 「バックログアイテムを実装可能にして」 then 「実装して」. Implemented directly by the user-facing Claude session without worker agents. Merging, deploying and completing the cards were not requested and remain the user's decision.

| Card | Branch / worktree | Commits |
|---|---|---|
| KAN-10 | kan-10-project-overview, ../kan-10 | 0175a80 |
| KAN-9 | kan-9-discard, ../kan-9 (based on the KAN-10 branch) | dff623d and later test and record commits |

Merging kan-9-discard brings KAN-10 with it.

## Decisions agreed with the user

KAN-9: strikethrough plus a 破棄 badge; a discarded dependency keeps blocking. KAN-10: repository details live in the free-text overview; the overview appears on the board, list and task detail and in GET /tasks/{task}/context.

## Outcome and deviations from the card text

- KAN-9: the card said restoring releases dependents. Restoring returns the task to a non-final stage, so dependents stay blocked as an ordinary unfinished dependency until the task reaches a final stage. Implemented that way; the live card text was corrected.
- KAN-10: the 2000-character limit also applies to POST /projects, so that the limit cannot be bypassed at creation.

## Verification

At the kan-9-discard tip: cargo test passed (90 tests, including tests/project_overview.rs and tests/discard.rs); web build, behavior.py, theme.py and settings_integration.py passed. Migration 0005 on a backup of the live DB moved schema 4 to 5 with every existing row unchanged. End-to-end run on an isolated real server passed, including live updates, agent API use, drag rejection, restore, audit log lines, and text contrast of at least 4.5:1 in both themes.

Scripts and results: ~/.local/state/agent-native-kanban/kan-9-10/ (verify-all.sh, e2e.py, migrate_check.py, e2e-result.json, board-light.png, board-dark.png, detail.png).

## Remaining

Deploying needs a build and restart together, because the new web/dist depends on the new API. Migration 0005 is applied on start, and the previous binary refuses a database with a newer schema, so take a DB backup first.
