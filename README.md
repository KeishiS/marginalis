# Marginalis

Marginalis は、研究ノート・引用・断片的なアイデアを収集し、ノート間の参照として整理する
セルフホスト型の研究ノート環境です。

現在は Web UI よりも API を優先しています。OIDC でログインした利用者は REST API から
AsciiDoc ノートを作成・取得・更新・検索・削除でき、OAuth 2.1（Authorization Code + PKCE）で
認可した MCP クライアントも同じ ACL と更新規則で操作できます。ノートの正本はファイルであり、
検索・ACL・参照は再構築可能な SQLite 投影です。`/acceptance` に受入確認専用の最小画面が
ありますが、これは通常利用のためのブラウザ編集 UI ではありません。

## ドキュメント

| 文書 | 内容 |
| --- | --- |
| [REST API リファレンス](docs/rest-api.md) | Cookie 認証、CSRF、ノート CRUD、ACL、root 管理 |
| [OpenAPI 3.1](docs/openapi.json) | `/api/v1` の機械可読な REST 契約。実行中のサーバーでは `/api/v1/openapi.json` でも取得できる |
| [MCP と OAuth](docs/mcp.md) | エンドポイント、ツール、スコープ、クライアント登録、トークン失効 |
| [NixOS での運用](docs/nixos.md) | flake input、シークレット、リバースプロキシ、初期化、確認手順 |
| [実環境での受入確認](docs/acceptance.md) | REST、MCP、バックアップ・復元・監査の確認手順 |
| [リリース手順](docs/release.md) | RC の自動ゲート、実環境受入、タグ付けと GitHub Release |
| [アーキテクチャ](docs/architecture.md) | クレート境界と、正本・投影・認可の不変条件 |
| [要件定義](docs/requirements.md) | 確定・仮決定・未決に分類した要件 |
| [ロードマップ](docs/roadmap.md) | 今後の着手順と依存関係 |

## 開発時の検証

`nix develop` の環境で、日常の作業は `cargo make` から実行します。

```text
cargo make format
cargo make lint
cargo make test
cargo make verify
# RC または正式リリースの前には、NixOS VM テストとパッケージビルドも実行する。
cargo make release-gate
```

## 起動時の環境変数

| 変数 | 必須 | 説明 |
| --- | --- | --- |
| `MARGINALIS_DATABASE_URL` | 必須 | SQLite の接続 URL |
| `MARGINALIS_BASE_URL` | 必須 | 公開ベース URL。現在は `https://marginalis.sandi05.com` |
| `MARGINALIS_LISTEN_ADDR` | 必須 | HTTP の待受アドレス。例: `127.0.0.1:3000` |
| `MARGINALIS_DATA_DIR` | 必須 | AsciiDoc 正本・SQLite・フォーマット v1 マーカーを置く永続ディレクトリ。空のディレクトリだけを初期化し、マーカーのない既存ディレクトリは拒否する |
| `MARGINALIS_MCP_ENABLE` | 任意 | `true` の場合だけ OAuth 保護された MCP エンドポイントを公開する。既定値は `false` |
| `MARGINALIS_MCP_CLIENT_METADATA_ALLOWED_HOSTS` | MCP 有効時に推奨 | Client ID Metadata Document を取得してよい HTTPS ホストのカンマ区切り一覧。空なら未知クライアントのメタデータを取得しない |
| `OIDC_ISSUER_URL` | 必須 | OIDC issuer。Kanidm のクライアントごとの issuer として `https://id.sandi05.com/oauth2/openid/marginalis` を設定する |
| `OIDC_CLIENT_ID` | 必須 | OIDC クライアント ID。現在は `marginalis` |
| `OIDC_CLIENT_SECRET` | 必須 | OIDC クライアントシークレット。シークレット管理機構から環境変数へ注入する |
| `OIDC_CLIENT_SECRET_FILE` | 代替 | `OIDC_CLIENT_SECRET` の代わりに、シークレットだけを含むファイルのパスを指定する |
| `RUST_LOG` | 任意 | 構造化ログの粒度。未指定時は `info`。例: `RUST_LOG=debug` |
| `ROOT_PASSWORD` | 初回のみ必須 | 未初期化の DB へ緊急管理者 `root` を作るためのパスワード |
| `ROOT_PASSWORD_FILE` | 代替 | `ROOT_PASSWORD` の代わりに、初期化用パスワードファイルのパスを指定する |

IdP へ登録するリダイレクト URI は次のとおりです。

```text
https://marginalis.sandi05.com/auth/oidc/callback
```

OIDC の認可要求には `openid profile email` スコープと Authorization Code Flow、PKCE S256 を
使います。

## シークレットの扱い

`OIDC_CLIENT_SECRET` と `ROOT_PASSWORD` を、Git・SQLite・通常の設定ファイル・ログへ保存
しないでください。デプロイ環境のシークレット管理機構（コンテナオーケストレーター、systemd
credential、ホスティング基盤のシークレット注入など）から、環境変数または `*_FILE` で渡します。

`ROOT_PASSWORD` は初回起動時に Argon2id ハッシュとして DB へ保存されます。初期化済みの DB
では不要で、設定しても既存の root パスワードは変更されません。

保留中の OIDC ユーザーは、root が REST API から有効化できます。エンドポイントと CSRF の
扱いは [REST API リファレンス](docs/rest-api.md#root-管理)を参照してください。root の
パスワードをコマンド履歴・プロセス引数・ログに残さないでください。

## データ初期化の方針

REST API と MCP を共通のアプリケーション層へ接続する再基線化では、既存の `dataDir` を移行
しません。データフォーマット v1 は `FORMAT` マーカーを持つディレクトリだけを受け入れます。
既存データの破棄は、サービス停止後に運用者が明示的に行います。通常の起動・`nixos-rebuild`・
Marginalis 自身が `dataDir` を自動削除することはありません。NixOS での正確な手順は
[NixOS での運用](docs/nixos.md#既存データを破棄して初期化する)を参照してください。

## MCP

MCP は既定で無効です。有効化の方法、OAuth Authorization Code + PKCE、Client ID Metadata
Document、利用できるツール、トークンの期限とローテーションは [MCP と OAuth](docs/mcp.md) を
参照してください。

## 現在の範囲と後続作業

`v0.1.0` の範囲は、REST API、root による保留 OIDC ユーザーの承認、OAuth 保護 MCP です。
受入専用の `/acceptance` を除くブラウザ編集 UI、数式・コードのクライアント描画、Device
Authorization Grant、ベクトル・曖昧検索、専用管理オリジン・mTLS は後続の作業です。実装順と
未完了の項目は [Issue 一覧](issues/README.md)で管理しています。
