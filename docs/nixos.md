# NixOS での運用

本書を現行のNixOS設定と日常運用の正本とします。v0.2 の root、`dataDir` 内の
AsciiDoc 正本、`/api/v1` の手順は適用しません。

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
  mcp = {
    enable = true;
    # /mcp を browser から呼ぶ client の HTTPS origin だけを列挙する。
    allowedOrigins = [ "https://chatgpt.com" ];
  };
};
```

`clientSecretFile` は systemd credential として渡され、Nix store に現れてはなりません。内部 CA の
Kanidm を使う場合は `caCertificateFile` に PEM trust anchor を指定し、OIDC Discovery と token exchange
に適用します。
SQLite正本は`dataDir`（既定値`/var/lib/marginalis`）直下の`marginalis.sqlite`に固定します。
任意のdatabase URLは指定できません。正本を別volumeへ置く場合は、`dataDir`自体をその絶対pathへ
変更してください。現行のSQLite schema versionは2です。旧versionを自動移行しないため、このOAuth
再設計より前のdatabaseを使っている場合はarchiveを退避してから空のdatabaseとして再初期化します。
再初期化後はMCP clientの再登録と利用者の再認可が必要です。

reverse proxy は `/auth/`、`/api/`、`/mcp`、`/.well-known/`、`/oauth/` を同一オリジンへ転送します。
サブパスでは通常endpointの外部prefixをupstreamへ渡す前に除去します。一方、RFC 8414/9728の
`/.well-known/` URLはhost rootから始まりsubject pathを末尾に持つため、pathを除去せずupstreamへ渡します。
`baseUrl` と OIDC redirect URI は一致させます。
`mcp.allowedOrigins` は HTTPS origin の完全一致であり、path、query、userinfo を含む値や HTTP origin は
起動時に拒否されます。この設定は `/oauth/authorize` の承認 form には適用されません。

## Kanidm の group claim

Marginalis は OIDC callback で署名検証済み ID token の `groups` claim を読む。`server-users` がなければ
ログインを拒否し、`server-admins` があれば発行する Web session と MCP authorization を管理者として固定する。
Kanidm の group 変更は次回 OIDC login から反映され、既存の Web session と MCP token は有効期限または
明示的な認可取消までその時点の権限を保つ。
Web session は最終利用から24時間で失効し、ログインから7日を絶対期限とする。継続利用中は
アイドル期限だけを延長するため、group変更を直ちに反映する必要がある場合は、対象利用者に再ログインを
依頼するか、7日の絶対期限まで待つ。

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

- `marginalis-purge-expired.timer` は毎日実行され、30 日を超えたソフトデリート済みノートと、
  期限切れ・失効済みのWeb/OIDC/MCP認証状態を削除します。
- `marginalis-backup.service` は `backupDirectory` を設定した場合だけ有効です。単一のSQLite read
  transactionからsnapshotを取得するため、HTTP serviceを停止せずに実行できます。
- backup は `marginalis-archive.json` を含む時刻付きディレクトリです。空の database へ
  `marginalis import-archive --input <absolute-file>` で取り込めます。定期復元試験は v3 の release gate 外です。

初回配備後は `GET /api/v2/health`、OIDC login、一般利用者と管理者の可視性、MCP authorization を確認します。
