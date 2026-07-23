# AsciiDocライブラリ統合アダプタの実装計画

このディレクトリは、本アプリのAsciiDoc機能プロファイルを一般的なAsciiDocライブラリへ
適用するための実装Issueを管理する。現在の実装候補はAdocWeaveだが、各Issueは標準構文と
公開APIに対するホスト側アダプタとして記述する。ライブラリ固有の内部実装やアプリ固有forkを
前提にしない。

## 実装順序

現在の優先順位は、Web UIより先にREST APIとMCPを成立させることである。root管理はREST APIで継続し、
管理UIは後続とする。既存デプロイのdataDirは移行せず、Issue 015で明示的に初期化して新schemaへ移る。

1. 完了: [016: プロダクト契約と要件定義の再整合](016-product-contract-reconciliation.md)
2. 完了: [017: 依存境界を強制するアーキテクチャ再基線化 v2](017-architecture-boundary-rebaseline-v2.md)
3. 完了: [020: data format v1とmaintenance lifecycle](020-data-format-and-maintenance-lifecycle.md)
4. 完了: [019: Web security baselineとroot管理境界](019-web-security-and-admin-boundary.md)
5. [021: 試験アーキテクチャとrelease gate](021-test-architecture-and-release-gates.md)
6. [009: OIDCプロバイダ登録と実環境結合試験](009-oidc-provider-registration.md)
7. [012: MCP曖昧検索用の中間表現インデックス調査](012-mcp-fuzzy-search-index.md)
8. [013: root管理・OIDCユーザー承認](013-root-administration-and-approval.md) の再有効化
9. [006: ブラウザ編集プレビュー](006-browser-preview.md)（Web UIを公開する段階）
10. 完了: [018: API契約・OpenAPI・互換性方針](018-api-contract-and-openapi.md)
11. [013: root管理・OIDCユーザー承認](013-root-administration-and-approval.md) の招待・専用管理origin

015、014、005および010の初期公開に必要な実装は完了している。以後は実OIDC/MCP clientとの結合、
検索の拡張、公開前検証およびWeb UIを優先する。

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
