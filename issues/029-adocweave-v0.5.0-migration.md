# 029: AdocWeave v0.5.0 への移行

状態: 2026-07-24 着手。ツールチェーンと依存 rev は更新済み。API・data format 移行は未完了。

> 当初は v0.4.0 を対象としていたが、着手時点で最新が v0.5.0（commit
> `48b8c284402763729a560366f7cbac9fc218edc8`）となったため、対象を v0.5.0 へ更新した。

## 問題

現在のMarginalisはAdocWeave `v0.1.0-rc.3` のcommitと、Core・HTML・Projection・
Conformance・WASMごとの契約versionを固定している。移行元は v0.4.0 ではないため、
`v0.1.0-rc.3`からstable v0.1.0を経て、v0.2.0、v0.3.0、v0.4.0、v0.5.0までの変更を
順に監査しなければならない。

AdocWeave v0.5.0は公開契約を単一の`CONTRACT_VERSION = 5`へ再編し、Rust公開moduleを
用途別facadeへ移した。また、移行途中のreleaseにはresolved reference表示、失敗情報の秘匿化、
table表示属性、admonition、quote／verse、block title、include noticeおよびstylesheetの変更が
含まれる。

現在の`data format v1`は固定済みのAdocWeave公開契約とノートprofileを正本解釈の前提とする。
したがって依存だけを更新すると、既存の検索・xref投影、HTML出力、WASM要求および契約監視が
不整合になる。

## 上流の正本

- release成果物と実装はAdocWeave `v0.5.0` tagおよびcommit
  `48b8c284402763729a560366f7cbac9fc218edc8`へ固定する。
- 移行手順と公開契約の説明はAdocWeave `main`の`docs/current-contract.adoc`および
  `docs/upgrade-0-2.adoc`から`docs/upgrade-0-5.adoc`までを参照する。調査時点の文書commitは
  `7deaa548e16220463f65d31f19be5845e32f47f5`である。
- 各releaseのpackage、Rustおよびcontract versionは、そのtagに含まれる
  `release-manifest.json`を機械可読な正本とする。
- `main`のv0.3.0移行ガイドには、本文とmanifestがcontract 2を示す一方、更新手順だけが
  contract 3を示す不整合がある。v0.3.0 tagのmanifestに従いcontract 2として扱う。

## 採用方針

### 保護属性

- raw AsciiDoc APIの既存契約を維持する。createでは`note-id`、`creator-id`、`created-at`、
  `updated-at`を、updateでは不変metadataと新しい`updated-at`をサーバの正本値へ置換して保存する。
- AdocWeaveの`ParseOptions.protected_attributes`と`protected-attribute`診断は、正本値への
  置換を代替せず、置換後の保存sourceと再構築時sourceに対する追加検証として利用する。
  利用者入力の不一致自体は既存契約どおりサーバ値へ置換し、保存される正本に不一致が残る場合は
  strict解析でfail closedする。
- AdocWeaveの解析、属性query、source rangeによる置換は`marginalis-asciidoc`へ集約する。
  `marginalis-server`からAdocWeaveへの直接依存を削除し、serverはアプリ固有adapter APIだけを使う。

### 公開属性query

v0.5.0では`DocumentAttribute`と`AttributeOperation`のmodule pathが非公開になった。最終属性値は
`analysis.presentation().attributes()`から取得できるが、Marginalisが必要とする重複、set／unset、
source順および置換rangeを、公開型へ依存せず安定して判定できるqueryが不足している。

- AdocWeaveのprivate moduleへ依存しない。
- 汎用のtyped occurrence queryを[上流提案009](upstream/009-public-document-attribute-occurrences.md)
  として提案する。
- 上流APIが利用可能になるまで移行を待てない場合は、`marginalis-asciidoc`内部に限定した
  header scannerを用いる。標準属性行だけを対象とし、AdocWeaveのrange、duplicate、unset fixtureと
  byte単位で照合する。server側へ独自parserを置かない。

### data format

- v0.1.0-rc.3以後に公開契約、projection、HTMLおよびxref解決結果の意味が変わるため、
  data format v2を既定の移行判断とする。
- data format v1を維持できるのは、既存profileで受理する全fixtureについて、正本解釈と永続化する
  projectionが同一であることを比較試験で証明できた場合だけとする。
- v2へ上げる場合は既存v1 deploymentを暗黙に開かない。起動時にfail closedし、明示的な移行または
  初期化、全projection再構築、backup／restore互換性を文書化する。

## 実装項目

1. `v0.1.0-rc.3`からv0.5.0までのrelease manifestと移行ガイドを順に確認し、利用者可視の
   HTML、projection、診断、WASMおよびRust API差分をfixtureへ対応付ける。
2. `v0.5.0` tagと実体commitを依存、lockfileおよびNix source hashへ再現可能に固定する。
   `ADOCWEAVE_SOURCE_REVISION`とNix `outputHashes`の旧RC.3値も更新する。
3. `AdocWeaveContracts`と起動時fail-closed検証を、上流の単一`CONTRACT_VERSION`へ移行する。
   旧契約値をキャッシュ・投影・WASM requestで使う箇所を棚卸しする。
4. Rust importを`semantic`、`output`、`preprocess`、`resolution`および`text` facadeへ移す。
   WASM requestも同じ`CONTRACT_VERSION`を使用する。
5. 属性抽出と保護属性置換を`marginalis-asciidoc`の公開adapter APIへ集約し、
   `marginalis-server`のAdocWeave直接依存を削除する。
6. resolved reference、semantic block traversal、URL recognition、table／block presentation、
   include noticeおよびstylesheet変更を、保存時profile、`xref:note:`抽出、外部URL allowlist、
   `searchable_text`、HTMLおよびWASMで比較する。
7. data format v2、既存v1 deploymentの明示的な移行または初期化、全projection再構築、
   backup／restoreおよびrollbackの手順を実装・文書化する。v1を維持する場合は同一性fixtureを
   判断記録として残す。
8. native／WASM parity、golden HTML、projection、診断、依存境界、Nix packageおよび
   release assetのcontract一致をCIで検証する。

## 完了条件

- AdocWeave `v0.5.0` のtag、commitおよびNix hashが一致し、lockfileから再現できる。
- 起動時の契約不一致はfail closedし、nativeとWASMで同じ契約を報告する。
- `marginalis-server`はAdocWeaveへ直接依存せず、属性検証とsource置換は
  `marginalis-asciidoc`の公開adapter APIを経由する。
- サーバ正本値による保護属性置換と、strictなprotected-attribute診断の両方がfixtureで検証される。
- `v0.1.0-rc.3`からv0.5.0までの保存、検索、xref、HTMLおよびWASMの互換性差分がfixtureで明示される。
- data format version、再構築、backup/restoreおよび既存deploymentの扱いが文書と実装で一致する。
- `cargo make release-gate`が成功する。

## 2026-07-24 調査結果

### ツールチェーン（完了）

- v0.5.0 は Rust `1.97.1` を要求する。nixos-unstable の現行 HEAD は 1.97.0 までのため、flake へ
  oxalica `rust-overlay` を追加し、devShell と `buildRustPackage` を `rust-bin.stable."1.97.1"` へ
  切り替えた。nixpkgs 入力も 2026-07-23（1.97.0 提供）へ更新済み。`nix develop` で
  `rustc 1.97.1` を確認した。
- 依存 rev を v0.5.0（`48b8c28`）へ更新し、Cargo.lock を再解決した。

### API 差分（`cargo check -p marginalis-asciidoc` は 28 エラー）

1. **モジュール再編（機械的）**: v0.5.0 は公開 API を facade へ整理した。対応は概ね次のとおり。
   - `html` → `output::html`、`projection` → `output::projection`、`conformance` →
     `output::conformance`
   - `inline`/`parser`/`walker` → `semantic`
   - `source` → `text`、`render`/`url` → `resolution`、`preprocessor` → `preprocess`
   - `Engine`/`ParseOptions`/`Analysis`/`SyntaxMode` は crate root で再エクスポート継続。
2. **契約バージョンの集約**: `CORE_PROFILE_VERSION` ほか 5 定数は削除され、単一
   `adocweave::CONTRACT_VERSION: u16 = 5` になった。`AdocWeaveContracts`・`PINNED_CONTRACTS`・
   `verify_runtime_contracts`を単一版へ再設計する。WASMのrequest／responseも同じ定数を使い、
   旧`WASM_API_VERSION`は存在しない。
3. **`attributes` モジュールの private 化（要design判断）**: `AttributeOperation`・
   `DocumentAttribute`の公開pathはなくなった。一方で v0.5.0 は protected-attribute を
   ライブラリ機能として実装しており（`ParseOptions.protected_attributes` と strict モードの
   `protected-attribute` 診断）、これはsourceの正本値への置換を行わない。既存API契約を守るため、
   サーバ置換をadapter境界へ移して維持し、診断を追加検証として使う。
4. **公開属性queryの不足**: `analysis.presentation().attributes()`は最終値を返すが、重複、
   set／unset、source順およびrangeを型付きで扱う安定APIがない。上流提案009を作成し、
   private moduleへの依存を避ける。
5. **移行区間の追加差分**: RC.3からstable v0.1.0までにもresolved referenceの`display_text`、
   失敗情報のkind-only化および契約version更新がある。v0.2.0からv0.4.0までの移行ガイドも
   省略せず適用する。
