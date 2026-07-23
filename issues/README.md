# AsciiDocライブラリ統合アダプタの実装計画

このディレクトリは、本アプリのAsciiDoc機能プロファイルを一般的なAsciiDocライブラリへ
適用するための実装Issueを管理する。現在の実装候補はAdocWeaveだが、各Issueは標準構文と
公開APIに対するホスト側アダプタとして記述する。ライブラリ固有の内部実装やアプリ固有forkを
前提にしない。

## 実装順序

現在の優先順位は、Web UIより先にREST APIとMCPを成立させ、`v0.1.0-rc.1`を受入することである。root管理は
REST APIで継続し、管理UIは後続とする。既存デプロイのdataDirは移行せず、Issue 015で明示的に初期化して
新schemaへ移る。

### RC.1 release blocker

### RC.1受入とリリース

1. [009: OIDCプロバイダ登録と実環境結合試験](009-oidc-provider-registration.md)
2. [022: v0.1.0-rc.1 release acceptance](022-v0.1.0-rc.1-release-acceptance.md)

### RC.1後の優先項目

1. [013: root管理・OIDCユーザー承認](013-root-administration-and-approval.md)のユーザー再有効化、招待、専用管理origin・mTLS
2. [012: MCP曖昧検索用の中間表現インデックス調査](012-mcp-fuzzy-search-index.md)に基づく検索拡張と実MCP client結合試験
3. [006: ブラウザ編集プレビュー](006-browser-preview.md)（Web UIを公開する段階）
4. [029: AdocWeave v0.4.0 への移行](029-adocweave-v0.4.0-adoption.md)
5. [030: E2Eテスト自動化の準備と実装](030-end-to-end-test-automation-readiness.md)
6. [027: 検索・xref・閲覧用RenderPolicyの完成](027-search-reference-and-rendering-projections.md)の公開filterとRenderPolicy
7. [026: OIDCログイン開始要求のブラウザ結合](026-oidc-login-binding-and-runtime-limits.md)の期限設定化とproxy境界の改善
8. [021: 試験アーキテクチャとrelease gate](021-test-architecture-and-release-gates.md)のtest module完全分離

### 完了済みの基盤・初期実装

[016](016-product-contract-reconciliation.md)、[017](017-architecture-boundary-rebaseline-v2.md)、
[018](018-api-contract-and-openapi.md)、[019](019-web-security-and-admin-boundary.md)、
[020](020-data-format-and-maintenance-lifecycle.md)、[025](025-acl-and-metadata-invariants.md)、
[023](023-deletion-transaction-and-confirmation-integrity.md)、[015](015-api-first-architecture-rebaseline.md)、
[014](014-rest-notes-search-and-mcp.md)、[005](005-projections-and-rebuild.md)、
[010](010-nixos-module-and-release-packaging.md)、[028](028-contract-and-maintenance-reconciliation.md)の
初期公開範囲は完了している。[024](024-write-recovery-and-concurrency.md)と
[021](021-test-architecture-and-release-gates.md)のRC.1範囲も完了している。
Issue 012は初期実装済みであり、検索拡張と運用結合試験を後続作業として残す。

AsciiDocアダプタに関する依存順は次のとおりである。

1. [008: 一般AsciiDocライブラリへの適用アダプタ](008-asciidoc-library-adaptation-boundary.md)
2. [001: 依存固定と契約監視](001-adocweave-dependency-and-contract.md)
3. [002: ノート用プロファイルと属性検証](002-note-profile-and-metadata.md)
4. [003: ノート参照とResolver](003-note-references-and-resolver.md)
5. [004: 安全なHTML、数式、コード表示](004-safe-rendering-and-presentation.md)
6. [005: 検索・グラフ投影と再構築](005-projections-and-rebuild.md)
7. [006: ブラウザ編集プレビュー](006-browser-preview.md)
8. [007: 結合試験とリリース検証](007-integration-testing-and-release.md)

`001`から`005`は保存・閲覧・検索・グラフのサーバ側機能を成立させる最小経路である。
`006`は編集体験を改善するが、サーバ側の検証を置き換えない。`007`は全Issueの完了条件を
継続的に検証する。

上流AsciiDocライブラリに提案する汎用APIのIssue本文は、[upstream](upstream/README.md)に分けて
管理する。アプリ固有のUUID、ACL、SQLiteおよびBase URL決定は、上流提案へ含めない。

アプリケーション全体の認証・運用上の前提は[009: OIDCプロバイダ登録と実環境結合試験](009-oidc-provider-registration.md)
で管理する。

公開用NixOS moduleとパッケージングは[010: NixOS moduleと公開パッケージ](010-nixos-module-and-release-packaging.md)
で管理する。

公開前の破壊的な責務分割と基盤再設計は[011: アーキテクチャ再基線化](011-architecture-rebaseline.md)
で管理する。010の最小moduleは、011で定める設定・server境界を先に確定した直後から並行して実装する。

ノート変更を検索用中間表現へ反映し、ACLを守ったMCP曖昧検索を実現するための調査は
[012: MCP曖昧検索用の中間表現インデックス調査](012-mcp-fuzzy-search-index.md)で管理する。

rootのローカル認証とOIDC保留ユーザーの承認は
[013: root管理・OIDCユーザー承認](013-root-administration-and-approval.md)で管理する。

REST APIだけでのノートCRUD・検索と、そのuse caseを再利用するMCP連携は
[014: RESTノートAPI・検索・MCP連携](014-rest-notes-search-and-mcp.md)で管理する。

RESTとMCPを実装する前の破壊的なAPI-first再構成は
[015: API-firstアーキテクチャ再基線化](015-api-first-architecture-rebaseline.md)で管理する。

## 実装原則

- AdocWeaveコアはファイル、DB、ネットワーク、時刻および認証情報へアクセスさせない。
- ノートID、ACL、参照先の実在確認およびURL生成はホスト拡張で行う。
- HTMLレンダラーには、同一revisionの解析結果から作った`RenderInputs`だけを渡す。
- 保存時はstrict、編集中はpermissiveとし、どちらも生HTMLを出力しない。
- AdocWeaveのCore、HTML、ProjectionおよびWASM契約versionを、キャッシュと投影の
  再構築判断に使用する。
- `xref:note:`、文書属性、STEMおよびsource blockは標準AsciiDoc構文として扱う。アプリの
  アダプタはAST検証、Resolver、描画入力および投影を担い、新しいパーサー文法を追加しない。
