# Status

Last updated: 2026-11-02 JST. Director: wK:pK. State: IMPLEMENTING.

Director moved all six cards to 実装 and claimed/started them as the existing agent principal. Claim IDs are work references, not authentication tokens. Workers do not mutate the live board. Do not claim/start again.

| Card | claim_id |
|---|---|
| KAN-11 | 01a197ec-a343-7266-80e2-25c0d4ed4ce4 |
| KAN-12 | 01a197ec-a352-7001-b1d0-ccbdd1d688cb |
| KAN-13 | 01a197ec-a362-72d3-a6c4-1cdebc3df009 |
| KAN-14 | 01a197ec-a372-7547-8c3e-fb16b3fb2811 |
| KAN-15 | 01a197ec-a383-71e7-a71b-bd0f1a6fe024 |
| KAN-16 | 01a197ec-a393-7280-9f0d-041eab818c39 |

| Worker | Model | Pane | Branch / worktree | Record | Port |
|---|---|---|---|---|---|
| kan-11-14 | gpt-5.6-terra | wR:p1 | kan-11-14-admin-tree, ../kan-11-14 | work/kan-11-14.md | 18821 |
| kan-12 | gpt-5.6-terra | wS:p1 | kan-12-guide, ../kan-12 | work/kan-12.md | 18822 |
| kan-13 | gpt-5.6-terra | wT:p1 | kan-13-occupant-reason, ../kan-13 | work/kan-13.md | 18823 |
| kan-15 | gpt-5.6-terra | wV:p1 | kan-15-blocker-card, ../kan-15 | work/kan-15.md | 18824 |
| kan-16 | gpt-5.6-terra | wW:p1 | kan-16-review-card, ../kan-16 | work/kan-16.md | 18825 |

Workers run with KANBAN_TOKEN removed from their environment.
Director working files: ~/.local/state/agent-native-kanban/backlog-11-16/.

## Board rule
The board must match reality. When a worker reports, the director runs work/complete and moves the card to レビュー without asking the user. Moving to 完了 still needs the user.
The director does not claim review work and does not assign reviewers: a reviewer is a principal that takes cards from the レビュー column itself (user correction). Issuing its token needs the user's decision, because the kanban skill forbids issuing tokens; this is pending. KAN-15 is the exception already running, and the claim below is held by the director until that is decided. Implementation claims held by the director for the workers have the same fault.

Review claims: KAN-15 01a197f2-671e-7272-ab2c-8f861c5e8bf0

## Remaining
Workers are delegated. When a worker reports, start its independent reviewer as in reviewer.md, return blocking findings to the worker, re-review, then ask the user about merging, the review stage and deployment.
