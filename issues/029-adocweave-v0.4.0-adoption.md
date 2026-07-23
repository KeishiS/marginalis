# 029: AdocWeave v0.4.0 への移行

状態: RC.1 リリース後に着手。

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

- AdocWeave `v0.4.0` のtag、commitおよびNix hashが一致し、lockfileから再現できる。
- 起動時の契約不一致はfail closedし、nativeとWASMで同じ契約を報告する。
- 保存、検索、xref、HTMLおよびWASMの互換性差分がfixtureで明示される。
- data format version、再構築、backup/restoreおよび既存deploymentの扱いが文書と実装で一致する。
- `cargo make release-gate`が成功する。
