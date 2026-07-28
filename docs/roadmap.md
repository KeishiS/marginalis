# ロードマップ

この文書は、公開済み機能の履歴ではなく、現在の作業、次に必要な判断、保留中の候補を示します。
公開済みの利用者影響は[変更履歴](../CHANGELOG.md)、個別の完了条件は
[GitHub Issues](https://github.com/KeishiS/marginalis/issues)を参照してください。

## 現在地

`v0.7.0`は2026-07-28に公開しました。後方互換性と移行コストを制約にせず進めた再設計は、
`v0.8.0`として統合します。

| 順序 | Issue | 対象 | 状態 |
| --- | --- | --- | --- |
| 1 | [#54](https://github.com/KeishiS/marginalis/issues/54) | domain型とapplication境界 | 実装・検証済み |
| 2 | [#55](https://github.com/KeishiS/marginalis/issues/55) | ACLとSQLite transaction境界 | 実装・検証済み |
| 3 | [#52](https://github.com/KeishiS/marginalis/issues/52) | 公開契約とTypeScript Web UI | 実装・検証済み |
| 4 | [#53](https://github.com/KeishiS/marginalis/issues/53) | 要件、試験、受入、リリースゲート | 実装・自動検証済み |

`v0.8.0`のREST APIは`/api/v3`だけを提供します。OpenAPI、TypeScriptクライアント、MCPツール定義は
`marginalis-contract`から生成し、一覧、閲覧、編集、共有設定は一つのReactアプリケーションが
担当します。Issue #57以降では完全なAsciiDoc文書を保存の正本とします。SQLite schemaは9、
note profileは3、アーカイブは`marginalis-archive-7`です。

## 次の判断

#53の自動検証ゲートは成功しました。次の公開候補を作るかは、人手受入を含む次の条件で判断します。

- `cargo make verify`と対象NixOS VMの成功
- OpenAPI、TypeScript、MCP、実ルーターの契約一致
- [要件と検証の対応表](traceability.md)の記載漏れなし
- [受入基準](acceptance.md)に従った版別結果と証跡
- 破壊的変更、保存形式、再初期化手順の変更履歴への記載

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
- **文書のAsciiDoc化**: 形式変更に見合う保守上の便益を測定できる場合に再検討

## 継続監視

- ChatGPT、Claude Code、Codex CLIのMCP接続
- 所有者と直接ACL共有先のtransport横断認可
- AdocWeave package版、note profile、OpenAPI、MCPツール定義の一致
- バックアップ最終成功時刻、保存世代数、四半期復元試験
- データベース容量、ノート数、revision競合
