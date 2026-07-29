# ロードマップ

この文書は、公開済み機能の履歴ではなく、現在の作業、次に必要な判断、保留中の候補を示します。
公開済みの利用者影響は[変更履歴](../CHANGELOG.md)、個別の完了条件は
[GitHub Issues](https://github.com/KeishiS/marginalis/issues)を参照してください。

## 現在地

`v0.9.0`は2026-07-28に公開しました。Issue
[#24](https://github.com/KeishiS/marginalis/issues/24)の比較と実接続を経て、MCPのAuthorization
ServerにAuth0を採用しました。現在は内蔵Authorization Serverを撤去し、MarginalisをProtected
Resourceへ限定する移行を進めています。採用理由は
[ADR 0001](adr/0001-auth0をmcpのauthorization-serverに採用.md)を参照してください。

現行のREST APIは`/api/v3`、SQLite schemaは10、note profileは3、アーカイブは
`marginalis-archive-7`です。完全なAsciiDoc文書を保存の正本とし、OpenAPI、TypeScript
クライアント、MCPツール定義を`marginalis-contract`から生成します。

## 移行の完了条件

ChatGPT Web UIではDCR、Kanidm login、scope、ノート操作、所有者・ACLを確認済みです。移行の
公開判断までに、次を完了します。

- Claude CodeとCodex CLIからの接続
- Auth0でのgrant取消後におけるaccess tokenとrefresh tokenの挙動測定
- schema 10へのarchive経由移行
- NixOS配備、ログ、障害診断、文書の検証

## 今回扱わない作業

- **#19 検索**: 今回の再設計とリリース候補から除外
- **#22 グラフ**: 今回の再設計とリリース候補から除外

これらは削除した要件ではありません。再開するときは、現在の契約、認可、運用条件に基づいて
GitHub Issueの前提と完了条件を見直します。

## 保留条件

- **PostgreSQL**: 複数process、高可用性、またはSQLiteで満たせない規模が実測された場合に再検討
- **添付Resource**: 保存先、MIME type、容量、認可、バックアップを一つの公開契約として
  定義できる場合に再検討
- **文書のAsciiDoc化**: [評価結果](repository-asciidoc-evaluation.md)に基づき、Markdownでは
  解決できない具体例と、変換後に強化できる検査がそろった場合に再検討

## 継続監視

- ChatGPT、Claude Code、Codex CLIのMCP接続
- 所有者と直接ACL共有先のtransport横断認可
- AdocWeave package版、note profile、OpenAPI、MCPツール定義の一致
- バックアップ最終成功時刻、保存世代数、四半期復元試験
- データベース容量、ノート数、revision競合
