# AdocWeave への上流提案

このディレクトリでは、他のホストにも有用な AsciiDoc ライブラリ API として上流へ提案する
Issue 本文の下書きを管理する。本アプリ固有の業務要件は持ち込まない。UUID・ACL・SQLite・
ベース URL の決定、KaTeX、コードハイライターは提案対象に含めない。

1. [Resolver 出力用 URL policy と WASM 整合](001-resolved-url-policy-and-wasm-parity.md) — RC.3 で採用
2. [外部リンクの固定属性](002-external-link-attributes.md) — RC.3 で採用
3. [Resolver 由来の表示ラベルと未解決表示](003-resolved-reference-display.md) — v0.1.0 で採用
4. [警告付き参照解決](004-reference-resolution-notices.md) — typed notice は RC.3 で採用
5. [source block 言語ポリシー](005-source-language-policy.md) — RC.3 で採用
6. [数式 projection API](006-math-projection-api.md) — RC.3 で採用
7. [STEM 言語プロファイル](007-stem-language-profile.md) — RC.3 で採用
8. [リソース無効化プロファイル](008-resource-profile.md) — RC.3 で採用
9. [文書属性occurrenceの公開query](009-public-document-attribute-occurrences.md) —
   v0.6.0で採用

この作業環境には、上流リポジトリへの書き込み認証情報と remote を設定していない。そのため、
各ファイルは GitHub Issue としてそのまま投稿できる下書きとして管理する。
