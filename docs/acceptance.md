# v0.3.0 受入確認

この受入は空の SQLite database から行います。v0.2 の DB、`dataDir`、ファイル正本、root credential は
入力にしません。archive import と定期復元試験は公開後の運用改善であり、本 release gate には含めません。

## 自動証跡

PR CI の `verify` と `nixos-e2e` はそれぞれ `cargo make verify` と `nix flake check -L` を実行する。
後者には NixOS module の配備、backup/purge、OIDC 未到達時の login fail-closed、実 Kanidm 1.10.4 の
private CA・OAuth2 client provisioning・OIDC Discovery を含む。プロセス内結合試験は、署名済みgroup
claimを発行するmock IdPとv0.3の本番用SQLite・service・routerを使い、`server-users`拒否、
`server-admins`可視性、MCP OAuth、ノート作成、認可取消後のaccess/refresh token失効を確認する。
以下の実browser操作、実Kanidmのgroup変更、外部MCP clientは実運用のIdPとclientを要するため、
release issueで手動結果を記録する。

## 必須確認

1. NixOS module で service を配備し、`GET /api/v2/health` が `200` を返す。
   OAuth metadataは`baseUrl`からRFC 8414/9728に従って導出し、Kanidmの`issuerUrl`からは
   導出しない。host rootの`baseUrl`ではsubject pathを付けず、subpathの場合だけwell-known
   suffixの後ろへsubject pathを付ける。
2. TLS とサブパスで OIDC login を行い、`server-users` 所属者だけが session を取得できる。
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
7. `marginalis-backup.service` が指定先に archive を作ること、日次 purge timer が有効なことを確認する。

現行schema versionは2である。旧schemaからの自動移行は受入対象外であり、再配備時はarchiveを退避したうえで
空のdatabaseを初期化し、MCP clientを再登録・再認可する。

実施結果は release issue に、環境、client の版、Kanidm 1.10 の版、base URL（機密情報を除く）、
各項目の結果として記録します。HTTP失敗時はresponseの`X-Request-Id`を記録し、同じIDの
`marginalis` service logを添えます。Cookie、token、authorization code、client secret、ノート本文は
記録しません。
