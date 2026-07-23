# 実環境受入確認

この手順は、NixOS VM・unit testとは別に、実際のreverse proxy、KanidmおよびMCP clientを通す確認である。
password、Cookie、OIDC code、MCP access/refresh tokenをコマンド履歴、issue、ログまたは共有画面に
貼り付けない。

## 事前条件

1. `GET /api/v1/health`が`200`、`GET /api/v1/readiness`がOIDC `available`として`200`を返す。
2. `services.marginalis.mcp.enable = true`とする場合、reverse proxyが`/mcp`、`/oauth/`および
   `/.well-known/`を同じoriginへ転送する。
3. `approval`登録policyの場合、rootが対象OIDC userを有効化済みである。root操作は
   [REST API](rest-api.md#root管理)のCSRF要件に従う。

## 段階1: OIDC利用者によるREST

1. ブラウザで`/auth/oidc/login`へ移動し、Kanidm login後に`GET /api/v1/session`が`200`かつ
   `is_root: false`を返すことを確認する。
2. Cookie jarとCSRF cookieを管理できるAPI clientで、`POST /api/v1/notes`へ有効なAsciiDoc正本を送る。
   `creator-id`には前段のsession user ID、`note-id`には新しいUUIDv7を指定する。`201`とsource URLを
   記録する。
3. `GET` sourceの`ETag`を取得し、その値を`If-Match`と`X-CSRF-Token`に付けて`PUT`する。`204`後に
   `GET /api/v1/search?q=<固有語>`が作成ノートだけを返すことを確認する。
4. 同じ`ETag`で二度更新して`409`となること、更新後の`ETag`で`DELETE`すると`204`となることを確認する。
   削除後は一覧・検索・source取得がいずれも`404`であることを確認する。

`marginalis_session`や`marginalis_csrf`をshellの引数・履歴へ貼り付けない。API clientのsecret storeまたは
一時的なCookie jarを用い、確認後に削除する。

## 段階2: 実MCP client

1. clientのStreamable HTTP endpointを`https://marginalis.sandi05.com/mcp`に設定する。
2. Protected Resource MetadataからOAuth authorization endpointへ進み、Kanidm login後、通常userとして
   scopeを確認して許可する。root sessionはMCP clientを認可できない。
3. `search_notes`、`create_note`、`get_note`、`update_note`、`prepare_delete_note`、`delete_note`を順に実行する。
   RESTで作ったノートはMCP検索でも、MCPで作ったノートはREST検索でも同じACL結果になることを確認する。
4. RESTの`DELETE /api/v1/mcp-authorizations?client_id=...`で認可を取消し、既存access tokenによる`/mcp`が
   `401`になることを確認する。

unknown clientを使う場合はClient ID Metadata Documentのhostを`clientMetadataAllowedHosts`へ追加する。
metadataを持たないclientは、rootが事前登録してから試す。

## 段階3: 運用確認

1. root監査を読み取り専用で確認する。

   ```sh
   sudo -u marginalis sqlite3 /var/lib/marginalis/marginalis.sqlite \
     'SELECT action, occurred_at_ms FROM root_audit_log ORDER BY audit_id DESC LIMIT 100;'
   ```

2. 永続backup storageと保持世代を決め、`backupDirectory`を設定する。`marginalis-backup.service`を起動後、
   新しいgenerationに`FORMAT`、`MANIFEST`、`COMPLETE`、`marginalis.sqlite`、`notes/`があることを確認する。
3. 本番dataDirを切り替えず、`marginalis restore --input <generation> --output <新しい絶対path>`を実行する。
   `RESTORED` marker、SQLiteおよび正本が作られることを確認する。実際のdataDir切替は、旧データを
   保持する場所とrollback手順を決めてから行う。
4. `marginalis-rebuild-projections.service`を実行し、main serviceを再起動してhealth/readinessを再確認する。
5. `systemctl status marginalis-prune-audit.timer`で、root監査の365日保持timerが有効であることを確認する。

各段階で、失敗時は`X-Request-Id`と`journalctl -u marginalis.service -b --no-pager`を対応付ける。
