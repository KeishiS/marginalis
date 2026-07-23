# Marginalis

Marginalisは、研究ノート、引用、断片的なアイディアを収集し、ノート間の参照として整理する
セルフホスト型の研究ノート環境である。

現在はWeb UIよりもAPIを優先する。OIDCでログインした利用者はREST APIからAsciiDocノートを
作成・取得・更新・検索・物理削除でき、OAuth 2.1 Authorization Code + PKCEで認可したMCP clientも
同じACLと更新規則で操作できる。ノート正本はファイル、検索・ACL・参照は再構築可能なSQLite投影である。

| 文書 | 内容 |
| --- | --- |
| [REST API](docs/rest-api.md) | Cookie認証、CSRF、ノートCRUD、ACL、root管理 |
| [MCPとOAuth](docs/mcp.md) | endpoint、tool、scope、client登録、token失効 |
| [NixOS運用](docs/nixos.md) | flake input、secret、reverse proxy、初期化、確認手順 |
| [実環境受入確認](docs/acceptance.md) | REST、MCP、backup・restore・監査の確認手順 |
| [アーキテクチャ](docs/architecture.md) | crate境界、正本・投影・認可の不変条件 |

## 開発時の検証

`nix develop`の環境で、頻繁な作業は`cargo make`から実行する。

```text
cargo make format
cargo make lint
cargo make test
cargo make verify
```

## 起動時の環境変数

| 変数 | 必須 | 説明 |
| --- | --- | --- |
| `MARGINALIS_DATABASE_URL` | 必須 | SQLite接続URL。 |
| `MARGINALIS_BASE_URL` | 必須 | 公開Base URL。現在は`https://marginalis.sandi05.com`。 |
| `MARGINALIS_LISTEN_ADDR` | 必須 | HTTP待受アドレス。例: `127.0.0.1:3000`。 |
| `MARGINALIS_DATA_DIR` | 必須 | AsciiDoc正本とSQLite DBを置く永続ディレクトリ。 |
| `MARGINALIS_MCP_ENABLE` | 任意 | `true`の場合だけOAuth保護されたMCP endpointを公開する。既定値は`false`。 |
| `MARGINALIS_MCP_CLIENT_METADATA_ALLOWED_HOSTS` | MCP有効時に推奨 | Client ID Metadata Documentを取得してよいHTTPS hostのカンマ区切り一覧。空なら未知clientを取得しない。 |
| `OIDC_ISSUER_URL` | 必須 | OIDC issuer。Kanidmのclientごとのissuerとして`https://id.sandi05.com/oauth2/openid/marginalis`を設定する。 |
| `OIDC_CLIENT_ID` | 必須 | OIDC Client ID。現在は`marginalis`。 |
| `OIDC_CLIENT_SECRET` | 必須 | OIDC Client Secret。secret管理機構から環境変数へ注入する。 |
| `OIDC_CLIENT_SECRET_FILE` | 代替 | `OIDC_CLIENT_SECRET`の代わりに、secretだけを含むファイルを指定する。 |
| `RUST_LOG` | 任意 | 構造化ログの粒度。未指定時は`info`。例: `RUST_LOG=debug`。 |
| `ROOT_PASSWORD` | 初回のみ必須 | 未初期化DBへ緊急管理者`root`を作るパスワード。 |
| `ROOT_PASSWORD_FILE` | 代替 | `ROOT_PASSWORD`の代わりに、初期化用password fileを指定する。 |

IdPへ登録するredirect URIは次である。

```text
https://marginalis.sandi05.com/auth/oidc/callback
```

OIDC認可要求は`openid profile email` scopeとAuthorization Code Flow、PKCE S256を使用する。

## Secretの扱い

`OIDC_CLIENT_SECRET`と`ROOT_PASSWORD`は、Git、SQLite、通常の設定ファイルおよびログへ
保存してはならない。デプロイ環境のsecret管理機構（コンテナorchestrator、systemd credential、
ホスティング基盤のsecret注入等）から環境変数または`*_FILE`で渡す。

`ROOT_PASSWORD`は初回起動時にArgon2id hashとしてDBへ保存される。初期化済みDBでは不要であり、
設定しても既存のrootパスワードを変更しない。

初期実装では、保留OIDCユーザーをrootがREST APIから有効化できる。endpointとCSRFの扱いは
[REST API仕様](docs/rest-api.md#root管理)を参照する。rootのパスワードはコマンド履歴、process引数、
ログへ残してはならない。

## 再基線化とデータ初期化

REST APIとMCPを共通のapplication層へ接続する再基線化では、既存のdataDirを移行しない。既存
データの破棄は、サービス停止後に運用者が明示して行う。通常の起動、`nixos-rebuild`および
Marginalis自身がdataDirを自動削除することはない。NixOSでの正確な手順は
[NixOS運用](docs/nixos.md#api-first再基線化時の初期化)を参照する。

## MCP

MCPは既定で無効である。有効化、OAuth Authorization Code + PKCE、Client ID Metadata Document、
利用可能なtool、tokenの期限とローテーションは[MCP仕様](docs/mcp.md)を参照する。

## 現在の範囲と後続作業

初期公開の範囲はREST API、rootによる保留OIDCユーザー承認、OAuth保護されたMCPである。ブラウザー
編集UI、数式・コードのクライアント描画、Device Authorization Grant、ベクトル／あいまい検索および
専用管理origin・mTLSは後続とする。実装順と未完了項目は[issues](issues/README.md)で管理する。
