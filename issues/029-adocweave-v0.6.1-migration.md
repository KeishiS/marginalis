# 029: AdocWeave v0.6.1への移行

## 状態

2026-07-24に着手した。Rust 1.97.1への更新は完了している。AdocWeaveの依存版、
公開API、互換性の識別値、保存形式は未移行である。

当初はv0.4.0、次にv0.5.0を移行先としていた。Marginalisが必要とする文書属性の
定義箇所を取得するAPIはv0.6.0で追加された。文書も修正されたv0.6.1を最終移行先とする。

## 背景

現在の依存定義はAdocWeave v0.5.0のコミットを指す。一方、`marginalis-asciidoc`と
`marginalis-server`の連携コードは、`v0.1.0-rc.3`の公開APIと6種類の契約版を前提とする。
したがって実際の移行範囲は、`v0.1.0-rc.3`から正式版v0.1.0、v0.2.0、v0.3.0、
v0.4.0、v0.5.0、v0.6.0を経たv0.6.1までである。

v0.6系では数値の`CONTRACT_VERSION`が廃止された。Rust、WASM、ブラウザー版、構造情報、
キャッシュは、完全一致するパッケージ版を互換性の識別値として使う。また、
`Analysis::ast()`は非公開になり、公開`Document`、型付きの文書属性API、
選択的な`ProductSet`へ移行した。

現在の保存形式v1は、RC.3の解析仕様とノート入力規則を前提とする。依存だけを更新すると、
保存時の検証、検索・xref用データ、HTML、WASM要求、キャッシュキー、起動時検証で、
異なる版の識別値と解釈が混在する。

## 移行先

- 実装と配布物はAdocWeave `v0.6.1`タグ、注釈付きタグオブジェクト
  `d1621dceb9dc053cda1a7c8ff3163cf624dacc5f`および注釈付きタグが指すコミット
  `2a7ec4f7c2df6104ead9a7285ca13fc364ce8dda`へ固定する。
- v0.6.1では公開APIは変わらない。ただし互換性の識別値は`0.6.1`へ変わるため、
  v0.6.0の成果物と混在させない。
- 移行手順は上流`main`の`docs/current-contract.adoc`と`docs/upgrade-0-2.adoc`から
  `docs/upgrade-0-6.adoc`までを順に適用する。調査時点の`main`はv0.6.1の注釈付きタグが指すコミットと一致する。
- パッケージ版とRust版は、v0.6.1タグの`release-manifest.json`を基準とする。
  必要なRust版は1.97.1である。
- v0.3.0移行ガイドに残る古い契約値が配布目録と異なる場合は、各リリースタグの
  `release-manifest.json`を優先する。

## 決定事項

### 互換性の識別値

- `AdocWeaveContracts`、`PINNED_CONTRACTS`、6種類の数値比較を廃止する。代わりに、
  パッケージ版`"0.6.1"`の完全一致を検証する。
- Rust版では`adocweave::VERSION`を使う。WASMの要求・応答と解析結果、構造情報JSONでは
  `packageVersion`を使う。旧`apiVersion`、`WASM_API_VERSION`、
  `conformanceContractVersion`および`contractVersion`を残さない。
- キャッシュ、保存済みの構造情報、WASM資産、配布物には同じパッケージ版だけを組み合わせる。
  起動時または要求処理時に不一致を検出した場合は、安全側に停止する。

### 保護属性と連携処理

- AsciiDoc本文を直接扱う既存APIは維持する。作成時は`note-id`、`creator-id`、
  `created-at`、`updated-at`をサーバーの値へ置換する。更新時は変更できないメタデータを
  維持し、`updated-at`をサーバー時刻へ置換する。
- v0.6.1の`Analysis::document_attribute_occurrences()`と
  `semantic::{DocumentAttributeOccurrence, DocumentAttributeOperation}`を利用する。記述順、
  重複、空文字の設定、2種類の解除、行・名前・値の位置を公開APIだけで扱う。
  独自のヘッダースキャナーは実装しない。
- `ParseOptions.protected_attributes`と`protected-attribute`診断は、属性の置換そのものには
  使わない。置換後に保存する本文と、再構築時に読み込む本文の追加検証に使う。
- AdocWeaveの解析、属性取得、位置に基づく置換、構造情報、変換入力の作成は
  `marginalis-asciidoc`にまとめる。`marginalis-server`からAdocWeaveへの直接依存を削除する。

### 保存形式

- RC.3以後に本文の解釈と構造情報が変わった。v0.6系では構造情報の識別フィールドも
  `contractVersion`から`packageVersion`へ変わる。このため、保存形式はv2へ更新する方針とする。
- 保存形式v1を維持できるのは、既存の入力規則で受理する全テスト用入力について、本文の解釈と
  永続化する構造情報が同一であると比較試験で証明できた場合だけである。
- v2へ更新する場合は、v1の`dataDir`を暗黙に開かない。起動を安全側に停止し、明示的な移行または
  初期化、全構造情報の再構築、バックアップ、復元、切戻しの手順を提供する。

## 作業計画

### 1. 現在の期待結果を記録する

1. RC.3で受理していたメタデータ、xref、URL、HTML、検索、数式、ソースブロック、WASMの
   入力と期待結果を保存する。
2. v0.1.0からv0.6.0までの各移行ガイドを、影響するテスト用入力と再構築対象へ対応付ける。
3. v0.6.1の変更が文書修正とパッケージ版の更新だけであり、v0.6.0から公開APIの意味を
   変えないことを記録する。

### 2. 依存と供給経路をv0.6.1へ固定する

1. ワークスペースの依存定義と`Cargo.lock`を、注釈付きタグが指すコミット`2a7ec4f...`へ更新する。
2. `ADOCWEAVE_SOURCE_REVISION`、Nixの`outputHashes`、パッケージ内の説明、再現性検査を
   v0.6.1へ揃える。
3. タグ、タグが指すコミット、`Cargo.lock`、Nixハッシュ、`release-manifest.json`の
   `packageVersion = 0.6.1`をCIで照合する。

### 3. Rustの公開APIへ移行する

1. import先を`semantic`、`output`、`preprocess`、`resolution`、`text`の公開APIへ移す。
2. `Analysis::ast()`を`Analysis::document()`へ置き換える。文書木の走査、HTML変換、
   文書プロファイルの検証は、公開された`Document`モデルを使う。
3. メタデータ検証を`document_attribute_occurrences()`へ移す。重複、設定・解除、値、位置を
   Marginalisの検証型へ変換する。
4. 旧6契約型をパッケージ版の型へ置き換え、起動時に不一致を拒否する。

### 4. 保護属性の置換を`marginalis-asciidoc`へまとめる

1. 属性の位置を使い、元の書式を保って置換するAPIを`marginalis-asciidoc`へ追加する。
2. 必須属性の欠落、重複、解除を拒否する。複数箇所は後方から置換し、位置のずれを防ぐ。
3. 作成・更新時に使うサーバー値、置換後の厳格モードの診断、再構築時の不変条件を
   テストで確認する。
4. `marginalis-server`の直接解析コードとAdocWeaveへの直接依存を削除する。

### 5. 構造情報、HTML、WASMを移行する

1. WASM要求の`packageVersion`と`products`を設定し、必要な出力だけを要求する。
   Rust版とWASM版の比較試験では、比較する出力をすべて明示する。
2. 要求、応答、解析結果の概要、構造情報、ブラウザー用資産に記録する`packageVersion`が、
   すべて`0.6.1`で一致することを検証する。
3. `attributeOccurrences`がRust版とWASM版で一致することを確認する。`ProductSet`で選択した
   出力の有無、HTML、診断、検索、xref、URL規則、include通知、スタイルシートの期待結果も比較する。
4. v0.5以前の数値識別フィールドがJSON、キャッシュキー、保存済み出力へ残らないことを検査する。

### 6. 保存形式を確定して全体を検証する

1. テスト結果の差分に基づいて保存形式v2を確定する。v1を維持する場合は、同一性の証明と
   判断理由を記録する。
2. v1の`dataDir`の拒否または明示的な移行、全構造情報の再構築、バックアップ、復元、
   切戻しを試験する。
3. クレート単位の検査とテスト、依存方向、Rust版とWASM版の一致、Nixパッケージ、
   NixOS VMの順で検証する。最後にリリース前の必須検証として`cargo make release-gate`を実行する。

## 完了条件

- AdocWeave v0.6.1のタグ、タグが指すコミット、`Cargo.lock`、Nixハッシュ、
  `release-manifest.json`が一致する。
- Rust版、WASM、構造情報、キャッシュ、配布物が同じパッケージ版`0.6.1`を使い、
  不一致を安全側に拒否する。
- `marginalis-server`はAdocWeaveへ直接依存せず、属性検証と本文の置換を
  `marginalis-asciidoc`の公開API経由で行う。
- 保護属性の置換、厳格モードの診断、重複、空文字の設定、2種類の解除、位置を
  テストで検証する。
- RC.3からv0.6.1までの保存、検索、xref、HTML、WASMの差分を期待結果として記録する。
- 保存形式の版、再構築、バックアップ、復元、切戻し、既存環境の扱いが
  文書と実装で一致する。
- `cargo make release-gate`が成功する。

## 現在地（2026-07-24）

### 完了済み

- flakeへoxalica `rust-overlay`を追加し、devShellと`buildRustPackage`をRust 1.97.1へ切り替えた。
- 依存先のリビジョンと`Cargo.lock`は一度v0.5.0のコミット`48b8c28`へ更新した。
  v0.6.1への再更新が必要である。
- [上流提案009](upstream/009-public-document-attribute-occurrences.md)はv0.6.0で採用され、
  Rust版とWASM版から文書属性の定義箇所を取得できるようになった。

### 未完了のAPI差分

- 古いルートモジュールのパスと`Analysis::ast()`が残っている。公開APIと
  `Analysis::document()`への移行が必要である。
- `ADOCWEAVE_SOURCE_REVISION`とNix `outputHashes`はRC.3のままである。
- 旧6契約型、`WASM_API_VERSION`および`api_version`が残り、パッケージ版による識別へ移行していない。
- `marginalis-server`がAdocWeaveへ直接依存し、非公開になった旧属性型を使用している。
- WASMのテストは`ProductSet`を指定せず、旧来の全出力を含む応答と数値版を前提としている。

### 上流機能の充足

参照解決後の表示文字列、URL規則、型付き通知、ソースコードの言語規則、数式とSTEMの
構造情報、外部リソースの制御、文書属性の定義箇所を取得するAPIは、現行のAdocWeaveで
提供される。現時点でMarginalisの移行を阻む上流機能不足はない。
