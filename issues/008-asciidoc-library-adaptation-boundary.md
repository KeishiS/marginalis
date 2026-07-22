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

## 必要なライブラリ機能とホスト側アダプタ

| 分類 | 一般ライブラリに求める公開機能 | 本アプリの適用 |
| --- | --- | --- |
| 解析 | 文書ヘッダ、属性、ブロック、インライン、アンカーおよびUTF-8 byte rangeを持つAST・診断 | `note-id`等をAST後に検証し、保存時strict／編集時permissiveの診断へ変換する。 |
| 参照 | `xref`の参照先をscheme、locator、anchorに分解し、範囲付きで列挙するAPI | schemeが`note`である参照だけをUUIDv7検証し、各位置を投影へ保存する。 |
| 参照解決 | 参照先の解決結果または失敗を、解析と別に描画へ渡すResolver／render input API | SQLiteとACLで解決し、`<Base URL>/notes/<uuid>`を渡す。権限なしは対象不在と同じ失敗へ畳み込む。 |
| URL policy | scheme allowlist、control文字・難読化拒否、root-relative URLを許可できる安全なポリシー | `http`／`https`と、アプリが生成した`/…/notes/<uuid>`だけを許可する。入力からroot-relative URLを一般許可しない。 |
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
- root-relative URL許可や外部リンク固定属性が公開APIで不足する場合は、最小の一般機能として
  上流へ提案する。上流対応までの一時実装も、アプリ固有の文法変更ではなく、明示的な
  描画アダプタに限定する。

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
