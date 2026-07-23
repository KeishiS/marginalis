# 021: 試験アーキテクチャとrelease gate

状態: 実装中。RC.1の自動gateを追加し、実Kanidm/MCPの受入だけを手動gateとして残す。

## 目的

単一crateの大きなtest moduleへ集まった検証を、unit・adapter contract・transport contract・integration・
deployment acceptanceへ分け、変更時に必要な検証範囲を予測可能にする。

## 実装項目

1. application層にin-memory fake portを置き、command/query/policyを高速に単体試験する。
2. SQLite/files/AsciiDoc/OIDC adapterごとにport contract testを作る。
3. 実SQLite・filesystem・OIDC mock・Axum・MCPを通す`marginalis-integration-tests` crateを作る。
4. OpenAPI/REST/MCP schema、migration、data format、backup/restoreをrelease gateへ加える。
5. NixOS VM testを、module評価だけでなくmaintenance lifecycleとreverse proxy前提まで拡張する。
6. 実Kanidm・実MCP clientを通す確認は、secretをCIへ入れず`docs/acceptance.md`の手動acceptanceとして維持する。

## RC.1 release gate

- `cargo make verify`: format、Clippy、workspace test、transport dependency boundary、OpenAPI contract、flake評価。
- `actionlint`: 通常CIとtag/手動起動用のrelease workflowを検証する。
- `nix flake check -L`: module評価、maintenance lifecycle VM、実binaryのroot-only縮退起動VMを実行する。
- `nix build .#packages.x86_64-linux.default --no-link`: Linux packageと`share/marginalis/openapi.json`を実ビルドする。
- 実Kanidm、reverse proxyおよび実MCP clientは秘密情報を必要とするため、`docs/acceptance.md`とIssue 022で
  手動受入する。

## 完了条件

- production crateのtest helperが具体adapterを本番依存へ引き上げない。
- 主要なpolicyがtransportなしで試験できる。
- HTTP/MCP/data format/NixOS moduleの変更に対応するrelease gateが明確である。
- 実環境手順と自動試験の責務分担が文書化される。

## 実施結果

- `cargo make release-gate`とGitHub Actionsの`Release gate` workflowを追加した。
- NixOS VMはmodule設定とmaintenance lifecycleに加え、実Marginalis binaryのroot初期化、OIDC Discovery失敗時の
  縮退起動、health/readiness、OpenAPIおよびroot loginを検証する。
- test moduleの完全なintegration crateへの分離は、RC.1の機能・security gateを満たした後の内部改善として継続する。
