# Agent-Native Kanban

人間とAIエージェントが対等な作業主体として仕事を発見・取得・遂行・引継ぎするための共有作業管理システム。
振る舞いの仕様は [docs/DRAFT.md](docs/DRAFT.md)、実装で確定した解釈は下の「仕様の解釈」にある。

- サーバー: Rust（axum）、SQLite を組み込みで使う単一プロセス
- Web UI: React + Vite。ビルド結果をサーバーが配信する

## 起動

Rust（Cargo）、Node.js（npm）、Task（go-task）を用意して実行する。

```bash
task run
```

Web の依存インストール、Web とサーバーの release ビルド、サーバー起動まで実行する。
`kanban.sqlite` が存在しない初回は、組織 `Acme` を作成し、24時間有効・1回限りの管理者登録 URL を端末に表示する。
起動後にその URL を開いて登録する。通常のアクセス先は `http://127.0.0.1:18787`。停止は Ctrl+C。
2回目以降は既存の DB を使う。ビルドだけなら `task build`。

DB、待受アドレス、公開 URL、初回の組織名は指定できる。`PUBLIC_URL` の既定値は `http://` と `LISTEN` をつないだ値。

```bash
task run DB=local.sqlite LISTEN=127.0.0.1:18788 PUBLIC_URL=http://localhost:18788 ORG=Example
```

個別にビルド・初期化・起動する場合は次の手順を使う。

```bash
(cd web && npm ci && npm run build)
cargo build --release

# 組織を作り、最初の管理者用のサインアップ URL を表示する
./target/release/agent-native-kanban bootstrap \
  --db kanban.sqlite --public-url http://127.0.0.1:18787 --org Acme --expires-hours 24

RUST_LOG=info ./target/release/agent-native-kanban serve \
  --db kanban.sqlite --listen 127.0.0.1:18787 \
  --public-url http://127.0.0.1:18787 --web-dist web/dist
```

全ての引数が必須。`--public-url` はサインアップ URL の生成と Cookie 認証の発信元検証に使う。`https://` なら Cookie に `Secure` を付ける。
TLS は持たないので、公開するときは TLS 終端のリバースプロキシの後ろに置く。

UI の開発時は `cd web && npm run dev` で Vite を起動する。`/api` は `127.0.0.1:18787` へ中継されるので、サーバーの `--public-url` を Vite の URL にする。

## テスト

```bash
cargo test
(cd web && npm run build)
```

`tests/scenarios.rs` が仕様9章の受け入れシナリオ A〜D に対応する。

Webの回帰検証は、PythonのPlaywrightとChromiumを用意した環境で、ビルド後に `python3 web/tests/behavior.py` を実行する。API応答と変更通知を固定し、501件の一覧、外部から追加した段階、検索の競合、途中ページの失敗、委譲トークンの表示と失効をブラウザで確認する。実行用の一時領域と結果は `~/.local/state/agent-native-kanban-ui-tests/` に保存する。

## 認証

| 主体 | 方式 |
|---|---|
| 人間 | メールアドレスとパスワードでログインし、Cookie セッションを使う |
| エージェント | `Authorization: Bearer ank_...`。ブラウザや人間のセッションを必要としない |

管理者が人間を1人ずつ登録する代わりに、**有効期間付きのサインアップ URL** を発行して配る。有効期間は60秒〜30日、利用回数の上限が必須で、URL に登録後の権限を結び付ける。URL とトークンの秘密は発行時に1度だけ表示され、DB にはハッシュだけを保存する。

パスワードを忘れた場合、管理者が「組織の管理」から再設定する（メール送信はしない）。

## API

ベースパスは `/api/v1`。タスクは ID でも `DEV-12` 形式のキーでも指定できる。

認証前のエージェント向けには `GET /api/v1/guide` が UTF-8 Markdown の利用ガイドを返す。トークンの入手・認証から、仕事の検索、取得、記録、完了、変更イベントまでの実行例はここから読める。

失敗は次の形式で返す。

```json
{ "error": { "code": "not_claimable", "message": "...", "details": { "reasons": [] } } }
```

| ステータス | 意味 |
|---|---|
| 401 | 未認証・認証情報の失効 |
| 403 | 権限不足（`details.required_permission`） |
| 404 | 存在しない、または閲覧権限がない（区別しない） |
| 409 | 競合。`not_claimable`、`stale_claim`、`version_conflict`、`task_occupied` など |
| 422 | リクエストの不備 |

### 仕事の発見から完了まで

```bash
H='Authorization: Bearer ank_...'; A=http://127.0.0.1:18787/api/v1
J='Content-Type: application/json'

# 自分が取得できる仕事を探す（priority, assignee, readiness, due_before, q なども指定可）
curl -H "$H" "$A/tasks?can_claim=true&sort=priority"

# DEV-12 に直接依存するタスクを探す（ID でも指定可。完了済みの依存も含む）
curl -H "$H" "$A/tasks?depends_on=DEV-12"

# 文脈をまとめて読む（依存、コメント、成果物、履歴）
curl -H "$H" "$A/tasks/DEV-12/context"

# 取得する。応答の work.claim_id が作業権で、以降の操作で提示する
curl -H "$H" -X POST "$A/tasks/DEV-12/work/claim"

curl -H "$H" -H "$J" -d '{"claim_id":"..."}' "$A/tasks/DEV-12/work/start"
curl -H "$H" -H "$J" -d '{"kind":"progress","body":"..."}' "$A/tasks/DEV-12/comments"
curl -H "$H" -H "$J" -d '{"claim_id":"...","summary":"...","evidence":[{"name":"PR","url":"https://..."}]}' \
  "$A/tasks/DEV-12/work/complete"
```

### 主なエンドポイント

| 用途 | エンドポイント |
|---|---|
| 利用ガイド | `GET /guide`（認証不要、UTF-8 Markdown） |
| 認証 | `POST /auth/login`、`POST /auth/logout`、`GET・POST /signup/{token}` |
| 自分 | `GET・PATCH /me`、`PUT /me/password`、`GET /me/inbox`（自分宛の質問・レビュー依頼） |
| 組織 | `PATCH /organization`（組織管理者による自組織名の変更） |
| 委譲 | `GET・POST /me/delegations`（認証に使ったトークンから発行した委譲トークン） |
| 主体 | `GET・POST /principals`（POST はエージェントの登録）、`GET・PATCH・DELETE /principals/{id}`、`PUT /principals/{id}/password` |
| 権限 | `GET・POST /principals/{id}/grants`、`DELETE /grants/{id}` |
| トークン | `GET・POST /principals/{id}/tokens`、`DELETE /tokens/{id}` |
| サインアップ URL | `GET・POST /signup-links`、`DELETE /signup-links/{id}` |
| プロジェクト | `GET・POST /projects`、`GET・PATCH /projects/{id}` |
| ワークフロー | `POST /projects/{id}/stages`、`PATCH・DELETE /stages/{id}`、`PUT /projects/{id}/transition-rules` |
| リソース | `GET・POST /resources`、`GET・PATCH・DELETE /resources/{id}`、`POST /resources/{id}/{acquire,release,revoke}` |
| タスク | `GET・POST /tasks`、`GET・PATCH /tasks/{task}`、`GET /tasks/{task}/context`、`GET /tasks/{task}/history` |
| 業務段階 | `POST /tasks/{task}/stage` |
| 作業（占有者） | `POST /tasks/{task}/work/{claim,start,pause,resume,release,transfer,complete}` |
| 作業（編集権限） | `POST /tasks/{task}/work/{reopen,hold,unhold}` |
| 作業（管理権限） | `POST /tasks/{task}/work/{revoke,reassign}` |
| 記録 | `POST /tasks/{task}/comments`、`POST /tasks/{task}/artifacts`、`POST /tasks/{task}/files?role=&name=`、`GET /artifacts/{id}/content` |
| 関係 | `POST /tasks/{task}/dependencies`、`DELETE /tasks/{task}/dependencies/{other}`、`POST /tasks/{task}/links`、`DELETE /tasks/{task}/links/{other}` |
| 変更イベント | `GET /events?after=<seq|latest>`、`GET /events/stream?after=<seq|latest>`（SSE） |

イベントは `seq` の昇順で欠けなく読める。応答の `cursor` を次の `after` に渡す。新しい仕事は `work.available` で検出する。SSE は `Last-Event-ID` で再開でき、認証情報が失効すると接続を閉じる。

### 共有リソース

リソースは組織またはプロジェクトに属する、上限付きの共有枠。`capacity: 1` は mutex、2以上は semaphore として使える。作成時に所属を決め、その後は変更できない。同じ所属内の名前は一意。

組織リソースは組織の全主体が閲覧でき、いずれかのスコープで `claim` を持つ主体が確保できる。プロジェクトリソースはプロジェクト全体を閲覧できる主体に見え、同プロジェクトの `claim` で確保できる。作成・編集・削除・解除は、組織リソースなら組織管理者、プロジェクトリソースならプロジェクトの `admin` が行う。タスク限定の委譲トークンは、リソースを閲覧・確保できない。

`POST /resources/{id}/acquire` は `{ "reason":"…" }`（前後空白を除き1〜500文字）を受け、成功時に `hold_id` を返す。確保者だけが `POST /resources/{id}/release` でその `hold_id` を解放できる。管理者は理由を添えて `revoke` できる。満杯なら `409 resource_busy` と現在の `details.holders` を返す。待ち行列、期限、自動解放はない。確保が残るリソースの削除と、確保数より小さい上限への変更は `409`。理由は閲覧できる主体全員とイベント購読者に見えるため、秘密を書かない。

作成・変更・削除・確保・解放・解除は `resource.*` イベントとして記録され、操作主体・リソース・理由・委譲トークンを追跡できる。組織リソースのイベントは、タスク限定でない同組織の主体へ配信する。解放イベントを購読してから再度 acquire する。

## 権限

`view` `create` `edit` `claim` `assign` `review` `approve` `admin` を、組織全体またはプロジェクト単位で与える。

- `admin` はそのスコープの全権限を含む。組織全体の `admin` が組織管理者
- プロジェクトに何らかの権限があれば閲覧できる
- 進捗・コメント・成果物の記録には `edit` `claim` `review` `approve` のいずれかが必要
- 同じ組織の主体の表示名は、組織の全主体が見られる（担当の選択に必要）。メールアドレスは管理者と本人だけ

### 権限の委譲

サブエージェントには、自分のトークンから権限を絞った委譲トークンを渡す。主体は増えず、組織管理者権限も要らない。

```bash
# DEV-12 とその子孫だけで claim を使える、1時間のトークンを発行する。token は1度だけ返る
curl -H "$H" -H "$J" -d '{
  "name": "implementer",
  "expires_at": "2026-11-01T12:00:00Z",
  "scopes": [{ "project_id": "...", "permission": "claim" }],
  "tasks": ["DEV-12"]
}' "$A/me/delegations"

# 使い終わったら発行元のトークンか委譲トークン自身で失効させる
curl -H "$H" -X DELETE "$A/tokens/<id>"
```

- `scopes` は必須で、委譲元が持つ範囲だけを指定できる。`review` `approve` は渡せるが、`admin` は渡せない
- `tasks` を省くとプロジェクト全体になる。指定すると、そのタスクと子孫だけを読み書きでき、範囲外は存在しない扱い（404）になる。プロジェクトの段階や遷移ルールは見える。`scopes` のプロジェクトと過不足なく対応させる
- 実効権限はリクエストごとに、主体の現在の権限と親までの全ての委譲を重ねて判定する。親の失効・期限切れや主体の無効化は子孫全てに及ぶ
- `expires_at` は必須で、親の期限を超えられない。長さの上限はない。委譲トークンからの再委譲はさらに絞る場合に限り、深さの制限なくできる
- 委譲元はトークンに限る（Cookie セッションからは発行できない）
- 委譲トークンは通常のトークンの発行・一覧、パスワード変更には使えない（`403 delegated_token_restricted`）
- 親が取得した `claim_id` は、現段階の取得権限を持つ委譲トークンなら引き継げる。主体が同じなので、同じタスクを見られる委譲トークンどうしは作業権を区別しない
- タスクを限定したトークンは、範囲内のタスクの子だけを作れる（`403 outside_delegated_tasks`）
- 操作の主体は委譲元のまま記録し、イベントの `delegated_token_id` でどの委譲トークンの操作かを追える。発行は `token.delegated` イベントに範囲ごと残る

Web UI では発行しない。組織管理者は「組織の管理」の「権限」から、主体のトークンと委譲の範囲、親まで含めた実際の状態を確認して失効できる。タスクの履歴には委譲トークンによる操作を ID の末尾8文字で示す。

独立したレビューや承認が必要なら、遷移ルールの必要権限を外部の主体だけに与える。委譲はその保証を目的としない。

## 仕様の解釈

- **作業はタスク1件につき占有1つ**。占有・活動は `claims`、作業準備は `tasks.work_state` に持つ。並行作業は子タスクに分割して表現する
- **業務段階の変更は作業状態を初期化する**。新しい段階の作業が `open` から始まる。占有が残っているタスクは段階を変更できず、`task_occupied` を返す
- **段階の設定**: `requires_work` が偽の段階（バックログや完了など）には取得する作業がない。`claim_permission` は取得に必要な権限で、レビュー段階なら `review` にする
- **依存の完了**は、依存先が `is_final` の段階に到達したこと。段階名からは判定しない
- **プロジェクトの境界**: 依存・関連・親子関係は同一プロジェクト内に限定する。横断指定は `422 relation_project_mismatch`。閲覧できない参照先は従来通り404を返す。APIとDBの両方で制約を検証する
- **関係の履歴**: 過去の依存・関連・親変更イベントも参照先への現在の閲覧権限を確認する。参照先を閲覧できないイベントは文脈・履歴・変更フィード（SSEを含む）から除外し、保存済みの記録は維持する
- **遷移ルールの履歴**: `workflow.rules_replaced` の `from`・`to` に変更前後のルール一覧を記録する（段階ID、必要権限、作業完了条件を含む）
- **遷移ルール**: ルールのない遷移は `edit` 権限で実行できる。ルールがあれば指定の権限（`edit` の代わり）と作業完了の条件を求める
- **作業権**: 取得ごとに新しい `claim_id` を発行する。占有者の交代後、古い `claim_id` の操作は `stale_claim` で拒否する
- **占有の期限切れはない**。作業状態は報告された状態なので、ハートビートによる自動解放はしない。放棄された作業は管理者が `revoke`・`reassign` で解除する。占有者が無効・削除になっても、解除されるまで取得可能にしない
- **主体の削除**は履歴を残す論理削除。認証情報・権限・メールアドレスは消し、ID と表示名、作業記録、担当、占有は残す
- **リソース確保**はタスクの占有とは別の、明示的に解放する共有枠。主体の無効化・削除、タスクの完了では解放しない。放置された確保は管理者が理由を残して解除する
- **メールアドレスはサーバー全体で一意**で、人間は1つの組織にだけ所属する

## 既存DBの更新

スキーマ2への移行は、既存のプロジェクト横断関係を検出すると対象の種類・タスクIDをログと起動エラーに出して停止する。データを自動削除したり、依存を無効化して作業を取得可能にしたりはしない。

更新前に旧バージョンで、対象を閲覧・編集できる主体が関係を明示的に解消する。依存は `DELETE /tasks/{task}/dependencies/{other}`、関連は `DELETE /tasks/{task}/links/{other}`、親子は子タスクに `PATCH /tasks/{task}` で `parent: null` と現在の `version` を送る。その後、新バージョンで起動し直す。既に解消した関係の履歴は削除されず、上記の閲覧制御が適用される。
