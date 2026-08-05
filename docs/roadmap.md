# ロードマップ

この文書は、公開済み機能の履歴ではなく、現在の作業、次に必要な判断、保留中の候補を示します。
公開済みの利用者影響は[変更履歴](../CHANGELOG.md)、個別の完了条件は
[GitHub Issues](https://github.com/KeishiS/marginalis/issues)を参照してください。

## 現在地

`v0.29.0`を2026-08-05に公開しました。公開済み機能と移行方法は
[変更履歴](../CHANGELOG.md#0290--2026-08-05)、確認結果は
[v0.29.0受入結果](acceptance-results/v0.29.0.md)を参照してください。この文書には公開履歴を
書き写さず、未完了の判断と作業だけを置きます。

## 公開予定

### v0.30.0: Authorization Serverの共有境界

大きなリファクタリングの前に、アーキテクチャの設計条件と回帰試験を項目単位で対応付ける
[#304](https://github.com/KeishiS/marginalis/issues/304)を行います。2026年7月には、OIDC login attemptの
発行時に必要な期限切れ行の削除がリファクタリングで失われ、文書だけが正しい状態になる回帰がありました。
設計条件に安定した識別子を付け、検証先の記載漏れを機械的に拒否してから、中核の分離を進めます。

その上で、[#269](https://github.com/KeishiS/marginalis/issues/269)によりOAuthの状態遷移、scope判定、
metadata生成を製品非依存の共有crateへ集約します。Axum、SQLite、Marginalis固有のresourceとscopeは
adapter側に残し、異なるresourceと保存先を持つ試験用consumerで境界を確認します。この版では独立した
リポジトリへ移さず、既存のgrantとtokenを維持し、DB schemaとarchive形式を変更しないことを原則とします。

同じ認可境界の改善として、同意画面で完全な`client_id`を主要な識別情報として示す
[#306](https://github.com/KeishiS/marginalis/issues/306)も含めます。MathJaxは、CSPだけに依存せず不要な
TeX packageの読込を入力段階で拒否する
[#305](https://github.com/KeishiS/marginalis/issues/305)により、許可するpackageを明示的に固定します。

### v0.31.0: ノートの来歴と人手確認

先に、外部文献管理ツールとの同期契約を扱う
[#225](https://github.com/KeishiS/marginalis/issues/225)のADRで、文献の取得元を保存する必要があるか決めます。
続いて、ノートの問い合わせ、更新、描画・引用解決、関係の図、ACLが同居するapplication moduleを
[#307](https://github.com/KeishiS/marginalis/issues/307)で分割します。公開動作を変えないこの整理を終えてから、
ノートの作成経路と人手確認状態を扱う
[#232](https://github.com/KeishiS/marginalis/issues/232)を実装します。

保存項目、REST、MCP、archiveの契約が変わるため、実装後は通常版の前に受入用の候補コミットを固定し、
export、archive移行、隔離復元、importを確認します。`v0.31.0`として公開するのは、既存データを維持する
移行経路と人手確認状態の更新規則がそろった後です。

### v0.32.0: 外部文献管理ツールとの連携

#225のADRから実装Issueを分け、最初はCSL-JSONによる一方向の取り込みを候補とします。変更前の事前確認、
citation keyの維持、競合の診断、入力から消えた文献を自動削除しない動作を先にそろえます。外部APIの
認証情報を保持する接続や双方向同期は、この契約を実運用で確認してから後続版で判断します。

### Authorization Serverの独立配布

`v0.30.0`で共有crateの境界と試験用consumerを確認した後、#269のADRで独立リポジトリ、registry、固定した
Git dependencyを比較します。独立リポジトリへの移動は、その判断とversion方針、脆弱性対応、両利用側の
CIがそろってから行います。

[連環（Renkan）](research-search-vision.md)はMarginalisとは別の探索サービスです。Renkanへの統合と
PostgreSQL adapterはRenkan側の計画で管理し、その版をMarginalisの公開予定へ含めません。Marginalis側では
別のresourceと保存先を使う試験実装により、分離した中核が製品固有の型へ依存しないことを確認します。

パッチ版は予定へ固定しません。各通常版の公開後に、互換性を変えない不具合や移行手順の修正が必要に
なった場合だけ公開します。

## 本文からの検索

全文検索の機能は当面提供しません。一覧で使える絞り込みはタグと更新日だけです。

方式を比較する#19は、意味検索と埋め込みモデルを比較対象に含み、機械学習を伴う作業へ進む
前提になっていたため終了しました。索引の設計、順位付け、評価集合の作成は、必要になった時点で
改めて起こします。

関係の図には、図の範囲を絞るための語の照合があります。題名、本文、タグのいずれかにその語を
含むノートだけで部分グラフを組み直します。索引を作らず、約1,000ノートの想定規模で単純な照合を
行うだけであり、順位付けも行いません。全文検索の機能として広げる場合は、上記の評価から改めて
始めます。

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
