# ロードマップ

この文書は、公開済み機能の履歴ではなく、現在の作業、次に必要な判断、保留中の候補を示します。
公開済みの利用者影響は[変更履歴](../CHANGELOG.md)、個別の完了条件は
[GitHub Issues](https://github.com/KeishiS/marginalis/issues)を参照してください。

## 現在地

`v0.9.0`は2026-07-28に公開しました。現在は、MCP向けOAuthをMarginalisから外部の
Authorization Serverへ移すかを
[#24](https://github.com/KeishiS/marginalis/issues/24)で評価しています。移行可否を決めるまでは
内蔵実装を維持し、外部候補のために実装を変更しません。

現行のREST APIは`/api/v3`、SQLite schemaは9、note profileは3、アーカイブは
`marginalis-archive-7`です。完全なAsciiDoc文書を保存の正本とし、OpenAPI、TypeScript
クライアント、MCPツール定義を`marginalis-contract`から生成します。

## 次の判断

#24では、[共通の評価手順](mcp-authorization-server-evaluation.md)に従って内蔵実装、
WorkOS AuthKit、Auth0、Keycloakを比較します。次の条件をすべて実際の接続で確認した候補だけを
移行対象とします。

- ChatGPT、Claude Code、Codex CLIからの接続
- 利用者、group、`resource`、`audience`、`scope`、失効の検証
- 所有者とACL共有先だけがノートを操作できること
- 小規模環境での費用、運用負担、障害時の影響

採否はADRで決定します。移行を採用する場合だけ、削除する内蔵実装と移行手順を別の実装Issueで
定めます。

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
