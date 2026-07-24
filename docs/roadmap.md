# ロードマップ

## 現在地

`v0.1.0` をリリースし、OpenAPI 仕様の互換性保証を開始しました
（2026-07-23、段階 0 完了）。OIDC 認証付きの REST API、OAuth 保護 MCP、NixOS モジュール、
手動受入手順、リリース検証が揃っています。今後は機能を広げる前に、実運用の主要経路を
継続的に検証できる基盤を整えます。

データフォーマットの識別子は v1 を維持していますが、AdocWeave v0.6.1 前提の内容へ
破壊的に再定義しました。以前の v1 は互換対象ではありません。
現在は `v0.2.0-rc.1` の実環境受入と公開準備を進めています。

各作業の詳細な受入条件と設計判断は [Issue 一覧](../issues/README.md)を正とします。この文書は
着手順と依存関係だけを示します。

## 優先順

| 段階 | 主 Issue | 目的 | 次段階へ進む条件 |
| --- | --- | --- | --- |
| 0（完了） | [009](../issues/009-oidc-provider-registration.md)、[022](../issues/022-v0.1.0-rc.1-release-acceptance.md) | RC.2 の実環境受入を完了し、v0.1.0 をタグ付けして OpenAPI の互換性保証を始める | 完了（2026-07-23、`v0.1.0` タグ） |
| 2（完了） | [029](../issues/029-adocweave-v0.6.1-migration.md) | AdocWeave v0.6.1 へ移行し、正本の解釈・投影・HTML・WASM の互換性基準を更新する | 完了（2026-07-24。旧 v1 は移行せず、`dataDir` を削除して初期化する） |
| 公開準備（現在） | [035](../issues/035-v0.2.0-rc.1-release-acceptance.md) | 空の新 v1 環境で実環境受入を行い、`v0.2.0-rc.1` の公開可否を判断する | 実 Kanidm、実 MCP クライアント、バックアップ先を使う受入が完了し、タグ公開方法を決定する |
| 1（次） | [030](../issues/030-end-to-end-test-automation-readiness.md) | ブラウザー・テスト IdP・リバースプロキシ・MCP クライアントを通す E2E 基盤を CI へ導入する | OIDC、REST CRUD、MCP OAuth、プロキシ境界の主要経路を非対話で再現できる |
| 3 | [032](../issues/032-mcp-authoring-profile-and-diagnostics.md) | MCP クライアント向けのプロファイル公開と位置付き検証診断を追加する | MCP / REST が後方互換な診断を返し、クライアントが推測なしで入力を修正できる |
| 4 | [033](../issues/033-repository-documentation-asciidoc-migration.md) | リポジトリ文書を AsciiDoc へ移行し、文書検証を CI へ組み込む | README、仕様、運用手順、Issue の形式・参照・閲覧方針が確定する。段階 3 以降と並行できる |
| 5 | [027](../issues/027-search-reference-and-rendering-projections.md) | RenderPolicy、参照表示、公開フィルターを完成させる | 閲覧用 HTML と参照表示の可視性・安全性の仕様がフィクスチャで固定される |
| 6 | [026](../issues/026-oidc-login-binding-and-runtime-limits.md)、[021](../issues/021-test-architecture-and-release-gates.md) | リソース上限の導入、未認証経路の書き込み増幅対策、肥大化したクレートの分割など、Web UI 前の地固めを行う | 明示的な上限が仕様として文書化され、`marginalis-web` / `marginalis-sqlite` がモジュール分割されている |
| 7 | [013](../issues/013-root-administration-and-approval.md) | ユーザー再有効化、招待、SMTP など管理機能を拡充する | 管理経路の脅威モデルと運用手順が実際の配備に適合する |
| 8 | [006](../issues/006-browser-preview.md)、[034](../issues/034-graph-visualization-web-ui.md) | Web UI を段階的に導入する（閲覧専用 → 編集・プレビュー → グラフ可視化） | プロファイル・診断・RenderPolicy を再利用し、保存時検証と編集時表示が乖離しない |
| 9 | [031](../issues/031-postgresql-storage-backend-feasibility.md) | PostgreSQL を任意バックエンドとして採用する価値と移行可能性を判断する | SQLite 継続・PostgreSQL 実装・見送りのいずれかを根拠とともに決定する |

## 継続的な改善

- [012](../issues/012-mcp-fuzzy-search-index.md): E2E で検索の可視性と性能を測定した後、
  曖昧検索や中間表現インデックスの必要性を再評価する。
- [021](../issues/021-test-architecture-and-release-gates.md): E2E スイートの定着後にテスト
  モジュールを分離し、リリースゲートの保守性を改善する（段階 6 の分割作業と連動）。

## 判断の節目

1. **段階 0**: 実環境受入（実 Kanidm・実 MCP クライアント・バックアップ確認）は運用者の
   手動作業である。完了判定と v0.1.0 のタグ付けは運用者が行う。
2. **段階 1 の開始前**: Issue 030 の実行基盤、テスト IdP、プロキシ再現範囲、MCP 自動
   クライアント、テスト成果物の方針の 5 項目を決める。
3. **段階 2 の完了時**: データフォーマット v1 を AdocWeave v0.6.1 前提で破壊的に
   再定義する。旧 v1 は移行せず、既存 `dataDir` を削除して初期化する手順を確定する。
4. **段階 4 の完了時**: GitHub 上の表示と外部リンクの互換性を踏まえ、AsciiDoc をリポジトリ
   文書の標準形式として確定する。
5. **段階 7 の前後**: 一般利用者向け Web UI を公開するか、API / MCP 中心の運用を続けるか
   判断する。公開する場合は Issue 034 のフロントエンド技術選定を先に行う。
6. **段階 9 の調査後**: 利用者数、ノート数、同時書き込み、バックアップ要件を根拠に、SQLite を
   継続するか PostgreSQL を任意バックエンドとして実装するか判断する。

## 監視項目

段階には置かず、実利用からの信号で再評価します。

- **Device Flow / Personal Access Token**: ブラウザーを開けない CUI 環境の実利用者が現れた
  時点で、要件定義の設計方針に沿って優先度を再評価する。
- **AdocWeave の参照表示 API**: v0.6.1 で利用できる `display_text` を段階 5 の
  参照表示規則に利用する。
- **検索品質**: 段階 1 の E2E で測定した後、Issue 012 の曖昧検索の要否を判断する。

各段階で `cargo make release-gate` と、該当する実環境受入を実施します。公開 API または
データフォーマットを変更する段階では、次のリリース候補を作る前に OpenAPI、MCP 仕様、NixOS
運用手順、受入確認を更新します。
