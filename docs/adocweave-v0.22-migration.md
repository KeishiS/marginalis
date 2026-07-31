# AdocWeave 0.22移行判断

> この文書は当時の判断の記録です。ここで決めた「引用を当面拒否する」方針は
> [0.23移行判断](adocweave-v0.23-migration.md)で取り消しました。現在は`cite:`を受理し、
> 書誌ライブラリーで解決して表示します。

## 目的

この文書は、AdocWeave 0.20.0から0.22.0への更新がMarginalisへ与える影響と、引用（citation）の
扱いを決めた理由を記録します。現行の入力方法は[REST API](rest-api.md)、archiveの変換手順は
[NixOSでの運用](nixos.md)を参照してください。

## 固定する依存

MarginalisはAdocWeave package版`0.22.0`を、release tagが指すcommit
`b7b678062bad410235527f752b0f472fe3a5736d`で使用します。Rust toolchainは`1.97.1`を維持します。
CargoとNixの両方で、このcommitと取得内容のハッシュを固定します。

## 解析結果の変更

`cite:[key]`という書き方が、引用として解析されるようになりました。引用とは、本文の中から
書誌情報を指し示す記述です。AdocWeaveは引用そのものを組版せず、citation key（書誌項目を指す
短い識別子）と位置を構造化して返すだけで、実際にどの文献かを解決する処理は利用側アプリの責務です。

0.20.0まで`cite:`は通常の文字列として出力されていたため、この変更は既存の本文の出力を変えます。

0.21.0の変更（`check --format json`の出力形式の統一）はCLIの診断出力に関するもので、Marginalisは
Rust APIを直接使うため影響を受けません。

## 引用を当面拒否する判断

**判断**: Marginalisは`cite:`を含むノートを保存時に拒否します。診断codeは`citation_disabled`、
文言は`citations are not available yet; use the standard bibliography instead`です。

**理由**: 解決した書誌情報をHTMLの描画へ渡す経路がAdocWeave側にまだありません（AdocWeave issue
#313、#314）。この状態で受理すると、利用者には解決されないcitation keyがそのまま表示されるか、
`unresolved_references`の設定によっては何も表示されないノートができます。後から解決経路が
整った時点で表示が変わるため、その途中状態を保存させないことを選びました。

**代替手段**: 従来どおり標準のbibliography（`[[[key]]]`で定義し`<<key>>`で参照する書き方）を
使用できます。この記法の解析は0.22.0でも変わっていません。

**解除の条件**: AdocWeave側で解決済みの引用をHTMLへ渡せるようになった時点で、この拒否を外し、
[書誌ライブラリー](requirements.md)の項目と結び付けます。追跡はMarginalis issue #147で行います。

## 公開契約への影響

AdocWeaveのWASM protocol、CLI引数、Language Server protocolおよび設定schemaに変更はありません。
Rust公開APIには破壊的変更がありますが、Marginalisが使用していたmethodは影響を受けません。

拒否する規則が1つ増えるため、`get_note_profile`が広告する`forbidden_rules`の内容が変わります。
解析規則の変更を利用者が識別できるよう、authoring profile版を8へ更新します。SQLite schema 12と
archiveのnote profile版4は維持します。

## archive形式

archiveは、内容を解析したAdocWeave package版を契約情報として記録します。同じ形式名に異なる
package版を混在させないため、現行形式を`marginalis-archive-12`へ更新します。

`migrate-archive`は、0.20.0を記録したarchive 11とarchive 10を0.22.0で全件再検証し、archive 12を
別ファイルへ出力します。従来の復元経路を維持するため、0.19.0を記録したarchive 9、0.17.0を記録した
archive 8、0.11.0を記録したarchive 7も、同じ方法でarchive 12へ変換できます。入力は変更せず、
既存の出力は上書きしません。

`cite:`を含むノートが旧archiveに入っている場合、そのノートは現行規則を満たさないため変換が
失敗します。該当ノートの本文を標準のbibliographyへ書き換えてから変換してください。
