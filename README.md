# Agent-Native Kanban

人間とAIエージェントが対等な作業主体として仕事を発見・取得・遂行・引継ぎするための共有作業管理システム。
振る舞いの仕様は [docs/DRAFT.md](docs/DRAFT.md)、実装で確定した解釈は下の「仕様の解釈」にある。

- サーバー: Rust（axum）、SQLite を組み込みで使う単一プロセス
- Web UI: React + Vite。ビルド結果をサーバーが配信する

## 起動

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

## 認証

| 主体 | 方式 |
|---|---|
| 人間 | メールアドレスとパスワードでログインし、Cookie セッションを使う |
| エージェント | `Authorization: Bearer ank_...`。ブラウザや人間のセッションを必要としない |

管理者が人間を1人ずつ登録する代わりに、**有効期間付きのサインアップ URL** を発行して配る。有効期間は60秒〜30日、利用回数の上限が必須で、URL に登録後の権限を結び付ける。URL とトークンの秘密は発行時に1度だけ表示され、DB にはハッシュだけを保存する。

パスワードを忘れた場合、管理者が「組織の管理」から再設定する（メール送信はしない）。

## API

ベースパスは `/api/v1`。タスクは ID でも `DEV-12` 形式のキーでも指定できる。

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
| 認証 | `POST /auth/login`、`POST /auth/logout`、`GET・POST /signup/{token}` |
| 自分 | `GET /me`、`PUT /me/password`、`GET /me/inbox`（自分宛の質問・レビュー依頼） |
| 主体 | `GET・POST /principals`（POST はエージェントの登録）、`GET・PATCH・DELETE /principals/{id}`、`PUT /principals/{id}/password` |
| 権限 | `GET・POST /principals/{id}/grants`、`DELETE /grants/{id}` |
| トークン | `GET・POST /principals/{id}/tokens`、`DELETE /tokens/{id}` |
| サインアップ URL | `GET・POST /signup-links`、`DELETE /signup-links/{id}` |
| プロジェクト | `GET・POST /projects`、`GET・PATCH /projects/{id}` |
| ワークフロー | `POST /projects/{id}/stages`、`PATCH・DELETE /stages/{id}`、`PUT /projects/{id}/transition-rules` |
| タスク | `GET・POST /tasks`、`GET・PATCH /tasks/{task}`、`GET /tasks/{task}/context`、`GET /tasks/{task}/history` |
| 業務段階 | `POST /tasks/{task}/stage` |
| 作業（占有者） | `POST /tasks/{task}/work/{claim,start,pause,resume,release,transfer,complete}` |
| 作業（編集権限） | `POST /tasks/{task}/work/{reopen,hold,unhold}` |
| 作業（管理権限） | `POST /tasks/{task}/work/{revoke,reassign}` |
| 記録 | `POST /tasks/{task}/comments`、`POST /tasks/{task}/artifacts`、`POST /tasks/{task}/files?role=&name=`、`GET /artifacts/{id}/content` |
| 関係 | `POST /tasks/{task}/dependencies`、`DELETE /tasks/{task}/dependencies/{other}`、`POST /tasks/{task}/links`、`DELETE /tasks/{task}/links/{other}` |
| 変更イベント | `GET /events?after=<seq|latest>`、`GET /events/stream?after=<seq|latest>`（SSE） |

イベントは `seq` の昇順で欠けなく読める。応答の `cursor` を次の `after` に渡す。新しい仕事は `work.available` で検出する。SSE は `Last-Event-ID` で再開でき、認証情報が失効すると接続を閉じる。

## 権限

`view` `create` `edit` `claim` `assign` `review` `approve` `admin` を、組織全体またはプロジェクト単位で与える。

- `admin` はそのスコープの全権限を含む。組織全体の `admin` が組織管理者
- プロジェクトに何らかの権限があれば閲覧できる
- 進捗・コメント・成果物の記録には `edit` `claim` `review` `approve` のいずれかが必要
- 同じ組織の主体の表示名は、組織の全主体が見られる（担当の選択に必要）。メールアドレスは管理者と本人だけ

## 仕様の解釈

- **作業はタスク1件につき占有1つ**。占有・活動は `claims`、作業準備は `tasks.work_state` に持つ。並行作業は子タスクに分割して表現する
- **業務段階の変更は作業状態を初期化する**。新しい段階の作業が `open` から始まる。占有が残っているタスクは段階を変更できず、`task_occupied` を返す
- **段階の設定**: `requires_work` が偽の段階（バックログや完了など）には取得する作業がない。`claim_permission` は取得に必要な権限で、レビュー段階なら `review` にする
- **依存の完了**は、依存先が `is_final` の段階に到達したこと。段階名からは判定しない
- **遷移ルール**: ルールのない遷移は `edit` 権限で実行できる。ルールがあれば指定の権限（`edit` の代わり）と作業完了の条件を求める
- **作業権**: 取得ごとに新しい `claim_id` を発行する。占有者の交代後、古い `claim_id` の操作は `stale_claim` で拒否する
- **占有の期限切れはない**。作業状態は報告された状態なので、ハートビートによる自動解放はしない。放棄された作業は管理者が `revoke`・`reassign` で解除する。占有者が無効・削除になっても、解除されるまで取得可能にしない
- **主体の削除**は履歴を残す論理削除。認証情報・権限・メールアドレスは消し、ID と表示名、作業記録、担当、占有は残す
- **メールアドレスはサーバー全体で一意**で、人間は1つの組織にだけ所属する
