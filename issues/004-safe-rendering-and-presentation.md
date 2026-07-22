# 004: 安全なHTML、数式、コード表示

## 目的

ノート用AsciiDocプロファイルを、安全なHTMLとブラウザ表示へ変換する。

## 範囲

- AdocWeaveのHTML allowlistと`RenderPolicy`を基盤に、入力由来のraw HTML、`style`、
  event handler、SVGおよび危険なURLを出力しない。
- 外部`http`／`https`リンクを別タブで開くため、固定の`target="_blank"`と
  `rel="noopener noreferrer"`を出力する拡張を実装し、HTML契約へ追加する。
- `include`、外部画像、添付参照およびJavaScriptを拒否する。
- LaTeX STEMはまず安全な数式ソースとして出力し、KaTeX等の数式レンダラーは独立した
  サニタイズ・CSP境界で統合する。
- source block言語を`rust`、`typescript`、`javascript`、`json`、`yaml`、`toml`、`bash`、
  `sql`および`text`へ制限し、未許可の言語を保存時に拒否する。言語なしはプレーンテキスト
  として表示し、ノート本文のコードを実行しない。
- renderer、数式表示およびコード表示の失敗時に、エスケープ済みの安全なfallbackを表示する。

## 範囲外

- Typst数式、Mermaid、外部画像、添付ファイルの表示。

## 完了条件

- 悪意あるpassthrough、URL難読化、raw HTMLおよび危険スキームがactive DOMを生成しない。
- 外部リンクが固定属性付きで出力される。
- LaTeXとコードの通常・不正・巨大入力に対して、安全な表示またはfallbackを返す。
- HTML allowlist、属性、class、URL policyをfixtureで固定する。

## 依存関係

- 001
- 003
