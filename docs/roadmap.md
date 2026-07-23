# ロードマップ

## 現在地

`v0.1.0-rc.2` で、OIDC 認証付きの REST API、OAuth 保護 MCP、NixOS モジュール、手動受入
手順、リリースゲートが揃いました。今後は機能を広げる前に、実運用の経路を継続的に検証できる
基盤を整えます。

各作業の詳細な受入条件と設計判断は [Issue 一覧](../issues/README.md)を正とします。この文書は
着手順と依存関係だけを示します。

## 優先順

| 段階 | 主 Issue | 目的 | 次段階へ進む条件 |
| --- | --- | --- | --- |
| 1 | [030](../issues/030-end-to-end-test-automation-readiness.md) | ブラウザ・テスト IdP・リバースプロキシ・MCP クライアントを通す E2E 基盤を CI へ導入する | OIDC、REST CRUD、MCP OAuth、プロキシ境界の主要経路を非対話で再現できる |
| 2 | [029](../issues/029-adocweave-v0.4.0-adoption.md) | AdocWeave v0.4.0 へ移行し、正本解釈・投影・HTML・WASM の契約を再固定する | データフォーマット、再構築、バックアップ・復元を含む互換性方針が確定する |
| 3 | [033](../issues/033-repository-documentation-asciidoc-migration.md) | リポジトリ文書を AsciiDoc へ移行し、文書検証を CI へ組み込む | README、仕様、運用手順、Issue の形式・参照・閲覧方針が固定される |
| 4 | [032](../issues/032-mcp-authoring-profile-and-diagnostics.md) | MCP クライアント向けのプロファイル公開と位置付き検証診断を追加する | MCP / REST が後方互換な診断を返し、クライアントが推測なしで入力を修正できる |
| 5 | [027](../issues/027-search-reference-and-rendering-projections.md) | RenderPolicy、参照表示、公開フィルターを完成させる | 閲覧用 HTML と参照表示の可視性・安全性契約がフィクスチャで固定される |
| 6 | [013](../issues/013-root-administration-and-approval.md) | 再有効化、招待、専用管理オリジン・mTLS などの管理機能を必要に応じて追加する | 管理経路の脅威モデルと運用手順が実際の配備に適合する |
| 7 | [006](../issues/006-browser-preview.md) | 一般利用者向けのブラウザ編集・プレビューを提供する | プロファイル・診断・RenderPolicy を再利用し、保存時検証と編集時表示が乖離しない |
| 8 | [031](../issues/031-postgresql-storage-backend-feasibility.md) | PostgreSQL を任意バックエンドとして採用する価値と移行可能性を判断する | SQLite 継続・PostgreSQL 実装・見送りのいずれかを根拠とともに決定する |

## 継続的な改善

- [021](../issues/021-test-architecture-and-release-gates.md): E2E スイートの定着後にテスト
  モジュールを分離し、リリースゲートの保守性を改善する。
- [012](../issues/012-mcp-fuzzy-search-index.md): E2E で検索の可視性と性能を測定した後、
  曖昧検索や中間表現インデックスの必要性を再評価する。
- [026](../issues/026-oidc-login-binding-and-runtime-limits.md): アプリケーション側の上限と
  プロキシ側レート制限を E2E で検証し、trusted proxy や専用管理オリジンの導入要否を判断する。

## 判断の節目

1. **段階 1 の開始前**: Issue 030 の実行基盤、テスト IdP、プロキシ再現範囲、MCP 自動
   クライアント、アーティファクト方針の 5 項目を決める。
2. **段階 2 の完了時**: AdocWeave v0.4.0 の互換性差分が、データフォーマット v2 または明示的な
   移行を必要とするか判断する。
3. **段階 3 の完了時**: GitHub 上の表示と外部リンクの互換性を踏まえ、AsciiDoc をリポジトリ
   文書の標準形式として確定する。
4. **段階 5 の完了時**: 一般利用者向け Web UI を公開するか、API / MCP 中心の運用を続けるか
   判断する。
5. **段階 8 の調査後**: 利用者数、ノート数、同時書き込み、バックアップ要件を根拠に、SQLite を
   継続するか PostgreSQL を任意バックエンドとして実装するか判断する。

各段階で `cargo make release-gate` と、該当する実環境受入を実施します。公開 API または
データフォーマットを変更する段階では、次のリリース候補を作る前に OpenAPI、MCP 契約、NixOS
運用手順、受入確認を更新します。
