# 004: 安全なHTML、数式およびコード表示

位置付け: ノートを安全に表示するための恒久要件を定める。閲覧用`RenderPolicy`の残作業は
[Issue 027](027-search-reference-and-rendering-projections.md)で管理する。

## 概要

ノート用AsciiDocプロファイルを、安全なHTMLとブラウザー表示へ変換する。

## 対象範囲

- AdocWeaveのHTML許可リストと`RenderPolicy`を基盤とし、入力由来のraw HTML、`style`、
  event handler、SVGおよび危険なURLを出力しない。
- 外部の`http`または`https`リンクを別タブで開くため、固定した`target="_blank"`と
  `rel="noopener noreferrer"`を出力する。
- `include`、外部画像、添付参照およびJavaScriptを拒否する。
- LaTeX STEMは安全な数式ソースとして出力する。KaTeXなどの数式レンダラーは、独立した
  サニタイズおよびCSPの境界で統合する。
- `source`ブロックの言語を`rust`、`typescript`、`javascript`、`json`、`yaml`、`toml`、
  `bash`、`sql`および`text`へ制限する。言語を省略した場合はプレーンテキストとして表示し、
  ノート内のコードを実行しない。
- HTML生成、数式表示またはコード表示に失敗した場合は、エスケープ済みの安全な代替表示を返す。

## 対象外

- Typst数式、Mermaid、外部画像および添付ファイルの表示。

## 完了条件

- 悪意のあるpassthrough、URL難読化、raw HTMLおよび危険なスキームがactive DOMを生成しない。
- 外部リンクが固定属性付きで出力される。
- LaTeXとコードの正常、不正および巨大な入力に対し、安全な表示または代替表示を返す。
- HTML許可リスト、属性、クラスおよびURLポリシーをテストデータで固定する。

## 関連Issue

- [Issue 001](001-adocweave-dependency-and-contract.md)
- [Issue 003](003-note-references-and-resolver.md)
- 閲覧用ポリシー: [Issue 027](027-search-reference-and-rendering-projections.md)
