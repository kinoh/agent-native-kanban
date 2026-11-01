# Independent review of KAN-11 to KAN-16

User instruction 「レビュアーも別に立てた方が良いぞ」 supersedes the brief where it says the director reviews: each branch gets a reviewer that did not write it. The director still decides acceptance, integrates and talks to the user.

Role: reviewer; Codex session on the configured default model, one per branch, started when the worker reports. A reviewer has no history with the worker. It does not launch or direct other agents.
Target: the assigned branch, as the diff from its merge base with main, in a detached worktree at the reported commit. Do not edit, commit to or check out the worker's branch.
Read: cards.json and brief.md in this directory, then the work record on the branch. Treat the record as a claim to check, not as evidence.
Check: every acceptance criterion against the code and by running it yourself; visibility and authorization, including task-limited delegated tokens and hidden tasks; tests that assert needed behaviour and would fail without the change; errors, logs, implicit fallbacks and unapproved defaults; scope beyond the card; both themes and real browser behaviour for UI cards. For KAN-12, try the card's sequence from the guide text alone, without reading source.
Report: Japanese, findings first, most severe first, each with file:line, the concrete failing scenario, and whether it blocks. State what you ran and its result, and what you did not check. If nothing blocks, say so plainly.
Constraints: same as brief.md: no KANBAN_TOKEN, live server, port 18787 or main/kanban.sqlite; the assigned port only; state under ~/.local/state/agent-native-kanban/review-<card>/; stop every server you start; web tests run under flock ~/.local/state/agent-native-kanban/web-tests.lock; herdr-watch for anything over 30 seconds.
Ports: kan-11-14 18831, kan-12 18832, kan-13 18833, kan-15 18834, kan-16 18835.
Flow: the director sends blocking findings back to the worker, then returns the fixed commit to the same reviewer.
