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

## Kanidm membership service account

`membershipTokenFile` は Marginalis 固有の Kanidm service account の read-only API token である。OIDC
ID token だけではログイン後の group 変更を検出できないため、Marginalis はこの token で
`GET /v1/person/{subject}` を照会し、`memberof` を最大 5 分ごとに再確認する。

NixOS の `services.kanidm.provision` は person、group、OAuth2 client を宣言できるが、Kanidm 1.10
では service account と API token を直接宣言できない。token は平文を生成時に一度だけ取得する秘密情報
でもある。従って、service account は初回に管理者 CLI で作成し、token は sops-nix、agenix 等の secret
manager で `membershipTokenFile` の場所へ配置する。

以下は `idm_admin` で初回設定する例である。`id.example.test` と CA ファイルのパスは実環境へ置き換える。

```bash
kanidm login \
  --url https://id.example.test \
  --ca /etc/ssl/kanidm-ca.pem \
  --name idm_admin

kanidm service-account create \
  marginalis-membership \
  "Marginalis membership resolver" \
  idm_admin

kanidm service-account api-token generate \
  marginalis-membership \
  "marginalis-production-2026"
```

最後のコマンドは token を一度だけ表示する。`--readwrite` は指定しない。表示値を root のみが読める秘密
ファイルへ保存し、NixOS 設定の `membershipTokenFile` と一致させる。module は systemd の
`LoadCredential` を使うため、PID 1 がこの元ファイルを読み、実行時に限って `marginalis` サービスへ
credential を渡す。したがって元ファイルを `marginalis` ユーザー所有・可読にする必要はない。token を
shell history、Nix 式、journal、Git に書き込んではならない。

```bash
sudo install -m 0600 -o root -g root /dev/null \
  /run/secrets/marginalis-kanidm-membership-token
sudoedit /run/secrets/marginalis-kanidm-membership-token
```

service account 自体には person entry の `memberof` を読む最小限の Kanidm access control profile
(ACP) を group 経由で付与する。Kanidm 1.10 の CLI には ACP を作るサブコマンドがないため、次の初回
設定では raw API を一度だけ使う。`idm_people_admins` のような広い組み込み group を service account へ
恒久的に付与してはならない。

まず ACP の receiver group を作り、service account をその group のみに加入させる。

```bash
kanidm group create marginalis-membership-readers idm_admin
kanidm group add-members marginalis-membership-readers marginalis-membership
```

ACP 作成だけには read-write token と `idm_access_control_admins` の一時的な権限が必要である。次の
token は作成直後だけ使用し、後続の手順で必ず失効させる。表示された token は shell history に保存せず、
プロンプトへ貼り付ける。

```bash
kanidm group add-members idm_access_control_admins marginalis-membership
kanidm service-account api-token generate --readwrite \
  marginalis-membership "one-time ACP bootstrap"

read -r -s -p 'Paste one-time token: ' BOOTSTRAP_TOKEN
echo
```

`id.example.test` と CA ファイルは実環境の値へ置き換える。以下の ACP は `person` entry だけを対象にし、
返却可能な属性を `memberof` のみに制限する。

```bash
curl --fail --show-error --cacert /etc/ssl/kanidm-ca.pem \
  -H "Authorization: Bearer $BOOTSTRAP_TOKEN" \
  -H 'Content-Type: application/json' \
  --data @- \
  https://id.example.test/v1/raw/create <<'JSON'
{
  "entries": [
    {
      "attrs": {
        "class": [
          "object",
          "access_control_profile",
          "access_control_search",
          "access_control_receiver_group",
          "access_control_target_scope"
        ],
        "name": ["marginalis_membership_read"],
        "description": ["Allow Marginalis to read person memberof only."],
        "acp_receiver_group": ["marginalis-membership-readers"],
        "acp_targetscope": [
          "{\"and\":[{\"eq\":[\"class\",\"person\"]},{\"andnot\":{\"or\":[{\"eq\":[\"class\",\"recycled\"]},{\"eq\":[\"class\",\"tombstone\"]}]}}]}"
        ],
        "acp_search_attr": ["memberof"]
      }
    }
  ]
}
JSON
```

成功後、temporary な権限と token を必ず除去する。`<bootstrap-token-uuid>` は
`api-token status` の UUID へ置き換える。最後に生成する read-only token だけを
`membershipTokenFile` へ保存する。

```bash
unset BOOTSTRAP_TOKEN
kanidm group remove-members idm_access_control_admins marginalis-membership
kanidm service-account api-token status marginalis-membership
kanidm service-account api-token destroy \
  marginalis-membership <bootstrap-token-uuid>

kanidm service-account api-token generate \
  marginalis-membership "marginalis-production-2026"
```

新しい read-only token で `GET /v1/person/<test-user>` を実行し、応答に `memberof` 以外の不要な
属性が含まれないことを確認する。失敗時は service account が
`idm_access_control_admins` に残っていないことを先に確認する。

token の状態とローテーションは次で扱う。新 token を secret manager へ反映し、Marginalis を再起動して
から、旧 token の UUID を失効させる。

```bash
kanidm service-account api-token status marginalis-membership
sudo systemctl restart marginalis.service
kanidm service-account api-token destroy marginalis-membership <old-token-uuid>
```

## 定期処理

- `marginalis-purge-deleted.timer` は毎日実行され、30 日を超えたソフトデリート済みノートを削除します。
- `marginalis-backup.service` は `backupDirectory` を設定した場合だけ有効です。HTTP service と競合するため、
  週末の停止枠で `systemctl start marginalis-backup.service` を実行します。
- backup service の完了後も `marginalis.service` は停止したままです。確認後に
  `systemctl start marginalis.service` で明示的に再開します。
- backup は `marginalis-v3-archive.json` を含む時刻付きディレクトリです。空の v3 database へ
  `marginalis import-archive --input <absolute-file>` で取り込めます。定期復元試験は v3 の release gate 外です。

初回配備後は `GET /api/v2/health`、OIDC login、一般利用者と管理者の可視性、MCP authorization を確認します。
