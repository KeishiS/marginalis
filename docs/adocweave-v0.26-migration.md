# AdocWeave 0.26移行判断

## 目的

この文書は、AdocWeave 0.23.0から0.26.0への更新がMarginalisへ与える影響を記録します。0.24.0、
0.25.0、0.26.0の3版分をまとめて取り込みます。現行の入力方法は[REST API](rest-api.md)、
archiveの変換手順は[NixOSでの運用](nixos.md)を参照してください。

前回の更新は[AdocWeave 0.23移行判断](adocweave-v0.23-migration.md)にあります。

## 固定する依存

MarginalisはAdocWeave package版`0.26.0`を、release tagが指すcommit
`3ff7cf5081741ac3c4082ffa7648937770cb1e8e`で使用します。Rust toolchainは`1.97.1`を維持します。
CargoとNixの両方で、このcommitと取得内容のハッシュを固定します。

## Rust公開APIは変わらない

Marginalisが呼び出す型と関数は、3版を通して変わりません。`AnalysisOptions`、`RenderPolicy`、
`RenderInputs`、`Analysis`の使い方はそのままです。置き換えた呼び出しはありません。

0.24.0はCLIとLanguage Serverの終了コードを失敗の理由ごとに分ける破壊的変更を含みますが、
MarginalisはAdocWeaveをRustのライブラリとして組み込むため、終了コードを読みません。0.25.0は
`adocweave_config::ProjectScopeId`を追加しましたが、これはCLIとLanguage Serverがプロジェクト
範囲を識別するための型で、Marginalisは使いません。

## 設定fileの上限は関係しない

0.24.0は設定fileの読み込みに1 MiBの上限を設けました。Marginalisは設定fileを読みません。
`crates/marginalis-asciidoc/src/configuration.rs`が`AnalysisOptions`と`RenderPolicy`を
`NOTE_POLICY`から直接組み立てます。上限を超える設定fileという状況が起きません。

## URLの扱いはより厳しい側へ寄る

0.25.0は、`javascript`と`vbscript`のURLを、利用側が許可schemeへ加えても出力しなくなりました。
browserがcodeとして実行するURLだからです。

Marginalisが許可するschemeは`http`と`https`だけで（`NOTE_POLICY.allowed_url_schemes`）、
以前からこれらのURLを出力しません。出力は変わらず、AdocWeave側にもう一段の防御が入ります。

## directiveの属性記法は受理範囲を変えない

0.25.0は、`include`と`ifeval`のdirectiveが`\{name}`をエスケープとして読むようにしました。
以前は属性として展開しており、同じ記法が本文と条件式で違う意味を持っていました。

Marginalisはどちらのdirectiveも受理しません。`include`は`include_directive_disabled`として
拒否します。`ifeval`はマクロとして解釈された結果、許可しないURL schemeとして拒否します。
したがってノートの受理範囲と描画結果は変わりません。この状態は
`directives_that_changed_escape_handling_stay_rejected`で固定しています。

本文中の`\{name}`は以前と同じく、属性の展開を打ち消した文字列として受理します。0.25.0が
変えたのはdirectiveの中の解釈だけです。

## ファイル数の上限は以前から同じ

0.25.0は`resources.max-files`の上限を、macOSとWindowsでも読み込み経路に適用するようにしました。
Marginalisが対象とするのはLinuxで、以前から上限が働いています。

## Language Serverの改善は使わない

0.24.0から0.26.0までの主な変更はLanguage Serverの応答性です。打鍵ごとの処理をワークスペースの
規模に依存しないようにし、ワークスペース走査を要求へ応答するthreadの外へ移し、`prepareRename`
を追加しました。MarginalisはLanguage Serverを使いません。

## 公開契約への影響

`get_note_profile`とOpenAPIが公開する`adocweave_package_version`が`0.23.0`から`0.26.0`へ
変わります。入力規則そのものは変わらないため、`profile_version`は上げません。

archiveへ記録する`adocweave_package_version`も`0.26.0`になります。`0.23.0`を記録した既存の
archiveは現行契約として受理されなくなるため、移行元の契約へ
`("marginalis-archive-13", "0.23.0", 5)`を加えました。`migrate-archive`で変換できます。
本文の書き換えは不要で、全ノートを現行の規則で再検証するだけです。

## 引用の番号付けは利用側が決める

0.24.0以降の「既知の制約」に、次の記述が加わりました。

> 引用の解決結果は文書全体の並べ替えを行いません。番号付きの引用styleで通し番号を振る場合は、
> 利用側アプリが出現順を見て文字列を決めてください。出現順は公開projectionの`citations`から
> 取得できます。

番号で示す引用スタイルを追加する作業は、この前提のうえで進めます。Marginalisは既に
`Analysis::citations()`から出現順を取得しており、追加の経路は要りません。
