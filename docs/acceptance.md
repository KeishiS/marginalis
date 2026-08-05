# 受入基準

この文書は、各バージョンで繰り返す恒久的な受入基準を定めます。実施日、結果、証跡は
版別の結果へ記録し、この文書へ混在させません。要件ごとの自動検証は
[要件と検証の対応表](traceability.md)を参照してください。

## 自動検証

Pull Requestと公開前の`release-gate`で次を確認します。

- React・TypeScriptの整形、静的解析、型検査、単体試験、依存監査、配布ビルド
- Rustの整形、静的解析、単体・結合試験、依存監査
- OpenAPIとTypeScript生成物の差分、RESTルーター、MCPツール定義の契約
- 所有者、閲覧者、編集者、対象外利用者の表示・操作・情報非漏洩
- ノート、ACL、参照、削除状態、revisionのtransaction整合性
- AdocWeaveの属性環境、コードブロック・数式HTML、対応する旧archiveから現行archiveへの全件再検証
- Kanidm、内蔵OAuth、TLS、サブパス、OIDC、MCP、バックアップ、削除、障害診断
- NixOS配備、空のデータベースの初期化、旧schemaの拒否

## 人手受入

1. ログイン後にノートを作成し、一覧、閲覧、編集、プレビュー、保存を確認します。一覧の
   絞り込み条件とページがURLから復元され、閲覧や編集の後も維持されることを確認します。
2. 編集中に未保存、保存中、保存成功、保存失敗が区別されることを確認します。
   `この結果はxref:note:<ノートID>[参照]`を入力すると、保存可能なまま空白を求める警告が表示
   され、入力位置へ移動できることを確認します。空白を追加すると警告が消えること、プレビュー
   更新の失敗後も最後に成功した表示が残ることも確認します。
3. revision競合で編集開始時点、編集中、現在保存済みの内容を比較し、最新revisionへ再保存します。
4. 目次、表、引用、リスト、すべてのコードブロックの行番号、題名・言語・開始行、インライン数式、
   ブロック数式が判読でき、横幅を超える表、コード、数式を内容ごとにスクロールできることを
   確認します。
5. 書誌ライブラリーへ文献を登録し、本文へ`cite:[citation key]`と書いて保存します。閲覧画面で
   引用が著者と発行年で表示され、末尾に生成された参考文献節の項目へ移動できること、項目から
   本文の引用位置へ戻れることを確認します。同じ文献を複数回引用しても項目が一つであること、
   登録していないcitation keyでは保存できるものの警告が表示されることも確認します。
6. 所有者が閲覧者と編集者をACLへ追加し、それぞれの表示と操作範囲を確認します。共有相手の画面でも
   引用が作成者のライブラリーで解決した表示になることを確認します。
7. 所有するノートを閲覧画面から削除し、通常の一覧から消えて削除済みノートの画面に現れることを
   確認します。題名、削除日時、完全削除予定、revisionを確認して復元し、通常の一覧と閲覧画面へ
   戻ることを確認します。共有相手には削除操作と削除済みノートの情報が表示されないことも確認します。
8. 数式マクロ設定へ`argmax`と`bm`の定義例を追加し、`\argmax_{x \in S} f(x)`と`\bm{x}`が
   所有するノートの閲覧とプレビューで組版されることを確認します。共有相手の画面でも所有者の
   定義が使われることを確認します。
9. Web UI、汎用REST API、MCPからそれぞれノートを作成し、作成経路が`web`、`rest`、`mcp`として
   表示されることを確認します。所有するノートを確認済みにした後は`reviewed`となり、本文またはACLを
   更新すると`pending`へ戻ること、共有相手には確認者の識別情報が表示されないことも確認します。
10. 対象外利用者にノートと関連情報が表示されず、直接指定した操作も`404`になることを確認します。
11. ChatGPT、Claude Code、Codex CLIからMCPへ接続し、RESTと同じ閲覧・更新結果になることを
   確認します。最初は`notes:read`だけに同意し、更新と削除を要求した時点で不足scopeが段階的に
   提示されること、追加認可を拒否しても元の閲覧権限が維持されることも確認します。登録、認証、
   作成、更新、ACL、削除の順序は
   [MCPクライアントの接続後受入](mcp.md#接続後の受入)に従います。
12. MCPクライアントの接続解除でrefresh tokenとauthorization grantを取り消し、再認可なしでは
   refreshできないこと、既発行access tokenが拒否されるまでの時間を確認します。
13. アーカイブを本番から隔離した空のデータベースへ復元し、ノート、ACL、参照、削除状態、revision、
    作成経路、人手確認記録、数式マクロ設定が一致することを確認します。
14. `export-documents`で書き出した文書書庫を変更せずに空のデータベースへ取り込み、revisionと
    人手確認状態が維持されることを確認します。次に、書庫内の`.adoc`本文またはACLを変更して
    取り込み、revisionが増えて確認待ちになることを確認します。

## 版別結果の記録

版別結果は`docs/acceptance-results/`へ保存し、各項目に次を記録します。

- 実施日
- `成功`、`失敗`、`未実施`のいずれか
- GitHub Actions、Pull Request、運用記録などの証跡
- 失敗または未実施の場合のリリース判断

秘密情報、トークン、Cookie、実際のノート本文は記録しません。`成功`には証跡を必須とし、
`未実施`を自動試験の成功で置き換えません。

## 記録

- [v0.30.1](acceptance-results/v0.30.1.md)
- [v0.30.0](acceptance-results/v0.30.0.md)
- [v0.29.0](acceptance-results/v0.29.0.md)
- [v0.28.1](acceptance-results/v0.28.1.md)
- [v0.28.0](acceptance-results/v0.28.0.md)
- [v0.27.0](acceptance-results/v0.27.0.md)
- [v0.26.1](acceptance-results/v0.26.1.md)
- [v0.26.0](acceptance-results/v0.26.0.md)
- [v0.25.1](acceptance-results/v0.25.1.md)
- [v0.25.0](acceptance-results/v0.25.0.md)
- [v0.24.0](acceptance-results/v0.24.0.md)
- [v0.23.0](acceptance-results/v0.23.0.md)
- [v0.22.0](acceptance-results/v0.22.0.md)
- [v0.21.0](acceptance-results/v0.21.0.md)
- [v0.20.0](acceptance-results/v0.20.0.md)
- [v0.19.0](acceptance-results/v0.19.0.md)
- [v0.18.0](acceptance-results/v0.18.0.md)
- [v0.17.0](acceptance-results/v0.17.0.md)
- [v0.16.1](acceptance-results/v0.16.1.md)
- [v0.16.0](acceptance-results/v0.16.0.md)
- [v0.15.0](acceptance-results/v0.15.0.md)
- [v0.14.0](acceptance-results/v0.14.0.md)
- [v0.13.0公開停止候補](acceptance-results/v0.13.0.md)
- [v0.12.0](acceptance-results/v0.12.0.md)
- [v0.11.0](acceptance-results/v0.11.0.md)
- [v0.10.0](acceptance-results/v0.10.0.md)
- [v0.9.0](acceptance-results/v0.9.0.md)
- [v0.8.0](acceptance-results/v0.8.0.md)
- [v0.7.0](acceptance-results/v0.7.0.md)
