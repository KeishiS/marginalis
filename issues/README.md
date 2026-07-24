# Issue 一覧

このディレクトリでは、Marginalisの実装作業と調査課題を管理する。現行仕様は`docs/`、
公開済みの変更は`CHANGELOG.md`を参照する。完了したIssueは作業履歴であり、現行仕様の
根拠として扱わない。

AsciiDoc関連のIssueでは、MarginalisをAdocWeaveへ組み込む処理を扱う。標準構文と公開APIを
利用し、AdocWeaveの内部実装への依存やMarginalis専用のforkは前提としない。

## 実装順序

`v0.1.0`の受入確認と公開は、2026-07-23に完了した。詳細は
[009](009-oidc-provider-registration.md)と
[022](022-v0.1.0-rc.1-release-acceptance.md)を参照する。
`v0.2.0-rc.1`の受入確認とタグ公開は、2026-07-24に完了した。詳細は
[035](035-v0.2.0-rc.1-release-acceptance.md)を参照する。

root管理には引き続きREST APIを使い、管理UIは後続作業とする。AdocWeave v0.6.1への更新では
保存形式v1を破壊的に上書きする。既存環境の`dataDir`は移行せず、サービス停止後に完全に
削除して空の状態から初期化する。

### v0.1.0 後の優先項目

着手順は[ロードマップ](../docs/roadmap.md)の段階に従う。

1. [030: E2Eテストの自動化](030-end-to-end-test-automation-readiness.md)
2. [029: AdocWeave v0.6.1への移行](029-adocweave-v0.6.1-migration.md)
3. [035: v0.2.0-rc.1のリリース受入](035-v0.2.0-rc.1-release-acceptance.md)
4. [032: MCP向けの入力規則と検証結果](032-mcp-authoring-profile-and-diagnostics.md)
5. [033: リポジトリ文書のAsciiDoc移行](033-repository-documentation-asciidoc-migration.md)
   （029の完了後。以降の作業とは並行できる）
6. [027: 検索、xref、閲覧用の変換規則](027-search-reference-and-rendering-projections.md)
7. [026: OIDCログインと実行時制限](026-oidc-login-binding-and-runtime-limits.md)の
   リソース上限・未認証経路対策と、
   [021: テスト構成とリリース前検証](021-test-architecture-and-release-gates.md)の
   テストモジュール分割・クレート分割
8. [013: root 管理・OIDC ユーザー承認](013-root-administration-and-approval.md)のユーザー再有効化、招待、専用管理オリジン・mTLS
9. Web UIの段階導入:
   [006: ブラウザー編集プレビュー](006-browser-preview.md)と
   [034: グラフ表示Web UI](034-graph-visualization-web-ui.md)（公開判断後）
10. [031: PostgreSQL対応の実現性調査](031-postgresql-storage-backend-feasibility.md)

[012: MCP 曖昧検索用の中間表現インデックス調査](012-mcp-fuzzy-search-index.md)に基づく検索
拡張は、E2E での検索品質測定後に必要性を再評価する。

### 完了した基盤作業

[016](016-product-contract-reconciliation.md)、[017](017-architecture-boundary-rebaseline-v2.md)、
[018](018-api-contract-and-openapi.md)、[019](019-web-security-and-admin-boundary.md)、
[020](020-data-format-and-maintenance-lifecycle.md)、[025](025-acl-and-metadata-invariants.md)、
[023](023-deletion-transaction-and-confirmation-integrity.md)、[015](015-api-first-architecture-rebaseline.md)、
[014](014-rest-notes-search-and-mcp.md)、[005](005-projections-and-rebuild.md)、
[010](010-nixos-module-and-release-packaging.md)、[028](028-contract-and-maintenance-reconciliation.md)、
[029](029-adocweave-v0.6.1-migration.md)の
初期公開範囲は完了している。[024](024-write-recovery-and-concurrency.md)と
[021](021-test-architecture-and-release-gates.md)の RC.1 範囲も完了している。
Issue 012 は初期実装済みであり、検索拡張と運用結合試験を後続作業として残す。

### AdocWeave連携の依存順

1. [008: 一般 AsciiDoc ライブラリへの適用アダプター](008-asciidoc-library-adaptation-boundary.md)
2. [001: 依存固定と仕様監視](001-adocweave-dependency-and-contract.md)
3. [002: ノート用プロファイルと属性検証](002-note-profile-and-metadata.md)
4. [003: ノート参照と Resolver](003-note-references-and-resolver.md)
5. [004: 安全な HTML、数式、コード表示](004-safe-rendering-and-presentation.md)
6. [005: 検索・グラフ投影と再構築](005-projections-and-rebuild.md)
7. [006: ブラウザー編集プレビュー](006-browser-preview.md)
8. [007: 結合試験とリリース検証](007-integration-testing-and-release.md)

`001`から`005`までが、保存・閲覧・検索・グラフのサーバー側機能に必要である。
`006`は編集機能を改善するが、サーバー側の検証を置き換えない。`007`は全Issueの
完了条件を継続的に検証する。

## 管理単位

- AdocWeaveへ提案する汎用APIは、[upstream](upstream/README.md)で管理する。
  Marginalis固有のUUID、ACL、SQLite、ベースURLは上流提案に含めない。
- アプリケーション全体の認証・運用上の前提は
  [009: OIDC プロバイダー登録と実環境結合試験](009-oidc-provider-registration.md)で管理する。
- 公開用 NixOS モジュールとパッケージングは
  [010: NixOS module と公開パッケージ](010-nixos-module-and-release-packaging.md)で管理する。
- 公開前の責務分割と基盤再設計は
  [011: アーキテクチャの再設計](011-architecture-rebaseline.md)で管理する。010の最小
  モジュールは、011で設定とサーバーの責務を確定した後に並行して実装する。
- ノート変更を検索用の中間表現へ反映し、ACL を守った MCP 曖昧検索を実現するための調査は
  [012: MCP 曖昧検索用の中間表現インデックス調査](012-mcp-fuzzy-search-index.md)で管理する。
- root のローカル認証と OIDC 保留ユーザーの承認は
  [013: root 管理・OIDC ユーザー承認](013-root-administration-and-approval.md)で管理する。
- REST API だけでのノート CRUD・検索と、そのユースケースを再利用する MCP 連携は
  [014: REST ノート API・検索・MCP 連携](014-rest-notes-search-and-mcp.md)で管理する。
- RESTとMCPを実装する前に行ったAPI中心の再設計は、
  [015: APIを中心としたアーキテクチャの再設計](015-api-first-architecture-rebaseline.md)に記録する。

## 実装原則

- AdocWeaveコアには、ファイル、DB、ネットワーク、時刻、認証情報へアクセスさせない。
- ノートID、ACL、参照先の存在確認、URL生成はMarginalis側で行う。
- HTML変換には、同じリビジョンの解析結果から作った`RenderInputs`だけを渡す。
- 保存時は`strict`、編集中は`permissive`とし、どちらも未検証のHTMLを出力しない。
- AdocWeaveの完全一致するパッケージ版を、解析キャッシュ、HTML、構造情報、
  適合性検査、WASM出力を作り直す判断に使う。
- `xref:note:`、文書属性、STEM、ソースコードブロックは標準 AsciiDoc 構文として扱う。アプリの
  アダプターは AST 検証、Resolver、描画入力、投影を担い、新しいパーサー文法を追加しない。
