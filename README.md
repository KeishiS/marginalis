# Marginalis

Marginalisは、研究ノートをAsciiDoc形式で作成・管理するセルフホスト型のナレッジベースです。
Kanidmで利用者の認証とグループ管理を行い、Web UI、REST API、MCPを提供します。

## 仕様

- ノートの本文、題名、タグ、アクセス権限、削除状態はSQLiteで管理します。
  ノート単位のAsciiDoc書き出しと、全データのJSON形式での読み込み・書き出しができます。
- Kanidmの`server-users`グループに属する利用者がアクセスできます。
  `server-admins`グループに属する利用者は、すべてのノートを閲覧・管理できます。
- `/api/v2`は公開REST APIです。仕様は[OpenAPI](docs/openapi.json)を参照してください。
- MCPは同一オリジンのStreamable HTTPエンドポイントとOAuth 2.1 Authorization Code + PKCE
  S256を提供します。クライアントはDynamic Client Registration（動的クライアント登録）を
  利用できます。
- 削除したノートは30日間保管し、日次のNixOSタイマーが期限を過ぎたデータを物理削除します。

## 利用と運用

NixOSの設定、秘密情報、バックアップについては[NixOSでの運用](docs/nixos.md)、
MCPへの接続方法については[MCPとOAuth](docs/mcp.md)を参照してください。

Marginalisを直接起動するには、少なくとも次の情報が必要です。

- `MARGINALIS_DATABASE_URL`
- `MARGINALIS_BASE_URL`
- `MARGINALIS_LISTEN_ADDR`
- `OIDC_ISSUER_URL`
- `OIDC_CLIENT_ID`
- OIDCクライアントシークレット

秘密情報は`*_FILE`または実行環境の認証情報を渡す仕組みを使い、Git、Nix store、SQLite、ログへ
保存しないでください。

## 開発

設計上の規則は[要件定義](docs/requirements.md)と[アーキテクチャ](docs/architecture.md)、
今後の作業計画は[ロードマップ](docs/roadmap.md)を参照してください。

検証は次のコマンドで実行できます。

```text
cargo make format
cargo make lint
cargo make test
cargo make verify
```

詳細は[文書案内](docs/README.md)を参照してください。
