# v0.4.0 受入確認

この受入はAdocWeave 0.10.1、SQLite schema 3、archive v2、共通入力診断を導入する
破壊的リリースを対象とします。v0.3以前のdatabaseとarchiveは移行せず、空のdatabaseから
初期化します。

## 自動証跡

PR CIの`verify`と`nixos-e2e`は、それぞれ`cargo make verify`と`nix flake check -L`を
実行します。`cargo make release-gate`はrelease metadata、配布package、protocol fixture、
失敗証跡の秘密情報検査も実行します。

自動試験では次を確認します。

- AdocWeave 0.10.1の固定、Strict modeによる保存・表示時の同一解析、型付き診断
- REST 422とMCP `isError: true`で共有する安定code、対象field、UTF-8 byte範囲
- `get_note_profile`、MCP tool schema、OpenAPIに公開する入力上限と規則の一致
- SQLite schema 3、archive v2のidentity、全階層の未知field拒否、空databaseへの復元
- Kanidm、OIDC、OAuth、ACL、backup、復元、障害診断の既存回帰試験

## 必須確認

1. 空のSQLite databaseへ配備し、`marginalis --version`が`0.4.0`、health endpointが
   `200`を返すことを確認します。
2. Web UIとRESTで有効なノートを作成・表示し、未許可source言語、外部参照、passthrough、
   不正なtitle・tagが位置付き診断で拒否されることを確認します。
3. archiveをexportし、formatが`marginalis-archive-2`、AdocWeave版が`0.10.1`、
   note profile版が`1`であることを確認します。隔離した空databaseへ復元します。
4. ChatGPT、Claude Code、Codex CLIでMCP認可を行い、`get_note_profile`を取得した後に
   create、update、readを確認します。無効入力ではJSON-RPC errorではなく
   `isError: true`のtool resultと構造化診断が返ることを確認します。
5. `notes:write`だけを持つtokenでも`get_note_profile`を取得でき、scope不足は従来どおり
   `403 insufficient_scope`になることを確認します。
6. backup、復元、purge、OIDC、MCP OAuthの運用確認を行い、ログや失敗証跡へCookie、
   token、認可code、client secret、ノート本文が出ないことを確認します。

実施結果には必須項目の完了日と成否だけを記録し、環境やclient版などの詳細は記録しません。
