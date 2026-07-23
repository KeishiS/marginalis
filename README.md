# Marginalis

Marginalisは、研究ノート、引用、断片的なアイディアを収集し、ノート間の参照として整理する
セルフホスト型の研究ノート環境である。

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
