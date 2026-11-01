# KAN-11 to KAN-16 implementation

## User requirements and authorization
User requested 「並列に全てやりましょう」 for the whole KAN backlog (KAN-11 to KAN-16) after reviewing it. KAN-14 was agreed as a tree of the principal creation relation on /admin, not token delegation.
Implement every card with independent workers in their own Git worktrees through Herdr. Card text is in cards.json and on the live board.
Merging into main, restarting the live server and moving cards to 完了 are not authorized by this request; the director decides them with the user after review.

## Roles
Director: the user-facing Claude session in Herdr pane wK:pK. It owns user communication, live Kanban mutations, review, integration and acceptance. Where the agent-coordination references say Astra, read this director.
Workers: Codex sessions, explicit model gpt-5.6-terra. They implement and verify on their own branch only. They may not spawn or direct other agents.

## Observed facts
/home/kino/src/agent-native-kanban is a bare repository with worktrees beside it; main is at ../main relative to each worker worktree. No Git remote. Rust/axum/SQLite backend, React/Vite frontend. The live server listens on 127.0.0.1:18787 from main/kanban.sqlite.

## Assignments
KAN-11 and KAN-14 go to one worker because both change the same name cell of the /admin principal table; each card gets its own commit(s). Every other card has its own worker. All branches start at the same main commit.

## Integration contract
- Each worker commits only on its own branch and never merges, rebases, amends or force-updates. The director integrates after review with merge commits.
- Expected overlap: web/src/views/Board.tsx and web/src/styles.css (KAN-15, KAN-16); src/api/guide.rs and README.md (KAN-12 and any card that adds API fields).
- KAN-12 owns the structure of guide.rs. Other cards add only the lines for their own API change.
- KAN-15 and KAN-16: put the new card display in its own small component with its own CSS block, and keep the edit to the existing card markup as small as possible, so that the merge stays simple. Do not read the other branch.
- New CSS uses existing CSS variables only and adds no direct colour values. Do not rename or remove existing variables or refactor unrelated CSS. Both themes must remain readable.
- No migration is expected. If a card appears to need one, ask the director first.

## Shared constraints
Read actual code and existing documents first. Preserve agreed scope and the decisions in the card; no unapproved workaround, implicit fallback, unapproved default value, unnecessary dependency or unrelated feature. Design explicit errors and useful audit/debug logs from the start. Tests describe the behaviour the product needs, not incidental behaviour.
Commit each unit of work before stopping or handing off; no uncommitted changes at the end. Never amend, rebase or force-push. Commit messages state the purpose and value of the change, in Japanese like the existing history. Stage exact paths. No stash.
No secrets in logs, files, prompts or commits. Do not decrypt encrypted data. Do not read or use KANBAN_TOKEN, and do not touch the live server, port 18787, main/kanban.sqlite or the live board.
Do not keep state in volatile system temp directories; use the state directory in the work record. Use only the assigned port, and stop every server you started before reporting.
Respond to the director in Japanese. Herdr: long operations (over 30 seconds expected) use the herdr-watch skill from the start, with no sleep, polling or agent wait. UI editing also follows /home/kino/.agents/skills/stop-describing-ui/SKILL.md.

## Completion
Every card criterion implemented and verified with evidence in the work record, relevant Rust tests and web tests passing, all changes committed on the branch. Worker success alone does not establish completion: the director reviews and accepts.
