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
    membershipApiUrl = "https://id.example.test";
    membershipTokenFile = "/run/secrets/marginalis-kanidm-membership-token";
  };
  mcp.enable = true;
};
```

`membershipTokenFile` は Kanidm の person entry に対する `memberof` 読み取りだけを許可した
service-account token を指定します。二つの secret file は systemd credential として渡され、Nix store に
現れてはなりません。内部 CA の Kanidm を使う場合は `caCertificateFile` に PEM trust anchor を指定する。
これは OIDC Discovery と membership API の双方に適用される。

reverse proxy は `/auth/`、`/api/`、`/mcp`、`/.well-known/`、`/oauth/` を同一オリジンへ転送します。
サブパスでは外部 prefix を upstream へ渡す前に除去し、`baseUrl` と OIDC redirect URI を一致させます。

## 定期処理

- `marginalis-purge-deleted.timer` は毎日実行され、30 日を超えたソフトデリート済みノートを削除します。
- `marginalis-backup.service` は `backupDirectory` を設定した場合だけ有効です。HTTP service と競合するため、
  週末の停止枠で `systemctl start marginalis-backup.service` を実行します。
- backup service の完了後も `marginalis.service` は停止したままです。確認後に
  `systemctl start marginalis.service` で明示的に再開します。
- backup は `marginalis-v3-archive.json` を含む時刻付きディレクトリです。空の v3 database へ
  `marginalis import-archive --input <absolute-file>` で取り込めます。定期復元試験は v3 の release gate 外です。

初回配備後は `GET /api/v2/health`、OIDC login、一般利用者と管理者の可視性、MCP authorization を確認します。
