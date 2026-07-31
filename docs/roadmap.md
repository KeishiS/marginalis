# ロードマップ

この文書は、公開済み機能の履歴ではなく、現在の作業、次に必要な判断、保留中の候補を示します。
公開済みの利用者影響は[変更履歴](../CHANGELOG.md)、個別の完了条件は
[GitHub Issues](https://github.com/KeishiS/marginalis/issues)を参照してください。

## 現在地

`v0.19.0`は2026-07-31に公開しました。AdocWeave 0.23.0への更新に合わせて本文の`cite:[key]`を
書誌ライブラリーで解決し、引用表示と参考文献一覧を組み立てます。解決にノート作成者の
ライブラリーを使う理由は
[引用はノート作成者の書誌ライブラリーで解決する](adr/0006-引用はノート作成者の書誌ライブラリーで解決する.md)に
記録しています。

`v0.20.0`は公開準備中です。保存しているノートと書誌情報を、AsciiDocファイルとCSL-JSONの
`tar.xz`書庫として書き出す`export-documents`と、それを再検証して取り込む`import-documents`を
追加します。書庫のmanifestは版情報を持ち、取り込む側が移行の要否を判断できます。版別の確認結果は
[v0.20.0受入結果](acceptance-results/v0.20.0.md)へ記録します。

引用スタイルの選択と、外部の文献管理ツールとの同期は
[#147](https://github.com/KeishiS/marginalis/issues/147)で引き続き扱います。

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
