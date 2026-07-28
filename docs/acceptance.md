# AdocWeave 0.11移行の受入確認

この文書は、AdocWeave 0.11.0へ移行したv0.6.0の受入項目と結果を記録します。SQLiteスキーマ4と
note profileバージョン`1`は維持し、アーカイブ形式を`marginalis-archive-4`へ更新しました。
AdocWeaveのバージョンが異なるアーカイブは自動で移行しません。

## 自動テスト

PR CIの`verify`と`nixos-e2e`、公開前の`cargo make release-gate`で次を確認します。

- SQLite schema 4に`note_acl`が存在しないこと
- archive v4がノートを直接保持し、旧bundle形、旧format、未知fieldを拒否すること
- 所有者identityがissuerとsubjectの完全一致であること
- 所有者、非所有者、`server-admins`の一覧・取得・更新・削除・復元
- MCP scopeが所有権を拡張しないこと
- アーカイブを隔離した環境へ復元し、再度書き出したデータが一致すること
- v0.5.0のschema定義で作成したdatabaseからの起動と既存ノートの保持
- 0.10.1のarchiveをimportした場合の、保存先作成前の拒否
- Kanidm、OIDC、OAuth、backup、purge、障害診断の回帰
- 0.10.1と0.11.0の固定入力に対する保存可否、診断位置、HTMLの一致
- 執筆時URL、描画時URLおよびHTML出力上限の独立した検査

## 必須確認

1. 空のSQLite databaseへ配備し、health endpointが`200`を返すことを確認します。
2. 一般利用者Aが作成したノートを、Aが一覧・取得・更新・削除・復元できることを確認します。
3. 一般利用者BにはAのノートが一覧へ現れず、IDを指定した取得・更新・削除・復元も`404`になることを
   確認します。同じsubjectでもissuerが異なる場合は別identityとして扱います。
4. `server-admins`がすべてのノートを一覧・取得・更新・削除・復元できることを確認します。
5. ChatGPT、Claude Code、Codex CLIでMCP認可を行い、所有者操作と非所有者の`not_found`を確認します。
   `notes:write`または`notes:delete`を持つtokenも所有権を越えられないことを確認します。
6. archiveをexportし、formatが`marginalis-archive-4`、AdocWeave版が`0.11.0`、
   note profile版が`1`であることを確認します。隔離した空databaseへ復元し、所有者、削除状態、
   revisionが一致することを確認します。AdocWeave版が`0.10.1`のarchiveは変更前に拒否されることも
   確認します。
7. backup、復元、purge、OIDC、MCP OAuthを確認し、ログや失敗証跡へCookie、token、認可code、
   client secret、ノート本文が出ないことを確認します。

実施結果には必須項目の完了日と成否だけを記録し、環境やclient版などの詳細は記録しません。

## 実施結果

- 2026-07-27: 成功
