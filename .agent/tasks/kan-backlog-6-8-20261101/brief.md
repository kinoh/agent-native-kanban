# KAN-6, KAN-7, KAN-8 implementation

## User requirements and authorization
User requested: 「整理して作業可能にしましょう（KAN-8は実装item化してもOK）」 then 「それぞれsubagentに分担して、worktree上で作業させましょう」.
Implement each prepared card with one independent worker in its own Git worktree through Herdr. Card text is preserved in cards.json and on the live board at http://127.0.0.1:18787/projects/KAN/board. Priority medium: KAN-6, KAN-8; low: KAN-7.
Merging into main, restarting the live server and completing live cards are not yet authorized by the wording above; the director decides them with the user after review.

## Roles
Director: the user-facing Claude session in Herdr pane wK:pK. It owns user communication, assignment boundaries, live Kanban mutations, review, integration and acceptance. Where the agent-coordination references say Astra, read this director.
Workers: Codex sessions, explicit model gpt-5.6-terra. They implement and verify on their own branch only. They may not spawn or direct other agents.

## Observed facts
Repository /home/kino/src/agent-native-kanban is a bare repository with worktrees beside it; main is at /home/kino/src/agent-native-kanban/main. No Git remote. Rust/axum/SQLite backend, React/Vite frontend. Live server listens on 127.0.0.1:18787 from main/kanban.sqlite.

## Integration contract
- Each card has its own branch and worktree, based on the same commit. Each worker commits only on its own branch and never merges, rebases, amends or force-updates.
- The director integrates after review with merge commits, in the order KAN-6, KAN-8, KAN-7, and resolves conflicts then. Expected overlap: src/api/mod.rs, src/api/guide.rs, README.md (KAN-6/KAN-8); web/src/App.tsx (KAN-7/KAN-8); web/src/styles.css (all).
- KAN-6 and KAN-8: new CSS uses the existing CSS variables only and adds no direct colour values, so that KAN-7 needs no follow-up rewrite. KAN-7 owns the colour variables. KAN-6 and KAN-8 must not rename or remove existing variables or refactor unrelated CSS.
- Only KAN-8 adds a migration (0004). Only KAN-8 changes src/api/feed.rs event visibility. KAN-6 does not change it.
- Keep each change limited to its card so that the merges stay small.

## Shared constraints
Read actual code and existing documents first. Preserve agreed scope and the decisions in the card; no unapproved workaround, implicit fallback, unapproved default value, unnecessary dependency or unrelated feature. Design explicit errors and useful audit/debug logs from the start. Tests describe the behaviour the product needs, not incidental behaviour.
Commit each unit of work before stopping or handing off; no uncommitted changes at the end. Never amend, rebase or force-push. Commit messages state the purpose and value of the change. Stage exact paths. No stash.
No secrets in logs, files, prompts or commits. Do not decrypt encrypted data. Do not read or use KANBAN_TOKEN, and do not touch the live server, port 18787, main/kanban.sqlite or the live board.
Do not keep state in volatile system temp directories; use the state directory in the work record. Use only the assigned port, and stop every server you started before reporting.
Respond to the director in Japanese. Herdr: long operations (over 30 seconds expected) use the herdr-watch skill from the start, with no sleep, polling or agent wait.

## Completion
Every card criterion implemented and verified with evidence in the work record, relevant Rust tests and web tests passing, all changes committed on the branch. Worker success alone does not establish completion: the director reviews and accepts.
