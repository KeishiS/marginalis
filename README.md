# Marginalis

Marginalis は、研究ノートを AsciiDoc で可搬に保ちながら、SQLite を単一の正本として運用する
セルフホスト型ノート環境です。Kanidm 1.10 が本人確認とグループ管理を担い、Web UI、REST API、
MCP は同じ認可規則を使います。

## 現行の契約

- ノート本文・メタデータ・ACL・削除状態の正本は SQLite です。AsciiDoc はノート単位の export、
  JSON archive は全体の import/export 形式です。
- `server-users` の利用者がログインできます。`server-admins` はすべてのノートを読め、管理できます。
- `/api/v2` が公開 REST API です。仕様は [OpenAPI](docs/openapi.json) を参照してください。
- MCP は同一オリジンの Streamable HTTP endpoint と OAuth 2.1 Authorization Code + PKCE S256 を
  提供します。クライアントは Dynamic Client Registration を使えます。
- 削除は 30 日間のソフトデリートで、日次の NixOS timer が期限切れデータを物理削除します。

`v0.2` の `/api/v1`、ローカル root、ファイル正本、既存データは互換対象ではありません。現行版は空の
SQLite database から初期化します。

## 運用

NixOS 設定、秘密情報、backupは[NixOSでの運用](docs/nixos.md)、MCP接続は
[MCPとOAuth](docs/mcp.md)を正とします。製品要件と不変条件は
[要件定義](docs/requirements.md)と[アーキテクチャ](docs/architecture.md)、今後の順序は
[ロードマップ](docs/roadmap.md)にあります。

直接起動には、少なくとも `MARGINALIS_DATABASE_URL`、`MARGINALIS_BASE_URL`、
`MARGINALIS_LISTEN_ADDR`、`OIDC_ISSUER_URL`、`OIDC_CLIENT_ID`、OIDC client secret が必要です。
Kanidm service account や API token は使いません。秘密情報は
`*_FILE` または実行環境の credential 機構で渡し、Git、Nix store、SQLite、ログへ保存しないでください。

## 開発

```text
cargo make format
cargo make lint
cargo make test
cargo make verify
```

公開前には `cargo make release-gate` と、Kanidm 1.10・MCP client・NixOS 配備の受入を実施します。
詳細は [文書案内](docs/README.md) を参照してください。
