# AdocWeave 0.11移行判断

## 目的

AdocWeave 0.10.1から0.11.0への移行で、Marginalisの保存可否、安定診断、HTMLおよび
URL安全性を維持しながら、責務別の公開設定へ移行します。

## 固定入力比較

0.10.1で固定した既存試験を0.11.0でも実行し、次の結果を確認しました。

| 固定入力 | 0.10.1 | 0.11.0 | 判断 |
| --- | --- | --- | --- |
| `snake_&#96;code&#96;`と日本語・絵文字内のmonospace | 同じHTML断片 | 同じHTML断片 | 保存・描画規則の維持 |
| header指定のない表 | 暗黙のheader | 暗黙のheader | 表規則の維持 |
| `[%noheader]`の表 | headerなし | headerなし | 表規則の維持 |
| 日本語本文と未許可source言語 | UTF-8 byte位置付き拒否 | 同じ安定codeと位置で拒否 | REST・MCP診断の維持 |
| include、passthrough、外部Resource、外部xref | 拒否 | 拒否 | note profileの維持 |
| 不正schemeと相対URL | 拒否 | 拒否 | 執筆時・描画時URL規則の維持 |

0.11.0で不正percent escapeとnetwork-path URLの拒否が明確化されました。これらは従来から
許可対象ではなく、安全性の明確化として扱います。

## 設定責務

- 保存時解析: Strict modeと解析上限を持つ`AnalysisOptions`
- 保存時診断: 執筆時URLを検査する`DiagnosticProfile`と`AuthoredUrlPolicy`
- HTML描画: active URLを出所別に検査する`RenderPolicy`と`ActiveUrlPolicy`
- HTML出力: 描画後に検査する`OutputLimits`

相対URLは`AuthoredUrlPolicy`の0.11.0既定値に依存せず、明示的に無効化します。描画時も
執筆由来、解決済み相対URL、root相対URLおよびdata URLを明示的に無効化します。

## lintと版の判断

`asciidoc-file-link`と`non-asciidoc-xref`は0.11.0の既定警告として有効にします。Marginalisは
AdocWeaveのerrorと独自のnote profile違反だけを保存拒否へ写像するため、この警告追加は保存可否や
公開する安定診断codeを変更しません。任意規則の`macro-boundary`は有効化しません。

以上からnote profile版`1`を維持します。SQLiteへ解析cacheを保存せず、archiveの構造も変わらないため、
SQLite schema 4と`marginalis-archive-3`も維持します。archive identityのAdocWeave package版だけを
`0.11.0`へ更新し、0.10.1のarchiveは暗黙に読み替えず拒否します。
