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
`v0.2.0`正式版の受入確認、Pull Request、タグ公開、タグのリリースゲートは、
2026-07-24に完了した。詳細は[036](036-v0.2.0-release-acceptance.md)を参照する。

`v0.3.0`では、機能拡充に先立って SQLite 正本、Kanidm 1.10 group 認可、新 API、MCP OAuth、
閲覧用 Web UI へ破壊的に再設計する。`v0.2.x` の API、保存形式、ローカル `root`、`dataDir`は
互換対象ではない。詳細な決定は[037](037-v0.3.0-architecture-rebaseline.md)を正とする。

### v0.3.0再設計の優先項目

着手順は[ロードマップ](../docs/roadmap.md)の段階に従う。

1. [037: v0.3.0のアーキテクチャ再設計](037-v0.3.0-architecture-rebaseline.md)
2. [038: SQLite正本とAsciiDoc import/export](038-sqlite-canonical-notes-and-asciidoc-bundles.md)
3. [039: Kanidmグループ認可とMCP OAuth](039-kanidm-group-authorization-and-mcp-oauth.md)
4. [040: v0.3.0のNixOS配備とKanidm 1.10 E2E](040-v0.3.0-nixos-and-e2e-foundation.md)
5. [041: 閲覧用Web UIとソフトデリート](041-web-ui-and-soft-deletion.md)
6. [042: v0.3.0のリリース受入](042-v0.3.0-release-acceptance.md)

横断的な品質確認として、[043: 本番到達性とテストカバレッジの可視化](043-production-reachability-and-test-coverage.md)
と[044: ソース配置と文書境界の整理](044-source-layout-and-documentation-boundaries.md)を完了した。
[042: v0.3.0のリリース受入](042-v0.3.0-release-acceptance.md)は自動検証と実環境受入を完了し、
Pull Request、`main`上の最終検証、タグ公開を進めている。

旧設計を前提とする未完了 Issue は削除せず履歴として保持する。新規実装の対象にするかは、
v0.3.0 公開後に再評価する。

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
