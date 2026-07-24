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
| 開発者 | [アーキテクチャ](docs/architecture.md)、[要件定義](docs/requirements.md) |
| リリース担当者 | [リリース手順](docs/release.md)、[変更履歴](CHANGELOG.md) |
| 今後の作業を確認する人 | [ロードマップ](docs/roadmap.md)、[Issue 一覧](issues/README.md) |

## 開発時の検証

`nix develop` で開発環境へ入り、`cargo make` から検証を実行します。

```text
cargo make format
cargo make lint
cargo make test
cargo make verify
# リリース候補または正式リリースの前に実行する。
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
| `MARGINALIS_DATA_DIR` | AsciiDoc 正本、SQLite、`FORMAT` マーカーを置くディレクトリ |
| `OIDC_ISSUER_URL` | OIDC issuer |
| `OIDC_CLIENT_ID` | OIDC クライアント ID |
| `OIDC_CLIENT_SECRET` または `OIDC_CLIENT_SECRET_FILE` | OIDC クライアントシークレット |
| `ROOT_PASSWORD` または `ROOT_PASSWORD_FILE` | 未初期化データベースで `root` を作成するための初期パスワード |

MCP は既定で無効です。`MARGINALIS_MCP_ENABLE=true` で有効にできます。未知のクライアントの
Client ID Metadata Document を取得する場合は、
`MARGINALIS_MCP_CLIENT_METADATA_ALLOWED_HOSTS` に許可する HTTPS ホストを指定します。

シークレットを Git、SQLite、通常の設定ファイル、ログへ保存しないでください。環境変数または
`*_FILE` を使い、実行環境のシークレット管理機構から渡します。`ROOT_PASSWORD` と
`ROOT_PASSWORD_FILE` は初回起動時にだけ必要です。初期化済みのデータベースへ指定しても、
既存の `root` パスワードは変更されません。

## 現在の範囲

REST API、`root` による利用者管理、OAuth で保護された MCP、NixOS モジュールを提供しています。
一般利用者向け Web UI、Device Authorization Grant、ベクトル検索、曖昧検索、専用の管理
オリジンと mTLS は提供していません。着手順は[ロードマップ](docs/roadmap.md)を参照してください。
