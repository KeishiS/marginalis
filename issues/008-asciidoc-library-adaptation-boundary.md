# 008: 一般AsciiDocライブラリへの適用アダプタ

## 目的

一般的なAsciiDocのパーサー・変換ライブラリが提供する標準構文と公開拡張点を組み合わせ、
本アプリのノートプロファイルを適用する。アプリ固有の文法、独自インラインマクロまたは
恒久的なライブラリforkは導入しない。

## 前提

- `xref:note:<note-uuid>[<label>]`および
  `xref:note:<note-uuid>#<anchor-id>[<label>]`は、標準の`xref`マクロとURI scheme形式の
  参照先である。`note:`単独の独自マクロは受理しない。
- 文書タイトル、属性、明示アンカー、`stem`、`[source,<language>]`および通常のリンクは、
  いずれも標準AsciiDoc構文として解析する。
- DB、ACL、ファイルI/O、時刻、HTTPおよびブラウザDOMはライブラリの外側に置く。

## 具体例

### ノート参照

入力は次の標準AsciiDocである。

```asciidoc
xref:note:01800000-0000-7000-8000-000000000001#definition[定義]
```

| 担当 | 処理 |
| --- | --- |
| AsciiDocライブラリ | 標準`xref`として、`scheme = note`、`locator = 01800000-0000-7000-8000-000000000001`、`anchor = definition`、labelおよび原文範囲をASTへ格納する。DBやURLを参照しない。 |
| 本アプリのアダプタ | UUIDv7を検証し、現在の利用者のACLで対象とアンカーを照会する。許可された場合は`<Base URL>/note/01800000-0000-7000-8000-000000000001#definition`を解決結果として作る。 |
| AsciiDocライブラリ | 入力された解決結果だけを用いて`<a href="…">定義</a>`を描画する。 |

対象が閲覧不能または不在なら、アダプタは同じ一般的な「未解決」結果を返す。ライブラリはhrefを
出力せず、対象ID、タイトル、ACL状態を新たに表示しない。アンカーだけが不在なら、アダプタは
`<Base URL>/note/<UUID>`を解決結果として返し、アプリはフォールバック状態と閲覧用warningを
記録する。空labelは許可された解決先のタイトルへ置換し、未解決時にはノートIDを表示しない。

### 外部リンク

```asciidoc
https://example.com/[外部サイト]
```

ライブラリは通常リンクを解析し、アプリは`http`／`https`だけを許可するURL policyを設定する。
ライブラリの描画APIまたは汎用描画アダプタは、許可された外部リンクへ固定の
`target="_blank" rel="noopener noreferrer"`を付ける。`javascript:`や制御文字を含むURLは、
アプリがHTML後処理で除去するのではなく、描画前のURL policyで拒否する。

### 数式とコードブロック

```asciidoc
:stem: latexmath

stem:[x^2 + y^2 = z^2]

[source,rust]
----
let answer = 42;
----
```

ライブラリは標準`stem`とsource blockを構造化して保持する。本アプリは`latexmath`だけを許可し、
数式ソースを安全な数式表示境界へ渡す。source blockでは言語名をallowlistで検証してから
ハイライターへ渡す。いずれも独自の数式マクロや実行可能なコードマクロをパーサーへ追加しない。

## 必要なライブラリ機能とホスト側アダプタ

| 分類 | 一般ライブラリに求める公開機能 | 本アプリの適用 |
| --- | --- | --- |
| 解析 | 文書ヘッダ、属性、ブロック、インライン、アンカーおよびUTF-8 byte rangeを持つAST・診断 | `note-id`等をAST後に検証し、保存時strict／編集時permissiveの診断へ変換する。 |
| 参照 | `xref`の参照先をscheme、locator、anchorに分解し、範囲付きで列挙するAPI | schemeが`note`である参照だけをUUIDv7検証し、各位置を投影へ保存する。 |
| 参照解決 | 参照先の解決結果または失敗を、解析と別に描画へ渡すResolver／render input API | SQLiteとACLで解決し、`<Base URL>/note/<uuid>`、Resolver由来の表示ラベルおよびnoticeを渡す。権限なしは対象不在と同じ失敗へ畳み込む。 |
| URL policy | scheme allowlist、control文字・難読化拒否、入力URLとResolver出力URLを区別できる安全なポリシー | 入力には`http`／`https`だけを許可し、Resolver出力には検証済みの絶対HTTPS URLだけを許可する。 |
| HTML描画 | HTML allowlist、属性のエスケープ、リンク属性の固定または描画フック | 外部リンクだけへ`target="_blank" rel="noopener noreferrer"`を固定し、内部リンクには付けない。raw HTML、style、event handler、SVGを出力させない。 |
| リソース | `include`、画像、添付および外部リソースを構文・解決段階で無効化する設定 | include、外部画像、添付参照および代理取得を無効化し、解析中のI/Oを発生させない。 |
| STEM | 標準`stem`構文をASTまたは安全にエスケープされた出力へ保持するAPI | `:stem: latexmath`だけを許可し、KaTeX等は別のサニタイズ済み表示境界で処理する。 |
| ソースコード | source blockの言語と内容を分離し、言語classを制限できる描画API | 許可した言語classだけをハイライターへ渡し、コードを実行しない。 |
| 投影 | 可読テキスト、見出し、コード、数式、参照を同一解析revisionから取得するAPI | 検索、グラフ、逆参照を再解析なしでSQLiteへ保存する。 |
| 整形・LSP・WASM | Formatter、位置変換、同一プロファイルで動くLSP/WASM境界 | 保存検証、編集診断、ブラウザプレビューで同じ規則とfixtureを使う。 |

## 実装方針

- アダプタは、ライブラリの公開ASTを再実装せず、解析結果を受け取って検証・投影・解決する。
- `xref:note:`の意味づけはResolverに置く。パーサーへ`note`専用マクロを追加しない。
- HTMLは、同一解析revisionに対応する解決結果だけを入力として描画する。文字列置換による
  HTML後処理でリンク意味を実装しない。
- Resolver出力用URL policy、外部リンク固定属性、表示ラベル・noticeが公開APIで不足する場合は、
  最小の一般機能として上流へ提案する。上流対応までの一時実装も、アプリ固有の文法変更ではなく、
  明示的な描画アダプタに限定する。

## 範囲外

- `note:`単独マクロ、独自ブロックマクロ、独自数式記法の導入。
- ライブラリが解釈するファイルパス、DBキー、ACLまたはHTTP URLへのアプリ固有意味付け。
- 外部URLの取得、コード実行、ブラウザDOMへの無検証HTML挿入。

## 完了条件

- 採用ライブラリの公開APIだけで、002から007の機能を実装できることをAPI対応表で確認する。
- 各アダプタが標準AsciiDoc入力から同じAST、診断、HTMLおよび投影をnativeとWASMで生成する。
- 追加が必要な一般機能は、理由、最小API、セキュリティfixtureおよび上流提案先を記録する。
- アプリ固有のパーサー拡張なしで、ノート参照、数式、コード、安全なリンク表示を実現する。

## 依存関係

なし。
