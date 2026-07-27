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
[042: v0.3.0のリリース受入](042-v0.3.0-release-acceptance.md)も完了し、2026-07-25に
`v0.3.0`タグを公開した。

### v0.3.1運用堅牢化

`v0.3.1`では機能を広げず、v0.3.0の公開仕様を運用し続けるための証拠を追加した。
受入確認と公開は2026-07-27に完了した。

1. [045: backup・復元ライフサイクル](045-backup-restore-lifecycle.md)
2. [046: browser・MCP protocol回帰試験](046-browser-mcp-protocol-regression.md)
3. [047: 実行時の運用診断](047-runtime-operability-diagnostics.md)
4. [048: v0.3.1のリリース受入](048-v0.3.1-release-acceptance.md)

045、046、047の自動検証と、048の実環境受入は完了している。

### v0.4.0のAsciiDoc契約と執筆支援

`v0.4.0`では、AdocWeaveを`v0.10.1`へ更新して保存形式v2とnote profile版を導入した後、
[032: MCP向けの入力規則と検証結果](032-mcp-authoring-profile-and-diagnostics.md)を実装する。
MCPを主要な執筆経路として、入力規則と位置付き診断をRESTとMCPで共通化する。ACL・OAuthの
再設計、Web編集、検索方式の変更、グラフ表示は同じreleaseへ混在させない。

新しい作業項目はローカルIssueとして追加せず、文書の役割整理後にGitHub Issuesで管理する。
詳細な着手順と版の範囲は[ロードマップ](../docs/roadmap.md)を正とする。

### 旧Issueと条件付き候補の分類

旧設計を前提とするIssueは削除せず、判断時点の履歴として保持する。次の分類を、新しい実装へ
着手してよいかの基準とする。

| Issue | 分類 | 扱い |
| --- | --- | --- |
| [006](006-browser-preview.md) | 条件付き再設計 | MCPでは満たせない編集需要を確認した場合だけ、SQLite正本とWeb UI v2を前提に書き直す |
| [012](012-mcp-fuzzy-search-index.md) | 評価待ち | 現行FTS5で失敗する固定例を得てから拡張方式を決める |
| [013](013-root-administration-and-approval.md) | 置換済み | ローカル`root`とapprovalはv0.3.0で廃止した。新規実装へ参照しない |
| [021](021-test-architecture-and-release-gates.md) | 基盤完了 | 残るファイル分割は独立目標にせず、対象moduleを変更するときに行う |
| [026](026-oidc-login-binding-and-runtime-limits.md) | 置換済み | session期限と上限はv0.3.0で再実装済み。運用診断だけ047で扱う |
| [027](027-search-reference-and-rendering-projections.md) | 部分置換 | 現行SQLite・REST v2で不足を再確認し、必要なfilterだけ別Issueにする |
| [030](030-end-to-end-test-automation-readiness.md) | 引継ぎ済み | v0.3.0基盤は完了。現行browser・MCP回帰は046で扱う |
| [031](031-postgresql-storage-backend-feasibility.md) | 保留 | 複数process、高可用性、規模超過の要件が生じるまで再開しない |
| [033](033-repository-documentation-asciidoc-migration.md) | 低優先度 | 大量の形式差分に見合う保守上の便益が確認されるまで着手しない |
| [034](034-graph-visualization-web-ui.md) | 条件付き再設計 | 検索と一覧だけでは参照関係を辿れない実例を得てから再設計する |

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
- backupの世代管理と復元可能性は
  [045: backup・復元ライフサイクル](045-backup-restore-lifecycle.md)で管理する。
- browser、OAuth、MCP clientのprotocol回帰は
  [046: browser・MCP protocol回帰試験](046-browser-mcp-protocol-regression.md)で管理する。
- 小規模NixOS運用の診断境界は
  [047: 実行時の運用診断](047-runtime-operability-diagnostics.md)で管理する。
- v0.3.1の公開判定は
  [048: v0.3.1のリリース受入](048-v0.3.1-release-acceptance.md)で管理する。

## 実装原則

- AdocWeaveコアには、ファイル、DB、ネットワーク、時刻、認証情報へアクセスさせない。
- ノートID、ACL、参照先の存在確認、URL生成はMarginalis側で行う。
- HTML変換には、同じリビジョンの解析結果から作った`RenderInputs`だけを渡す。
- 保存時は`strict`、編集中は`permissive`とし、どちらも未検証のHTMLを出力しない。
- AdocWeaveの完全一致するパッケージ版を、解析キャッシュ、HTML、構造情報、
  適合性検査、WASM出力を作り直す判断に使う。
- `xref:note:`、文書属性、STEM、ソースコードブロックは標準 AsciiDoc 構文として扱う。アプリの
  アダプターは AST 検証、Resolver、描画入力、投影を担い、新しいパーサー文法を追加しない。
