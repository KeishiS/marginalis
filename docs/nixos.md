# NixOS での運用

v3 の NixOS 設定は [v0.3.0 運用契約](v0.3.0-operations.md) を正とします。本書は設定の要点と日常操作を
補足します。v0.2 の root、`dataDir` 内の AsciiDoc 正本、`/api/v1` の手順は適用しません。

```nix
services.marginalis = {
  enable = true;
  baseUrl = "https://notes.example.test/marginalis";
  listenAddress = "127.0.0.1:3000";
  backupDirectory = "/srv/marginalis-backups";
  oidc = {
    issuerUrl = "https://id.example.test/oauth2/openid/marginalis";
    clientId = "marginalis";
    clientSecretFile = "/run/secrets/marginalis-oidc-client-secret";
    caCertificateFile = "/run/secrets/marginalis-kanidm-ca.pem";
  };
  mcp.enable = true;
};
```

`clientSecretFile` は systemd credential として渡され、Nix store に現れてはなりません。内部 CA の
Kanidm を使う場合は `caCertificateFile` に PEM trust anchor を指定し、OIDC Discovery と token exchange
に適用します。

reverse proxy は `/auth/`、`/api/`、`/mcp`、`/.well-known/`、`/oauth/` を同一オリジンへ転送します。
サブパスでは外部 prefix を upstream へ渡す前に除去し、`baseUrl` と OIDC redirect URI を一致させます。

## Kanidm の group claim

Marginalis は OIDC callback で署名検証済み ID token の `groups` claim を読む。`server-users` がなければ
ログインを拒否し、`server-admins` があれば発行する Web session と MCP authorization を管理者として固定する。
Kanidm の group 変更は次回 OIDC login から反映され、既存の Web session と MCP token は有効期限または
明示的な認可取消までその時点の権限を保つ。

Kanidm の OAuth2 client は `groups_name` scope を許可し、文字列配列の `groups` claim に `server-users` と
`server-admins` を含めるよう設定する。管理者も必ず `server-users` の member にする。

```bash
kanidm system oauth2 add-redirect-url marginalis \
  https://marginalis.sandi05.com/auth/oidc/callback
kanidm system oauth2 update-scope-map marginalis \
  server-users openid profile email groups_name
```

redirect URI がすでに登録済みなら最初のコマンドは不要である。Marginalis は Kanidm REST API を照会しないため、
service account、API token、custom ACP は不要である。

## 定期処理

- `marginalis-purge-deleted.timer` は毎日実行され、30 日を超えたソフトデリート済みノートを削除します。
- `marginalis-backup.service` は `backupDirectory` を設定した場合だけ有効です。HTTP service と競合するため、
  週末の停止枠で `systemctl start marginalis-backup.service` を実行します。
- backup service の完了後も `marginalis.service` は停止したままです。確認後に
  `systemctl start marginalis.service` で明示的に再開します。
- backup は `marginalis-v3-archive.json` を含む時刻付きディレクトリです。空の v3 database へ
  `marginalis import-archive --input <absolute-file>` で取り込めます。定期復元試験は v3 の release gate 外です。

初回配備後は `GET /api/v2/health`、OIDC login、一般利用者と管理者の可視性、MCP authorization を確認します。
