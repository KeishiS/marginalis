# ロードマップ

この文書は、公開済み機能の履歴ではなく、現在の作業、次に必要な判断、保留中の候補を示します。
公開済みの利用者影響は[変更履歴](../CHANGELOG.md)、個別の完了条件は
[GitHub Issues](https://github.com/KeishiS/marginalis/issues)を参照してください。

## 現在地

`v0.11.0`は2026-07-29に公開しました。MCPのAuthorization ServerにはAuth0を使用し、
MarginalisはProtected Resourceの責務だけを担います。採用理由は
[ADR 0001](adr/0001-auth0をmcpのauthorization-serverに採用.md)を参照してください。

現在は`v0.12.0`のリリース候補を準備しています。対象は次の三つです。

- [#104](https://github.com/KeishiS/marginalis/issues/104): 未対応SQLite schemaを拒否した直後も、
  データベースを変更せず実際の版を安定して報告する診断
- [#102](https://github.com/KeishiS/marginalis/issues/102): MCPクライアント向けの参考文献付き
  AsciiDoc例と、書誌情報を推測しない執筆指針
- [#103](https://github.com/KeishiS/marginalis/issues/103): 保存を妨げないAdocWeave診断の
  REST契約と編集画面

SQLite schema 11と`marginalis-archive-8`は変更しません。MCPとOpenAPIが示すnote profileは、
参考文献例と入力診断を含む版5へ更新します。archive内のノート受理規則は版4を維持します。

## v0.12.0の完了条件

- Pull Requestの通常CIと、`main`へマージしたコミットに対するリリースゲートの成功
- 配備したWeb UIでの警告表示、入力位置への移動、警告を含む保存、修正後の警告消去
- ChatGPT、Claude Code、Codex CLIでの入力規則取得、参考文献付きノートの作成と取得
- 旧schemaの起動拒否後における反復診断と、データベース、WAL、SHMの不変性
- `marginalis-archive-8`を本番から隔離した空のschema 11へ復元する確認

実施結果と証跡は[v0.12.0受入結果](acceptance-results/v0.12.0.md)へ記録します。

## 次の編集機能

v0.12.0の後は、[#99](https://github.com/KeishiS/marginalis/issues/99)の長文編集画面を進めます。
#99は追跡用Issueとして残し、着手前に次の独立した実装単位へ分割します。

- 編集、分割、プレビューの表示方式と画面幅への対応
- キーボード操作、IME、入力補助
- スクロール同期と診断位置への移動
- 大規模文書の性能、アクセシビリティ、ブラウザー受入

v0.12.0で追加する診断契約と位置移動を、この編集画面でも維持します。

## 検索と関係グラフの評価

[#19](https://github.com/KeishiS/marginalis/issues/19)では、固定した再発見例を使って単純一致、
SQLite FTS5、trigram、意味検索を比較します。評価で方式を選んだ後に、実装Issueを作成します。

[#22](https://github.com/KeishiS/marginalis/issues/22)は#19の後に実施します。検索と直接参照だけで
固定例を解決できる場合は、関係グラフを実装しません。グラフが必要と判断した場合だけ、
認可後のノートと関係だけを返す実装Issueを作成します。

## 次の破壊的更新

[#105](https://github.com/KeishiS/marginalis/issues/105)のSQLite schema番号体系の再開始は、
#19と#22の判断、および採用する機能の保存形式を決めた後に実施します。新しい番号体系を導入した
直後に検索用の保存形式を追加する事態を避けるためです。

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
