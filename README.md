# Marginalis

Marginalis は、研究ノート、引用、断片的なアイデアを AsciiDoc で蓄積し、ノート間の参照として
整理するセルフホスト型の研究ノート環境です。

現在は REST API と MCP を提供しています。OIDC でログインした利用者は、ノートの作成、取得、
更新、検索、削除を REST API から実行できます。OAuth 2.1 の Authorization Code Grant と
PKCE で認可した MCP クライアントも、同じアクセス制御と更新規則に従います。

ノートの正本は AsciiDoc ファイルです。SQLite には、正本から再構築できる検索索引、アクセス
制御、ノート間参照を保存します。`/acceptance` は実環境での受入確認にだけ使用する画面であり、
一般利用者向けの閲覧・編集画面ではありません。

## 文書案内

目的に応じて、次の文書から読み始めてください。文書全体の位置づけは
[文書案内](docs/README.md)にまとめています。

| 読者 | 文書 |
| --- | --- |
| REST API の利用者 | [REST API リファレンス](docs/rest-api.md)、[OpenAPI 3.1](docs/openapi.json) |
| MCP クライアントの利用者 | [MCP と OAuth](docs/mcp.md) |
| NixOS の運用者 | [NixOS での運用](docs/nixos.md)、[実環境での受入確認](docs/acceptance.md) |
| 開発者 | [GitHubを使う開発手順](docs/development.md)、[アーキテクチャ](docs/architecture.md)、[要件定義](docs/requirements.md) |
| リリース担当者 | [リリース手順](docs/release.md)、[変更履歴](CHANGELOG.md) |
| 今後の作業を確認する人 | [ロードマップ](docs/roadmap.md)、[Issue 一覧](issues/README.md) |

## 開発時の検証

`nix develop` で開発環境へ入り、`cargo make` から検証を実行します。

```text
cargo make format
cargo make lint
cargo make test
cargo make verify
# 公開リリースの前に実行する。
cargo make release-gate
```

`release-gate` は NixOS VM テストとパッケージビルドも含みます。リリース時の確認範囲と公開手順は
[リリース手順](docs/release.md)を参照してください。

## 起動と運用

NixOS モジュールの設定、シークレット、リバースプロキシ、永続データ、バックアップと復元は
[NixOS での運用](docs/nixos.md)で説明しています。

直接起動する場合は、少なくとも次の設定が必要です。

| 変数 | 用途 |
| --- | --- |
| `MARGINALIS_DATABASE_URL` | SQLite の接続 URL |
| `MARGINALIS_BASE_URL` | 外部からアクセスする HTTPS のベース URL |
| `MARGINALIS_LISTEN_ADDR` | HTTP の待受アドレス |
| `MARGINALIS_DATA_DIR` | SQLite runtime state を置くディレクトリ |
| `OIDC_ISSUER_URL` | OIDC issuer |
| `OIDC_CLIENT_ID` | OIDC クライアント ID |
| `OIDC_CLIENT_SECRET` または `OIDC_CLIENT_SECRET_FILE` | OIDC クライアントシークレット |
| `KANIDM_MEMBERSHIP_API_URL` | Kanidm membership API のベース URL |
| `KANIDM_MEMBERSHIP_TOKEN` または `KANIDM_MEMBERSHIP_TOKEN_FILE` | read-only service-account token |

MCP は既定で無効です。`MARGINALIS_MCP_ENABLE=true` で有効にできます。client は Dynamic Client
Registration と Authorization Code + PKCE を使います。

シークレットを Git、SQLite、通常の設定ファイル、ログへ保存しないでください。環境変数または
`*_FILE` を使い、実行環境のシークレット管理機構から渡します。

## 現在の範囲

Kanidm group 認可、SQLite 正本、閲覧 Web UI、OAuth で保護された MCP、NixOS module を提供します。
現行運用は [v0.3.0 運用契約](docs/v0.3.0-operations.md) を参照してください。
