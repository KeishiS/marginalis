# 実環境での受入確認

この手順は、NixOS VM テストやユニットテストとは別に、実際のリバースプロキシ・Kanidm・
MCP クライアントを通して行う確認です。

リリース内容に関係する確認項目を選び、結果と実行日時を記録してください。API の仕様は
[REST API リファレンス](rest-api.md)、MCP の仕様は [MCP と OAuth](mcp.md)を参照します。

パスワード、Cookie、OIDC コード、MCP のアクセストークン・リフレッシュトークンを、コマンド
履歴・Issue・ログ・画面共有に残さないでください。API クライアントのシークレットストアか
一時的な Cookie jar を使い、確認後に削除します。

## v0.2.0 系列へ初めて更新する場合

v0.2.0 系列では、データフォーマット v1 を AdocWeave v0.6.1 前提で破壊的に再定義します。
以前の v1 と新しい v1 は `FORMAT` マーカーだけでは区別できません。次の準備を終えるまで
新しいバイナリを起動してはいけません。

1. 本体サービスと監査削除タイマーを停止し、投影再構築、監査削除、バックアップの各
   oneshot が動作していないことを確認する。設定中の `services.marginalis.dataDir` と
   `databaseUrl` も確認する。
2. 必要なら旧 `dataDir` をオフラインの退避物として別の場所へ保存する。この退避物と
   旧バックアップは、v0.2.0 系列への復元入力として使用しない。
3. 削除対象が設定値と一致することを再確認し、旧 `dataDir` 全体を運用者が明示して削除する。
   SQLite が `dataDir` の外にある場合は DB と `-wal`・`-shm` も削除するか、新しい空の
   `databaseUrl` へ変更する。
4. v0.2.0 系列へ更新し、空の `dataDir` から `root` 資格情報、登録ポリシー、OIDC 利用者、
   ACL、ノートを初期化する。

アプリケーションと NixOS モジュールは、この削除を自動実行しません。具体的な NixOS 手順は
[NixOS での運用](nixos.md#v020-系列へ初めて更新する)を参照してください。

## 事前条件

1. `GET /api/v1/health` が `200` を、`GET /api/v1/readiness` が OIDC `available` として
   `200` を返す。
2. `services.marginalis.mcp.enable = true` とする場合、リバースプロキシが `/mcp`・
   `/oauth/`・`/.well-known/` を同じオリジンへ転送している。
3. 登録ポリシーが `approval` の場合、root が対象の OIDC ユーザーを有効化済みである。
   root のログインと承認手順は
   [保留 OIDC ユーザーの承認](nixos.md#保留-oidc-ユーザーの承認)を参照し、
   [REST API リファレンス](rest-api.md#root-管理)の CSRF 要件に従う。

## REST API の確認

1. ブラウザーで `/auth/oidc/login` へ移動し、Kanidm ログイン後に `GET /api/v1/session` が
   `200` かつ `is_root: false` を返すことを確認します。
2. Cookie jar と CSRF Cookie を扱える API クライアントで、`POST /api/v1/notes` へ有効な
   AsciiDoc 正本を送ります。必須のヘッダー属性は含めますが、`note-id`・`creator-id`・
   `created-at`・`updated-at` の値はサーバーが置き換えます。`201` と `Location` を記録し、
   取得した正本がサーバー値になっていることを確認します。
3. 正本取得時の `ETag` を `If-Match` と `X-CSRF-Token` に付けて `PUT` します。`204` の後、
   `GET /api/v1/search?q=<固有語>` が作成したノートだけを返すことを確認します。
4. 同じ `ETag` でもう一度更新して `409` になることを確認します。最新の `ETag` で削除準備を
   行い、返された確認トークンで削除を確定して `204` になることを確認します。削除後は正本
   取得が `404` になり、一覧・検索の `200` 応答に対象ノートが含まれないことを確認します。
5. `xref:note:<UUID>[表示名]`、インラインとブロックの LaTeX 数式、許可された
   `[source,rust]` を含む正本を保存できることを確認します。
6. `include`、パススルー、`javascript:` URL、未許可のソースコード言語をそれぞれ含む正本が
   `422 validation-failed` で拒否されることを確認します。

### 最小 Web UI による手動確認

OIDC ログイン後に `/acceptance` を開くと、JavaScript を使わずに閲覧可能なノートの一覧と、
作成・取得・更新・検索・削除を順に確認できます。一覧は ACL を適用したタイトル順の先頭
100 件です。この画面は同一オリジンの HTML フォームで CSRF トークンを送り、表示には
同一オリジンの静的 CSS だけを使います。CSP は `default-src 'none'`、`form-action 'self'`、
`frame-ancestors 'none'`、`style-src 'self'` で、スクリプトを許可しません。ただし、HTTP
ステータスやヘッダーを含む REST 仕様そのものの確認は、この画面ではなく次の外部 API
クライアント手順で行います。

### 外部 API クライアントによる確認例

Cookie jar と CSRF Cookie を保持できる外部 API クライアントを使います。クライアント自身の
ブラウザーログインで OIDC セッションを確立し、同じ Cookie jar で次のリクエストを実行します。
クライアントが `marginalis_csrf` Cookie を `X-CSRF-Token` へ展開できない場合は、REST 受入の
自動化対象から外し、Issue 030 の E2E 基盤で扱います。

| 順序 | リクエスト | 必須設定 | 期待結果 |
| --- | --- | --- | --- |
| 1 | `POST /api/v1/notes` | UTF-8 AsciiDoc ボディ、`Content-Type: text/plain; charset=utf-8`、`X-CSRF-Token` | `201`、`Location` |
| 2 | `GET {Location}` | Cookie jar | `200`、`ETag` |
| 3 | `PUT {Location}` | 更新済みボディ、`If-Match: <ETag>`、`X-CSRF-Token` | `204` |
| 4 | 同じ `If-Match` で再度 `PUT` | 同上 | `409` |
| 5 | `GET /api/v1/search?q=<固有語>` | Cookie jar | `200`、作成ノートだけを含む |
| 6 | `POST /api/v1/notes/{note_id}/delete-preparations` | `If-Match: <最新 ETag>`、`X-CSRF-Token` | `200`、`confirmation_token` |
| 7 | `POST /api/v1/notes/delete-confirmations` | JSON ボディ `{"confirmation_token":"..."}`、`X-CSRF-Token` | `204` |

## MCP クライアントの確認

1. クライアントの Streamable HTTP エンドポイントを `https://marginalis.sandi05.com/mcp` に
   設定します。
2. Protected Resource Metadata から OAuth 認可エンドポイントへ進み、Kanidm ログインの後、
   一般ユーザーとしてスコープを確認して許可します。root セッションでは MCP クライアントを
   認可できません。
3. `search_notes`・`create_note`・`get_note`・`update_note`・`prepare_delete_note`・
   `delete_note` を順に実行します。REST で作ったノートが MCP 検索でも、MCP で作ったノートが
   REST 検索でも、同じ ACL 判定の結果になることを確認します。
4. REST の `DELETE /api/v1/mcp-authorizations?client_id=...` で認可を取り消し、既存の
   アクセストークンによる `/mcp` が `401` になることを確認します。

未知のクライアントを使う場合は、Client ID Metadata Document のホストを
`clientMetadataAllowedHosts` に追加します。メタデータを持たないクライアントは、root が
事前登録してから試してください。

## 運用機能の確認

1. root 監査ログを読み取り専用で確認します。次の例は `databaseUrl` が既定値の場合です。

   ```sh
   sudo -u marginalis sqlite3 /var/lib/marginalis/marginalis.sqlite \
     'SELECT action, occurred_at_ms FROM root_audit_log ORDER BY audit_id DESC LIMIT 100;'
   ```

2. 永続バックアップの保存先と保持世代を決めて `backupDirectory` を設定します。
   `marginalis-backup.service` の実行後、新しい世代に `FORMAT`・`MANIFEST`・`COMPLETE`・
   `marginalis.sqlite`・`notes/` が揃っていることを確認します。
3. 本番の `dataDir` は切り替えずに、v0.2.0 系列の新しい v1 で作成した世代だけを使って
   候補コミットの作業ツリーから
   `nix run . -- restore --input <世代> --output <新しい絶対パス>`
   を実行します。`RESTORED` マーカー・SQLite・正本が作られることを確認します。実際の
   切り替えは、旧データの保管場所とロールバック手順を決めてから行います。
4. `marginalis-rebuild-projections.service` を実行し、本体サービスを再起動して
   health / readiness を再確認します。
5. `systemctl status marginalis-prune-audit.timer` で、監査ログの 365 日保持と期限切れ認証
   データの掃除を行うタイマーが有効であることを確認します。
6. `curl -fsS https://marginalis.sandi05.com/api/v1/openapi.json | jq -e '.openapi == "3.1.0"'`
   で、実行中のバイナリが公開仕様を返すことを確認します。`/api/v1` は v0.1.0 から互換性を
   保つため、リポジトリの `docs/openapi.json` と一致していることを確認します。

v0.1.0 で公開した API に破壊的変更が必要になった場合は、新しいバージョンパスを
追加し、既存バージョンには非推奨告知・移行手順・少なくとも 1 リリース周期の猶予を設けます。

いずれの確認でも、失敗時には応答の `X-Request-Id` と
`journalctl -u marginalis.service -b --no-pager` を対応付けて調査します。
