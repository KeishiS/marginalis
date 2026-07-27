# v0.5.0 受入確認

この受入はノート単位ACLを削除し、所有者と`server-admins`へ認可を単純化する破壊的リリースを
対象とします。旧`dataDir`、schema 3以前、archive v2以前は移行せず、空databaseから初期化します。

## 自動証跡

PR CIの`verify`と`nixos-e2e`、公開前の`cargo make release-gate`で次を確認します。

- SQLite schema 4に`note_acl`が存在しないこと
- archive v3がノートを直接保持し、旧bundle形、旧format、未知fieldを拒否すること
- 所有者identityがissuerとsubjectの完全一致であること
- 所有者、非所有者、`server-admins`の一覧・取得・更新・削除・復元
- MCP scopeが所有権を拡張しないこと
- archiveの隔離復元と論理的な往復
- Kanidm、OIDC、OAuth、backup、purge、障害診断の回帰

## 必須確認

1. 旧`dataDir`を退避後に完全削除し、空のSQLite databaseへ配備します。
   `marginalis --version`が`0.5.0`、health endpointが`200`を返すことを確認します。
2. 一般利用者Aが作成したノートを、Aが一覧・取得・更新・削除・復元できることを確認します。
3. 一般利用者BにはAのノートが一覧へ現れず、IDを指定した取得・更新・削除・復元も`404`になることを
   確認します。同じsubjectでもissuerが異なる場合は別identityとして扱います。
4. `server-admins`がすべてのノートを一覧・取得・更新・削除・復元できることを確認します。
5. ChatGPT、Claude Code、Codex CLIでMCP認可を行い、所有者操作と非所有者の`not_found`を確認します。
   `notes:write`または`notes:delete`を持つtokenも所有権を越えられないことを確認します。
6. archiveをexportし、formatが`marginalis-archive-3`、AdocWeave版が`0.10.1`、
   note profile版が`1`であることを確認します。隔離した空databaseへ復元し、所有者、削除状態、
   revisionが一致することを確認します。
7. backup、復元、purge、OIDC、MCP OAuthを確認し、ログや失敗証跡へCookie、token、認可code、
   client secret、ノート本文が出ないことを確認します。

実施結果には必須項目の完了日と成否だけを記録し、環境やclient版などの詳細は記録しません。

## 実施結果

- 2026-07-27：成功
