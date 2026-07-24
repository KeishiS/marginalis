# AdocWeaveへの上流提案

このディレクトリには、AdocWeaveへ提出する英語の提案文を置く。提案するのは、他の利用側
アプリにも有用なAsciiDocライブラリAPIに限る。Marginalis固有のUUID、ACL、SQLite、
ベースURL、KaTeX、コードハイライターは対象に含めない。

## 採用状況

1. [Resolver出力用URL規則とWASMの同一動作](001-resolved-url-policy-and-wasm-parity.md) — RC.3で採用
2. [外部リンクの固定属性](002-external-link-attributes.md) — RC.3で採用
3. [Resolver由来の表示ラベルと未解決表示](003-resolved-reference-display.md) — v0.1.0で採用
4. [警告付き参照解決](004-reference-resolution-notices.md) — 型付き通知をRC.3で採用
5. [ソースブロックの言語規則](005-source-language-policy.md) — RC.3で採用
6. [数式の構造情報API](006-math-projection-api.md) — RC.3で採用
7. [STEM言語の設定](007-stem-language-profile.md) — RC.3で採用
8. [外部ファイルを無効にする設定](008-resource-profile.md) — RC.3で採用
9. [文書属性の定義箇所を取得する公開API](009-public-document-attribute-occurrences.md) —
   v0.6.0で採用

各提案の先頭には採用状況と採用結果を記す。その後に、投稿時の問題、要求API、完了条件、
対象外を英語のまま残す。

この作業環境には、上流リポジトリへ書き込む認証情報とremoteを設定していない。各ファイルは、
GitHub Issueとして投稿できる下書きとして管理する。
