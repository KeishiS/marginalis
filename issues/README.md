# Issue 一覧

このディレクトリでは、本アプリの実装 Issue を管理する。AsciiDoc まわりの Issue は、本アプリの
機能プロファイルを一般的な AsciiDoc ライブラリへ適用するためのものである。現在の実装候補は
AdocWeave だが、各 Issue は標準構文と公開 API に対するホスト側アダプターとして記述し、
ライブラリ固有の内部実装やアプリ固有の fork を前提にしない。

## 実装順序

現在の優先順位は、Web UI より先に REST API と MCP を成立させ、`v0.1.0-rc.2` を受け入れる
ことである。root 管理は REST API で継続し、管理 UI は後続とする。既存デプロイの dataDir は
移行せず、Issue 015 で明示的に初期化して新しいスキーマへ移る。

### RC.2 release blocker

### RC.2 受入とリリース

1. [009: OIDC プロバイダー登録と実環境結合試験](009-oidc-provider-registration.md)
2. [022: v0.1.0 RC release acceptance](022-v0.1.0-rc.1-release-acceptance.md)

### v0.1.0 後の優先項目

着手順は[ロードマップ](../docs/roadmap.md)の段階に従う。

1. [030: E2E テスト自動化の準備と実装](030-end-to-end-test-automation-readiness.md)
2. [029: AdocWeave v0.4.0 への移行](029-adocweave-v0.4.0-adoption.md)
3. [032: MCP 向けノートプロファイル公開と検証診断の充実](032-mcp-authoring-profile-and-diagnostics.md)
4. [033: リポジトリ文書の AsciiDoc 移行](033-repository-documentation-asciidoc-migration.md)（029 完了後。以降と並行可）
5. [027: 検索・xref・閲覧用 RenderPolicy の完成](027-search-reference-and-rendering-projections.md)の公開フィルターと RenderPolicy
6. [026: OIDC ログイン開始要求のブラウザ結合と実行時制限](026-oidc-login-binding-and-runtime-limits.md)のリソース上限・未認証経路対策と、
   [021: 試験アーキテクチャと release gate](021-test-architecture-and-release-gates.md)のテストモジュール分離・クレート分割
7. [013: root 管理・OIDC ユーザー承認](013-root-administration-and-approval.md)のユーザー再有効化、招待、専用管理オリジン・mTLS
8. Web UI の段階導入: [006: ブラウザ編集プレビュー](006-browser-preview.md)と
   [034: グラフ可視化 Web UI](034-graph-visualization-web-ui.md)（公開判断後）
9. [031: PostgreSQL storage backend の実現性調査](031-postgresql-storage-backend-feasibility.md)

[012: MCP 曖昧検索用の中間表現インデックス調査](012-mcp-fuzzy-search-index.md)に基づく検索
拡張は、E2E での検索品質測定後に必要性を再評価する。

### 完了済みの基盤・初期実装

[016](016-product-contract-reconciliation.md)、[017](017-architecture-boundary-rebaseline-v2.md)、
[018](018-api-contract-and-openapi.md)、[019](019-web-security-and-admin-boundary.md)、
[020](020-data-format-and-maintenance-lifecycle.md)、[025](025-acl-and-metadata-invariants.md)、
[023](023-deletion-transaction-and-confirmation-integrity.md)、[015](015-api-first-architecture-rebaseline.md)、
[014](014-rest-notes-search-and-mcp.md)、[005](005-projections-and-rebuild.md)、
[010](010-nixos-module-and-release-packaging.md)、[028](028-contract-and-maintenance-reconciliation.md)の
初期公開範囲は完了している。[024](024-write-recovery-and-concurrency.md)と
[021](021-test-architecture-and-release-gates.md)の RC.1 範囲も完了している。
Issue 012 は初期実装済みであり、検索拡張と運用結合試験を後続作業として残す。

### AsciiDoc アダプターの依存順

1. [008: 一般 AsciiDoc ライブラリへの適用アダプター](008-asciidoc-library-adaptation-boundary.md)
2. [001: 依存固定と契約監視](001-adocweave-dependency-and-contract.md)
3. [002: ノート用プロファイルと属性検証](002-note-profile-and-metadata.md)
4. [003: ノート参照と Resolver](003-note-references-and-resolver.md)
5. [004: 安全な HTML、数式、コード表示](004-safe-rendering-and-presentation.md)
6. [005: 検索・グラフ投影と再構築](005-projections-and-rebuild.md)
7. [006: ブラウザ編集プレビュー](006-browser-preview.md)
8. [007: 結合試験とリリース検証](007-integration-testing-and-release.md)

`001` から `005` までが、保存・閲覧・検索・グラフのサーバー側機能を成立させる最小経路で
ある。`006` は編集体験を改善するが、サーバー側の検証を置き換えない。`007` は全 Issue の
完了条件を継続的に検証する。

## 管理単位

- 上流の AsciiDoc ライブラリへ提案する汎用 API の Issue 本文は、[upstream](upstream/README.md)
  で分けて管理する。アプリ固有の UUID・ACL・SQLite・ベース URL の決定は上流提案に含めない。
- アプリケーション全体の認証・運用上の前提は
  [009: OIDC プロバイダー登録と実環境結合試験](009-oidc-provider-registration.md)で管理する。
- 公開用 NixOS モジュールとパッケージングは
  [010: NixOS module と公開パッケージ](010-nixos-module-and-release-packaging.md)で管理する。
- 公開前の破壊的な責務分割と基盤再設計は
  [011: アーキテクチャ再基線化](011-architecture-rebaseline.md)で管理する。010 の最小
  モジュールは、011 で定める設定・サーバー境界を確定した直後から並行して実装する。
- ノート変更を検索用の中間表現へ反映し、ACL を守った MCP 曖昧検索を実現するための調査は
  [012: MCP 曖昧検索用の中間表現インデックス調査](012-mcp-fuzzy-search-index.md)で管理する。
- root のローカル認証と OIDC 保留ユーザーの承認は
  [013: root 管理・OIDC ユーザー承認](013-root-administration-and-approval.md)で管理する。
- REST API だけでのノート CRUD・検索と、そのユースケースを再利用する MCP 連携は
  [014: REST ノート API・検索・MCP 連携](014-rest-notes-search-and-mcp.md)で管理する。
- REST と MCP を実装する前の破壊的な API-first 再構成は
  [015: API-first アーキテクチャ再基線化](015-api-first-architecture-rebaseline.md)で管理する。

## 実装原則

- AdocWeave コアには、ファイル、DB、ネットワーク、時刻、認証情報へアクセスさせない。
- ノート ID、ACL、参照先の実在確認、URL 生成はホスト拡張で行う。
- HTML レンダラーには、同一リビジョンの解析結果から作った `RenderInputs` だけを渡す。
- 保存時は strict、編集中は permissive とし、どちらも生の HTML を出力しない。
- AdocWeave の Core・HTML・Projection・WASM の契約バージョンを、キャッシュと投影の再構築
  判断に使う。
- `xref:note:`、文書属性、STEM、ソースコードブロックは標準 AsciiDoc 構文として扱う。アプリの
  アダプターは AST 検証、Resolver、描画入力、投影を担い、新しいパーサー文法を追加しない。
