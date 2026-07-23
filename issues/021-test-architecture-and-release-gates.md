# 021: 試験アーキテクチャとrelease gate

状態: 提案。

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

## 完了条件

- production crateのtest helperが具体adapterを本番依存へ引き上げない。
- 主要なpolicyがtransportなしで試験できる。
- HTTP/MCP/data format/NixOS moduleの変更に対応するrelease gateが明確である。
- 実環境手順と自動試験の責務分担が文書化される。

