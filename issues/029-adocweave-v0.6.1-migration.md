# 029: AdocWeave v0.6.1への移行

状態: 2026-07-24着手。Rust 1.97.1へのツールチェーン更新は完了。依存、公開API、
package identityおよびdata formatの移行は未完了。

> 当初はv0.4.0、次にv0.5.0を対象としていたが、Marginalisが必要とする公開文書属性
> occurrence queryがv0.6.0で採用され、文書同期を修正したv0.6.1が公開されたため、
> 最終移行先をv0.6.1へ更新する。

## 問題

現在の依存定義はAdocWeave v0.5.0 commitを指す一方、`marginalis-asciidoc`と
`marginalis-server`の連携コードは`v0.1.0-rc.3`の公開APIと6種類の契約versionを前提とする。
したがって実際の移行区間は、`v0.1.0-rc.3`からstable v0.1.0、v0.2.0、v0.3.0、
v0.4.0、v0.5.0、v0.6.0を経たv0.6.1までである。

v0.6系では数値の`CONTRACT_VERSION`を廃止し、完全一致するpackage SemVerをRust、WASM、
browser、projectionおよびcacheの公開互換性identityとする。さらに`Analysis::ast()`を非公開化し、
公開`Document`、typed attribute occurrence queryおよび選択的`ProductSet`へ再編した。

現在のdata format v1は、RC.3の解析契約とノートprofileを正本解釈の前提とする。依存だけを
更新すると、保存時検証、検索・xref投影、HTML、WASM要求、cache keyおよび起動時検証が
異なるidentityと意味を混在させる。

## 上流の正本

- 実装と配布物はAdocWeave `v0.6.1` tag、annotated tag object
  `d1621dceb9dc053cda1a7c8ff3163cf624dacc5f`およびpeeled commit
  `2a7ec4f7c2df6104ead9a7285ca13fc364ce8dda`へ固定する。
- v0.6.1は公開APIを変更しないpatch releaseである。package identityだけは`0.6.1`へ変わるため、
  v0.6.0成果物と混在させない。
- 移行手順は上流`main`の`docs/current-contract.adoc`と`docs/upgrade-0-2.adoc`から
  `docs/upgrade-0-6.adoc`までを順に適用する。調査時点の`main`はv0.6.1のpeeled commitと一致する。
- package versionとRust versionはv0.6.1 tagの`release-manifest.json`を機械可読な正本とする。
  Rustの要求versionは1.97.1である。
- v0.3.0移行ガイドの更新手順に残る旧contract値の不整合は、各release tagのmanifestを優先する。

## 採用方針

### package identity

- `AdocWeaveContracts`、`PINNED_CONTRACTS`および6種類の数値比較を廃止し、採用package version
  `"0.6.1"`の完全一致を検証する単一のidentityへ置き換える。
- nativeでは`adocweave::VERSION`、WASM request／responseとparse summaryでは`packageVersion`、
  projection JSONでも`packageVersion`を用いる。旧`apiVersion`、`WASM_API_VERSION`、
  `conformanceContractVersion`および`contractVersion`を残さない。
- cache、保存済みprojection、WASM資産およびrelease assetは同じpackage versionだけを組み合わせる。
  起動時または要求処理時の不一致はfail closedする。

### 保護属性とadapter境界

- raw AsciiDoc APIの既存契約を維持する。createでは`note-id`、`creator-id`、`created-at`、
  `updated-at`を、updateでは不変metadataと新しい`updated-at`をサーバの正本値へ置換して保存する。
- v0.6.1の`Analysis::document_attribute_occurrences()`と
  `semantic::{DocumentAttributeOccurrence, DocumentAttributeOperation}`を利用する。source順、
  重複、空文字set、2形式のunsetおよび行・name・value rangeを公開APIだけで扱い、
  独自header scannerは実装しない。
- `ParseOptions.protected_attributes`と`protected-attribute`診断は置換を代替せず、置換後の
  保存sourceと再構築時sourceに対する追加検証として利用する。
- AdocWeaveの解析、属性query、range置換、projectionおよびrender入力構築を
  `marginalis-asciidoc`へ集約する。`marginalis-server`からAdocWeaveへの直接依存を削除する。

### data format

- RC.3以後に正本解釈とprojectionが変わり、v0.6系ではprojection identity fieldも
  `contractVersion`から`packageVersion`へ変わるため、data format v2を既定の判断とする。
- data format v1を維持できるのは、既存profileで受理する全fixtureについて、正本解釈と
  Marginalisが永続化するprojectionの同一性を比較試験で証明できた場合だけとする。
- v2へ上げる場合はv1 dataDirを暗黙に開かない。起動時にfail closedし、明示的な移行または
  初期化、全projection再構築、backup／restoreおよびrollback手順を提供する。

## 作業計画

### 1. 基準fixtureと差分表を固定する

1. RC.3で受理していたmetadata、xref、URL、HTML、検索、数式、source blockおよびWASM fixtureの
   現行出力を移行前baselineとして保存する。
2. v0.1.0からv0.6.0までの各移行ガイドを、影響するfixtureと再構築対象へ対応付ける。
3. v0.6.1の変更が文書同期とpackage identity更新だけであり、v0.6.0から公開APIの意味を
   変えないことを記録する。

### 2. 依存と供給経路をv0.6.1へ固定する

1. workspace dependencyとlockfileをpeeled commit `2a7ec4f...`へ更新する。
2. `ADOCWEAVE_SOURCE_REVISION`、Nix `outputHashes`、package commentおよび再現性検査をv0.6.1へ揃える。
3. tag、peeled commit、Cargo.lock、Nix hashおよび`release-manifest.json`の
   `packageVersion = 0.6.1`をCIで照合する。

### 3. native公開APIを移行する

1. importを`semantic`、`output`、`preprocess`、`resolution`および`text` facadeへ移す。
2. `Analysis::ast()`を`Analysis::document()`へ置き換え、walker、HTML rendererおよび
   文書profile検証を公開`Document`モデルへ移す。
3. metadata検証を`document_attribute_occurrences()`へ移し、重複、set／unset、値およびrangeを
   app固有の検証型へ変換する。
4. 旧6契約型をpackage identity型へ置き換え、起動時fail-closed検証を維持する。

### 4. 保護属性置換をadapterへ集約する

1. occurrence rangeを用いるsource-preserving置換APIを`marginalis-asciidoc`へ追加する。
2. 必須属性の欠落・重複・unsetを拒否し、複数rangeは後方から置換してoffsetを保持する。
3. create／updateのサーバ正本値、置換後strict診断および再構築時の不変条件をfixtureで固定する。
4. `marginalis-server`の直接解析コードとAdocWeave dependencyを削除する。

### 5. projection、HTMLおよびWASMを移行する

1. WASM requestの`packageVersion`と`products`を設定し、必要なproductだけを要求する。
   native／WASM parity試験では全比較対象を明示的に要求する。
2. request、response、parse summary、projectionおよびbrowser assetのpackage versionが
   すべて`0.6.1`で一致することを検証する。
3. `attributeOccurrences`のnative／WASM parity、`ProductSet`で選択したproductの有無、
   HTML、診断、検索、xref、URL policy、include noticeおよびstylesheetのgoldenを比較する。
4. v0.5以前の数値identity fieldがJSON、cache keyおよび保存済み成果物へ残らないことを検査する。

### 6. data formatとrelease gateを完成する

1. fixture差分に基づきdata format v2を確定する。v1維持を選ぶ場合は同一性証明と判断記録を残す。
2. v1 dataDirの拒否または明示的移行、全projection再構築、backup／restoreおよびrollbackを試験する。
3. crate単位のcheck／test、dependency boundary、native／WASM parity、Nix package、NixOS VMの順で
   検証し、最後に`cargo make release-gate`を実行する。

## 完了条件

- AdocWeave v0.6.1のtag、peeled commit、Cargo.lock、Nix hashおよびmanifestが一致する。
- native、WASM、projection、cacheおよび配布assetが同じpackage identity `0.6.1`を使い、
  不一致をfail closedする。
- `marginalis-server`はAdocWeaveへ直接依存せず、属性検証とsource置換を
  `marginalis-asciidoc`の公開adapter API経由で行う。
- 保護属性置換、strict診断、重複、空文字set、2形式のunsetおよびrangeがfixtureで検証される。
- RC.3からv0.6.1までの保存、検索、xref、HTMLおよびWASMの互換性差分がfixtureで明示される。
- data format version、再構築、backup／restore、rollbackおよび既存deploymentの扱いが
  文書と実装で一致する。
- `cargo make release-gate`が成功する。

## 2026-07-24調査結果

### 完了済み

- flakeへoxalica `rust-overlay`を追加し、devShellと`buildRustPackage`をRust 1.97.1へ切り替えた。
- dependency revとCargo.lockは一度v0.5.0 commit `48b8c28`へ更新した。v0.6.1への再更新が必要である。
- [上流提案009](upstream/009-public-document-attribute-occurrences.md)はv0.6.0で採用され、
  RustとWASMの公開occurrence queryが利用可能になった。

### 未完了のAPI差分

- 旧root module pathと`Analysis::ast()`が残り、公開facade／`Analysis::document()`への移行が必要である。
- `ADOCWEAVE_SOURCE_REVISION`とNix `outputHashes`はRC.3のままである。
- 旧6契約型、`WASM_API_VERSION`および`api_version`が残り、package identityへ移行していない。
- `marginalis-server`がAdocWeaveへ直接依存し、privateになった旧属性型を使用している。
- WASM testは`ProductSet`を指定せず、旧全量responseと数値versionを前提としている。

### 上流機能の充足

Resolver表示文字列、URL policy、typed notice、source language、数式・STEM projection、
resource profileおよび文書属性occurrence queryは現行AdocWeaveで提供される。現時点で
Marginalisの移行を阻む上流機能不足はない。
