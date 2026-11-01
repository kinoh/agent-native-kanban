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

- 2026-11-02: 着手。`GET /tasks` の各タスクへ、単一のバッチ検索で直近の
  `review_result`（本文、投稿者、時刻）を加え、既存のタスク閲覧可否・委譲トークンの
  task scope を通過したカードだけに表示する方針を確認した。ボードには詳細へのリンクを
  伴う小さな専用表示を追加し、本文から合否は導出しない。対象: `src/tasks.rs`、
  `tests/**`、`web/src/**`、`web/tests/**`、この作業記録。次: 現在の API テストの
  ヘルパーとコメント作成経路を確認し、実装と回帰テストを追加する。
- 2026-11-02: 実装と API・ブラウザー回帰テストを追加した。次の検証は Rust 全件テスト、
  `npm ci`、フロントエンド build、指定のブラウザーテスト。30秒超が見込まれる Rust
  全件テストは herdr-watch で起動する。
- 2026-11-02: `npm --prefix web ci` と build は成功。`behavior.py` の初回は、レビューの
  ライブ更新を確認するテストがクエリ付き `/tasks?...` を完全一致で比較して失敗した。
  表示の再取得自体は `expect_response` で確認済みのため、記録判定をクエリ付きパスへ修正し
  て再実行する。
- 2026-11-02: 修正後の `flock ~/.local/state/agent-native-kanban/web-tests.lock python3
  web/tests/behavior.py` は成功。最新レビューの自由形式本文、投稿者、詳細遷移、SSE 後の
  差し替え、レビュー未実施表示を確認した。同じロックで `theme.py` も成功し、light/dark
  両テーマでレビュー表示と未実施表示の可読性を確認した。Rust 全件テストの監視結果待ち。
- 2026-11-02: Rust 全件テストの初回は、新規 API テストが既存の別テストモジュールだけに
  ある非公開ヘルパーを参照してコンパイル失敗。公開済みのテスト API と直接の委譲作成に
  置き換えて修正し、再実行する。
- 2026-11-02: 修正後の `cargo test` は全件成功（新規 `review_results` 2件を含む）。
  `cargo build` と `python3 web/tests/settings_integration.py` も成功。受入条件の根拠:
  - `GET /tasks` は `review_result` の本文・投稿者・時刻を `latest_review_result` として
    一括検索で返し、複数件では時刻（同時刻は ID）降順で直近のみ選ぶ。レビューなしは null。
  - ボードの専用表示は自由形式の本文をそのまま出し、合否を表示しない。投稿者・時刻と
    タスク詳細リンクを表示する。SSE の再読込で更新・未実施表示も切り替わる。
  - API テストは閲覧可能なタスクだけの結果と、task-limited 委譲トークンで範囲外の本文を
    返さないことを確認した。API 側は一覧取得あたり一定回数のバッチ検索で、カード単位の
    リクエストを増やさない。
  - `behavior.py` は共有 lock で成功し、最新・未実施・自由形式・詳細・ライブ更新を確認。
    `theme.py` も同じ lock で成功し light/dark の両テーマを確認。
  実装コミット: `ff64a48`（ボードで直近レビュー結果を確認できるようにする）。判断待ちはない。
