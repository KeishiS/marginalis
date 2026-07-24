# 029: AdocWeave v0.5.0 への移行

状態: 2026-07-24 着手。ツールチェーン更新済み。API 移行は方針判断待ち。

> 当初は v0.4.0 を対象としていたが、着手時点で最新が v0.5.0（commit
> `48b8c284402763729a560366f7cbac9fc218edc8`）となったため、対象を v0.5.0 へ更新した。

## 問題

現在のMarginalisはAdocWeave `v0.1.0-rc.3` のcommitと、Core・HTML・Projection・
Conformance・WASMごとの契約versionを固定している。AdocWeave `v0.4.0` は公開契約を単一の
`CONTRACT_VERSION`へ再編し、semantic block traversalとURL recognitionを変更した。

現在の`data format v1`は固定済みのAdocWeave公開契約とノートprofileを正本解釈の前提とする。
したがって依存だけを更新すると、既存の検索・xref投影、HTML出力、WASM要求および契約監視が
不整合になりうる。RC.1の契約freeze中には実施しない。

## 実装項目

1. `v0.4.0` tagと実体commitを依存、lockfileおよびNix source hashへ再現可能に固定する。
2. `AdocWeaveContracts`と起動時fail-closed検証を、上流の単一`CONTRACT_VERSION`へ移行する。
   旧契約値をキャッシュ・投影・WASM requestで使う箇所を棚卸しする。
3. semantic block traversalとURL recognitionの変更を、保存時profile、`xref:note:`抽出、
   外部URL allowlist、検索用`searchable_text`およびHTML render contractで比較する。
4. 変更が正本の解釈または投影結果を変える場合はdata format versionを上げる。既存v1 deploymentの
   扱い（明示的な初期化または移行）と、projection再構築・backup/restoreの互換性を文書化する。
5. native/WASM parity、golden HTML、投影、診断、依存境界およびNix packageをCIで検証する。

## 完了条件

- AdocWeave `v0.5.0` のtag、commitおよびNix hashが一致し、lockfileから再現できる。
- 起動時の契約不一致はfail closedし、nativeとWASMで同じ契約を報告する。
- 保存、検索、xref、HTMLおよびWASMの互換性差分がfixtureで明示される。
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
   `verify_runtime_contracts` を単一版へ再設計する（issue 実装項目 2）。WASM 側の版定数は要再確認。
3. **`attributes` モジュールの private 化（要design判断）**: `AttributeOperation`・
   `DocumentAttribute` は非公開になった。一方で v0.5.0 は protected-attribute を
   ライブラリ機能として実装しており（`ParseOptions.protected_attributes` と strict モードの
   `protected-attribute` 診断）、marginalis-server の `replace_protected_attributes`
   （保護属性をサーバ値へ黙って置換）を上流機能へ寄せられる可能性がある。ただし挙動が
   「黙って上書き」から「診断でエラー」へ変わり得るため、REST/MCP の作成・更新の契約に
   影響する。移行方針の決定が必要。

### 判断が必要な点

- **保護属性の扱い**: 従来のサーバ側黙示置換を維持するか、v0.5.0 のネイティブ
  protected-attribute（診断ベース）へ移行するか。後者は作成 API の入力契約を変える。
- **data format**: 投影出力（`output::projection::project` の結果）が v1 と同一かを確認し、
  変わる場合は data format v2 と既存 deployment の扱いを決める（roadmap 判断の節目 #3）。
