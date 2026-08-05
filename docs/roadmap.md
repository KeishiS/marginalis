# ロードマップ

この文書は、公開済み機能の履歴ではなく、現在の作業、次に必要な判断、保留中の候補を示します。
公開済みの利用者影響は[変更履歴](../CHANGELOG.md)、個別の完了条件は
[GitHub Issues](https://github.com/KeishiS/marginalis/issues)を参照してください。

## 現在地

`v0.31.0`を2026-08-05に公開しました。公開済み機能と移行方法は
[変更履歴](../CHANGELOG.md#0310--2026-08-05)、確認結果は
[v0.31.0受入結果](acceptance-results/v0.31.0.md)を参照してください。この文書には公開履歴を
書き写さず、未完了の判断と作業だけを置きます。

## 公開予定

### v0.32.0: 外部文献管理ツールとの連携

[ADR 0010](adr/0010-外部書誌はcsl-jsonから一方向に取り込む.md)に従い、最初はCSL-JSONファイルを
事前確認してから一方向に取り込みます。citation keyの維持、取込元との対応、双方が変わった場合の競合、
入力から消えた文献を自動削除しない動作をそろえます。外部APIの認証情報を保持する接続や双方向同期は、
この契約を実運用で確認してから後続版で判断します。

現在は[#327](https://github.com/KeishiS/marginalis/issues/327)で、domainとapplicationの分類・保存計画、
SQLite schema 20、REST契約、Web UI、archive 16と、要件・設計条件に対応する境界試験を実装しています。
リポジトリ全体とNixOS移行の検証後にPull Requestで統合します。候補コミットでは
archive 15から16への変換、空のschema 20への隔離復元、初回取込と再取込の競合解決を受け入れた後、
`v0.32.0`として公開します。

### Authorization Serverの独立配布

`v0.30.0`で共有crateの境界とrepository契約試験を確認した後、
[#314](https://github.com/KeishiS/marginalis/issues/314)で独立リポジトリ、registry、固定したGit dependencyを
比較します。独立リポジトリへの移動は、その判断と版管理、脆弱性対応、両利用側のCIがそろってから
行います。現在の責務と更新手順は
[MCP Authorization Serverの保守](mcp-authorization-server-maintenance.md)を正本とします。

[連環（Renkan）](research-search-vision.md)はMarginalisとは別の探索サービスです。Renkanへの統合と
PostgreSQL adapterはRenkan側の計画で管理し、その版をMarginalisの公開予定へ含めません。Marginalis側では
別のresourceと保存先を使う試験実装により、分離した中核が製品固有の型へ依存しないことを確認します。

パッチ版は予定へ固定しません。各通常版の公開後に、互換性を変えない不具合や移行手順の修正が必要に
なった場合だけ公開します。

## 検索のサービス境界

全文検索、意味検索、検索索引、順位付けはMarginalisへ実装せず、
[連環（Renkan）](research-search-vision.md)へ委ねます。MarginalisはAsciiDocノートの正本、編集、共有を
担当し、RenkanはMarginalisを最初のデータソースとして検索投影を作り、ほかの研究資料と横断して
探索します。検索基盤、Marginalisとの接続契約、検索API、MCP、評価はRenkan側の計画で管理し、
Marginalisの公開予定へ含めません。

Marginalisのノート一覧にはタグと更新日による絞り込みを残します。関係の図では、表示する部分グラフを
組み直すために題名、本文、タグを単純に照合します。編集画面では開いている文書内を検索できます。
これらは検索索引、順位付け、横断検索を持たない局所的な操作であり、全文検索へ拡張しません。

## 次の破壊的更新

[#105](https://github.com/KeishiS/marginalis/issues/105)のSQLite schema番号体系の再開始は、公開版をまだ
割り当てません。現在の案は旧DBとarchiveを移行せず、空の`dataDir`から始めるため、内部的な番号整理に
対して運用負担が大きいためです。

通常のschema更新を続けるか、archive経由の移行を可能にして新しい番号体系へ移るか、既存データとの
互換性を切る破壊的更新にするかを、#225と#232の保存形式が確定した後に判断します。実施する場合も、
旧`dataDir`を自動削除せず、旧実行環境での確認と復旧に使える状態を維持します。

## 保留条件

- **PostgreSQL**: 複数ホスト、高可用性、復旧目標、代表負荷などの
  [再検討条件](adr/0002-sqliteを正本として維持する.md#再検討条件)が実測で成立した場合だけ再検討
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
- SQLite診断の失敗分類と旧schema拒否時のファイル不変性
