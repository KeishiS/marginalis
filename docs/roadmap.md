# ロードマップ

この文書は、公開済み機能の履歴ではなく、現在の作業、次に必要な判断、保留中の候補を示します。
公開済みの利用者影響は[変更履歴](../CHANGELOG.md)、個別の完了条件は
[GitHub Issues](https://github.com/KeishiS/marginalis/issues)を参照してください。

## 現在地

`v0.26.1`を2026-08-04に公開しました。公開済み機能と移行方法は
[変更履歴](../CHANGELOG.md#0261--2026-08-04)、確認結果は
[v0.26.1受入結果](acceptance-results/v0.26.1.md)を参照してください。この文書には公開履歴を
書き写さず、未完了の判断と作業だけを置きます。

## 公開予定

### v0.27.0: Authorization Serverの内部境界

最初に、[#269](https://github.com/KeishiS/marginalis/issues/269)のうち、内蔵Authorization Serverの
中核をMarginalis固有のドメインから分離します。この段階では独立したリポジトリやdaemonを作らず、
Marginalisリポジトリ内の製品非依存crateとして同一プロセスへ組み込みます。

既存のURL、scope、token寿命、SQLite schema、発行済みtokenとクライアント登録を意図せず変更しません。
Marginalisを分離後のcrateへ切り替え、ChatGPT、Claude Code、Codex CLIによる接続と取消までの回帰試験が
成功した時点で`v0.27.0`を公開します。構造変更を単独で公開することで、後続のscope変更と不具合の原因を
分けて確認できるようにします。

### v0.28.0: Web UIの基本操作

OAuth同意画面を通常画面と統一する
[#262](https://github.com/KeishiS/marginalis/issues/262)と、ノートの削除・復元をWeb UIから完結させる
[#263](https://github.com/KeishiS/marginalis/issues/263)、
[#267](https://github.com/KeishiS/marginalis/issues/267)をまとめます。削除だけを先に公開してWeb UIから
復元できない期間を作らず、一連の操作と受入試験がそろった時点で`v0.28.0`を公開します。

### v0.29.0: MCPアクセス制御

利用者とクライアントのscope上限を定める
[#268](https://github.com/KeishiS/marginalis/issues/268)を先に設計し、製品固有のscopeポリシーとして
分離後のAuthorization Serverへ渡します。その上で、書誌情報のscope分離
[#266](https://github.com/KeishiS/marginalis/issues/266)、同意時のscope選択
[#261](https://github.com/KeishiS/marginalis/issues/261)、認可済みクライアントの管理
[#264](https://github.com/KeishiS/marginalis/issues/264)、段階的な追加認可
[#265](https://github.com/KeishiS/marginalis/issues/265)の順に実装します。

既存クライアントの再認可を複数回求めないよう、これらは一つの`v0.29.0`として公開します。発行可能なscopeの
計算は共有可能な中核へ置きますが、scopeの名称、MCP toolとの対応、同意画面、利用者向け設定は
Marginalisに残します。

### Authorization Serverの共有元

`v0.27.0`ではcrateの公開境界を固定せず、`v0.29.0`までのscopeポリシー実装を通じて、製品非依存の
境界が実用に耐えるか確認します。その後、#269のADRで独立リポジトリ、registry、固定したGit dependencyを
比較します。独立リポジトリへの移動は、その判断とversion方針、脆弱性対応、両利用側のCIがそろってから
行います。

[連環（Renkan）](research-search-vision.md)はMarginalisとは別の探索サービスです。Renkanへの統合と
PostgreSQL adapterはRenkan側の計画で管理し、その版をMarginalisの公開予定へ含めません。Marginalis側では
別のresourceと保存先を使う試験実装により、分離した中核が製品固有の型へ依存しないことを確認します。

### v0.30.0以降: 保存形式と外部連携

ノートの作成経路と人手確認状態を扱う
[#232](https://github.com/KeishiS/marginalis/issues/232)と、外部の文献管理ツールとの同期契約を扱う
[#225](https://github.com/KeishiS/marginalis/issues/225)について、先に保存項目と公開契約を決めます。
SQLite schemaを変更する実装は、[#105](https://github.com/KeishiS/marginalis/issues/105)の新しいschema系統と
まとめて`v0.30.0`の候補とします。#225はADR確定後に実装Issueへ分割し、取込元や競合解決に必要な
保存形式を`v0.30.0`へ含めるか、後続版へ分けるかを決定します。

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

[#105](https://github.com/KeishiS/marginalis/issues/105)のSQLite schema番号体系の再開始は、
採用する機能の保存形式を決めた後に実施します。新しい番号体系を導入した直後に別の保存形式を
追加する事態を避けるためです。関係の図はノート間の参照を`note_references`からそのまま読みますが、
引用の線のために`note_citations`表を追加し、v0.22.0でschemaを13へ更新しました。archiveの形式は
変えていないため、`export-archive`と`import-archive`で移れます。

この更新では旧データを移行せず、空の`dataDir`から新しいデータベースを作成します。旧`dataDir`は
自動削除せず、旧実行環境での確認と復旧に使用できるよう別に保管します。

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
