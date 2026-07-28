# Web UIとACLの受入確認

この文書は、v0.7.0で追加したWeb UI、ノート参照、identity単位ACLの受入項目と結果を記録します。
SQLite schemaは8、archive形式は`marginalis-archive-6`、note profileは2です。以前のdatabaseと
archiveは自動で移行しません。

## 自動テスト

PR CIと公開前の`release-gate`で次を確認します。

- React・TypeScriptフロントエンドの整形、静的解析、型検査、単体試験、依存監査、配布ビルド
- ノートの作成、編集、安全なプレビュー、入力診断、revision競合の比較と再保存
- 日本語、絵文字、CRLF、高頻度入力、入力上限、解析失敗
- ノート参照の保存時索引化、参照元・参照先の表示、削除・復元・物理削除との整合
- 所有者、閲覧者、編集者、対象外利用者の表示・REST操作・情報非漏洩
- ACL更新の同一オリジン、CSRFトークン、revision確認
- ノートとACLを同じSQLite読み取りtransactionから取得する論理スナップショット
- Kanidm 1.10、TLS、nginxサブパス、OIDC、MCP OAuth、backup、purge、障害診断
- 空のdatabaseへのNixOS配備と、旧schemaを自動移行せず拒否すること

## 必須確認

1. ログイン後にノートを作成し、閲覧、編集、プレビュー、保存を実行します。
2. revision競合で三つの内容を比較し、修正後に最新revisionへ再保存します。
3. 所有者が閲覧者と編集者をACLへ追加し、それぞれの表示と操作範囲を確認します。
4. 対象外利用者にノートと関連情報が表示されず、直接指定した操作も`404`になることを確認します。
5. ChatGPT、Claude Code、Codex CLIからMCPへ接続し、ACLと同じ閲覧・更新結果になることを確認します。
6. archiveを隔離した空databaseへ復元し、ノート、ACL、参照、削除状態、revisionが一致することを
   確認します。

実施結果には完了日と成否だけを記録し、秘密情報やノート本文は記録しません。

## 実施結果

- 2026-07-28: 自動試験成功
