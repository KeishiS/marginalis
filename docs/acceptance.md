# v0.3.1 受入確認

この受入はv0.3.0のschema version 2と公開APIを維持した保守リリースを対象とします。v0.2のDB、
`dataDir`、ファイル正本、root credentialは入力にしません。実環境では更新前に成功backupを確保し、
本番とは隔離した空のSQLite databaseへ復元できることを確認します。

## 自動証跡

PR CIの`verify`と`nixos-e2e`は、それぞれ`cargo make verify`と`nix flake check -L`を実行します。
`cargo make release-gate`はこれらにrelease metadata、配布package、protocol fixture、失敗証跡の
秘密情報検査を加えます。

NixOS VMは次の経路を自動確認します。

- backup生成、markerとarchiveの検証、最新成功世代の隔離復元、論理内容の一致、30世代保持
- ACL、revision、ソフトデリート状態の復元と、空でないdatabase・破損archiveの拒否
- database破損、schema不一致、OIDC到達不能、保存先容量不足、purge失敗と回復
- 実Kanidm 1.10.4、private CA、nginx subpath、Playwrightによるloginとcallback
- DCR、Authorization Code + PKCE S256、同一sessionのconsent、CSRF・Origin拒否
- refresh token rotationと再利用時のfamily失効、実HTTP MCP初期化、認可取消

プロセス内結合試験は、署名済みgroup claimを発行するmock IdPと本番用SQLite・service・routerを使い、
`server-users`拒否、`server-admins`可視性、JSON-RPC、MCP tool callを網羅します。外部MCP clientと
本番データの確認だけを手動受入としてrelease issueへ記録します。

## 必須確認

1. NixOS module で service を配備し、`GET /api/v2/health` が `200` を返す。
   OAuth metadataは`baseUrl`からRFC 8414/9728に従って導出し、Kanidmの`issuerUrl`からは
   導出しない。host rootの`baseUrl`ではsubject pathを付けず、subpathの場合だけwell-known
   suffixの後ろへsubject pathを付ける。
2. TLSとサブパスでOIDC loginを行い、`server-users`所属者だけがsessionを取得できる。
3. `server-admins` の利用者が他人のノートを閲覧・管理でき、通常利用者には ACL が適用される。
4. `groups` claim を持たない主体と `server-users` 非所属の主体がログインを拒否されること、
   `server-admins` の追加が次回ログインで管理権限として反映されることを確認する。
5. Web UI と `/api/v2` からノートを作成、更新、削除、復元し、revision conflict と CSRF 拒否を確認する。
6. ChatGPT、Claude Code、Codex CLI の各 MCP client で Dynamic Client Registration、OIDC login、
   Authorization Code + PKCE、read/write、認可取消を確認する。ChatGPTがclient originから送る認可開始の
   `POST /oauth/authorize`はOAuth parameterがURL queryにあっても`303`でloginへ進み、client自身のCSRF
   fieldがあっても`same_origin_required`にならないことを確認する。一方、Marginalisの承認formから送る
   `POST /oauth/authorize/consent`は、OAuth clientのpopupやsandboxがopaqueな`Origin`を送っても、
   同一sessionのCSRF cookieとform tokenが一致する場合だけ認可を確定することを確認する。MCP requestの
   `Origin`が設定済み許可リストに一致すること、無効tokenが`401 invalid_token`、scope不足が
   `403 insufficient_scope`になることも確認する。token endpointへ`Authorization: Basic`を送った場合は
   `401 invalid_client`と`WWW-Authenticate: Basic`になることを確認する。
   Claude Codeは`claude mcp add --transport http marginalis B/mcp`で追加し、`/mcp`から認証する。
   DCRで登録される`http://localhost:PORT/callback`に明示portがあることを確認する。
   Claude.ai Web UIは`Customize`の`Connectors`へ`B/mcp`をcustom connectorとして追加する。
   Claude.ai subscriptionでClaude Codeへログインしている場合は、同connectorがClaude Codeにも表示される
   ことを確認する。
7. `marginalis-backup.service`が指定先に検証済みarchiveを作り、30世代だけを保持することを確認する。
   日次backup・purge timerと、四半期の`marginalis-restore-check.timer`が有効であることも確認する。
8. `marginalis diagnose`、systemdの終了status、安定event名、request IDを使い、database、
   OIDC、保守jobの失敗原因をノート本文や認証情報なしで追跡できることを確認する。

現行schema versionは2です。v0.3.0からv0.3.1では同じdatabaseを保持します。v0.2以前のschemaからの
自動移行は受入対象外です。

実施結果は release issue に、環境、client の版、Kanidm 1.10 の版、base URL（機密情報を除く）、
各項目の結果として記録します。HTTP失敗時はresponseの`X-Request-Id`を記録し、同じIDの
`marginalis` service logを添えます。Cookie、token、authorization code、client secret、ノート本文は
記録しません。
