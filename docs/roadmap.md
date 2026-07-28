# ロードマップ

この文書は、公開済み機能の履歴ではなく、現在の作業、次に必要な判断、保留中の候補を示します。
公開済みの利用者影響は[変更履歴](../CHANGELOG.md)、個別の完了条件は
[GitHub Issues](https://github.com/KeishiS/marginalis/issues)を参照してください。

## 現在地

`v0.8.0`は2026-07-28に公開しました。現在は、日常的なノート操作を補強する`v0.9.0`を
準備しています。保存形式とREST APIの世代は`v0.8.0`から変更せず、Web UIの一覧、編集、
プレビュー、閲覧表示と、変更を継続しやすくする検証構成を改善します。

| 順序 | Issue | 対象 | 状態 |
| --- | --- | --- | --- |
| 1 | [#65](https://github.com/KeishiS/marginalis/issues/65) | 一覧の情報、絞り込み、ページ分割 | 実装・検証済み |
| 2 | [#66](https://github.com/KeishiS/marginalis/issues/66) | 編集状態、入力診断、プレビュー継続 | 実装・検証済み |
| 3 | [#67](https://github.com/KeishiS/marginalis/issues/67) | 表、コード、数式を含む表示回帰 | 実装・検証中 |
| 4 | [#68](https://github.com/KeishiS/marginalis/issues/68) | 設計、CI、文書の横断監査 | 作業中 |
| 5 | [#64](https://github.com/KeishiS/marginalis/issues/64) | `v0.9.0`統合とリリース判断 | 未完了 |

現行のREST APIは`/api/v3`、SQLite schemaは9、note profileは3、アーカイブは
`marginalis-archive-7`です。完全なAsciiDoc文書を保存の正本とし、OpenAPI、TypeScript
クライアント、MCPツール定義を`marginalis-contract`から生成します。

## 次の判断

`v0.9.0`を公開するかは、#68の横断監査後に次の条件で判断します。

- `cargo make pre-push`とリリースゲートの成功
- OpenAPI、TypeScript、MCP、実ルーターの契約一致
- [要件と検証の対応表](traceability.md)の記載漏れなし
- [受入基準](acceptance.md)に従った版別結果と証跡
- 変更履歴と運用文書の現行実装との一致

## 今回扱わない作業

- **#19 検索**: 今回の再設計とリリース候補から除外
- **#22 グラフ**: 今回の再設計とリリース候補から除外
- **#24 外部Authorization Server評価**: 別の作業セッションで継続

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
