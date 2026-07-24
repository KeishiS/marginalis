# NixOS での運用

この文書は、NixOS モジュールを使って Marginalis を配備・保守する運用者を対象とします。
初めて配備する場合は「セットアップ」から「適用後の確認」までを順に実施してください。
日常の保守では、監査ログ、投影再構築、バックアップと復元の各節を参照します。

## セットアップ

GitHub の flake input から NixOS モジュールを取り込みます。

```nix
{
  inputs.marginalis.url = "github:KeishiS/Marginalis/v0.2.0-rc.1";
  outputs = { self, nixpkgs, marginalis, ... }: {
    nixosConfigurations.example = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        marginalis.nixosModules.default
        {
          services.marginalis = {
            enable = true;
            # リバースプロキシを使わず、待ち受けを直接公開する場合だけ有効にする。
            openFirewall = false;
            # journalctl で観測する tracing フィルター。
            logFilter = "info,marginalis_auth_oidc=info";
            baseUrl = "https://marginalis.sandi05.com";
            listenAddress = "127.0.0.1:3000";
            # 新規 SQLite DB にだけ適用される。既存 DB の設定は上書きしない。
            initialRegistrationPolicy = "approval";
            oidc = {
              issuerUrl = "https://id.sandi05.com/oauth2/openid/marginalis";
              clientId = "marginalis";
              clientSecretFile = "/run/secrets/marginalis-oidc-client-secret";
            };
            initialRootPasswordFile = "/run/secrets/marginalis-root-password";
            mcp = {
              enable = true;
              clientMetadataAllowedHosts = [ "clients.example.org" ];
            };
          };
        }
      ];
    };
  };
}
```

### シークレットの扱い

`clientSecretFile` と `initialRootPasswordFile` には、実行時に読めるファイルの絶対パスを
指定します。シークレットの値そのものを Nix ストアに書き込んではいけません。sops-nix や
agenix を使う場合は、復号後のランタイムファイルのパスを指定してください。モジュールは
systemd の `LoadCredential` でシークレットを渡すため、ユニットの環境変数にシークレットが
現れることはありません。

`initialRootPasswordFile` が必要なのは初回起動時だけです。root の初期化が済んだら、この
オプションは設定から削除できます。

### データの永続化

AsciiDoc 正本と `FORMAT` マーカーは `dataDir`（既定値 `/var/lib/marginalis`）に永続化
されます。SQLite の既定値は `sqlite:<dataDir>/marginalis.sqlite` です。`databaseUrl` を
明示した場合は SQLite を `dataDir` の外へ置けるため、バックアップ、復元、破棄では両方を
同じ保存データとして扱ってください。`initialRegistrationPolicy` は新規データベースの作成時に
だけ `open` または `approval` を書き込みます。既存データベースでは、root API から変更した
現在の登録ポリシーが優先され、`nixos-rebuild` で上書きされることはありません。

## リバースプロキシとの関係

`openFirewall` は `listenAddress` の TCP ポートをファイアウォールで許可するだけです。既定の
`127.0.0.1:3000` は外部から到達できないため、公開にはベース URL を終端するリバースプロキシが
必要です。

- プロキシは `/auth/`・`/api/`・`/mcp`・`/.well-known/`・`/oauth/` を同じオリジンへ転送
  します。TLS はプロキシで終端して構いませんが、`baseUrl` には外部から見える HTTPS URL を
  設定します。
- `baseUrl` にパス接頭辞を含める場合は、外部では各パスへ同じ接頭辞を付け、Marginalis へ
  転送する前にその接頭辞を除去します。例えば `https://example.org/marginalis` なら、外部の
  `/marginalis/api/v1/health` を内部の `/api/v1/health` へ転送します。
- Marginalis 自身が全応答に `Content-Security-Policy: default-src 'none'; base-uri 'none';
  form-action 'self'; frame-ancestors 'none'`、`X-Content-Type-Options: nosniff`、
  `Referrer-Policy: no-referrer` を付与します。`/acceptance` だけは同一オリジンの静的 CSS を
  読み込むため、CSP に `style-src 'self'` も加わります。プロキシでこれらを削除・緩和しないで
  ください。
- Cookie を伴う変更操作では `Origin` を検証し、`Sec-Fetch-Site` がある場合はその値も
  検証します。プロキシでこれらのブラウザー由来ヘッダーを削除・書き換えしないでください。
  `/acceptance` のサーバー生成 HTML フォームは、セッション連動 CSRF トークンを hidden
  field で送るため、このヘッダー要件の対象外です。
- Marginalis は `X-Forwarded-For` や `Forwarded` などのクライアント IP ヘッダーを信頼せず、
  認可判断にも使いません。

### プロキシ側のレート制限

root ログインの接続元レート制限は、Marginalis 内では TCP ピアアドレスだけを使う補助的な
制限です。プロキシ配下で利用者ごとの制限を行う場合は、プロキシ側で設定します。root ログインに
加えて、未認証で開始できる `/auth/oidc/login` と `/oauth/authorize` にも利用者 IP 単位の
制限を掛けてください。Nginx の例を示します。`http` ブロックで:

```nginx
limit_req_zone $binary_remote_addr zone=marginalis_login:10m rate=10r/m;
```

`/auth/root/login`・`/auth/oidc/login`・`/oauth/authorize` の各 location で:

```nginx
limit_req zone=marginalis_login burst=5 nodelay;
proxy_pass http://127.0.0.1:3000;
proxy_set_header Host $host;
proxy_set_header X-Forwarded-Proto $scheme;
```

forwarded 系ヘッダーをプロキシのレート制限のために Marginalis へ渡すのは問題ありませんが、
Marginalis がそれを認可判断に使うことはありません。

## ログ

`logFilter` は `RUST_LOG` として本体、投影再構築、監査削除、バックアップの各ユニットに
渡されます。障害調査では
`"info,marginalis_auth_oidc=debug"` のように、必要なモジュールだけを一時的に debug へ
上げてください。リクエストボディ、パスワード、OIDC コード、トークン、シークレットをログに
出力する設定は存在しません。

失敗の詳細は `journalctl -u marginalis.service -b --no-pager` で確認し、応答の
`X-Request-Id` と対応付けます。

## 適用後の確認

`nixos-rebuild switch` の後、次を順に確認します。

1. `systemctl status marginalis.service` が `active (running)` である。
2. `curl -fsS https://marginalis.sandi05.com/api/v1/health` がヘルスレスポンスを返す。
3. `curl -fsS https://marginalis.sandi05.com/api/v1/readiness` が `ready` を返す。`503` の
   場合は OIDC Discovery の障害により root 限定の縮退起動中です。
4. `https://marginalis.sandi05.com/auth/oidc/login` から Kanidm へ遷移し、ログイン後に
   ベース URL へ戻る。
5. 初回は root ログイン後に `GET /api/v1/admin/users/pending` で保留ユーザーを確認し、
   有効化する（次節）。
6. MCP を有効にした場合は `/.well-known/oauth-protected-resource/mcp` が JSON を返す。

OIDC Discovery が一時的に失敗しても、サービスは root 緊急ログインだけを有効にして起動
します。この間は通常の OIDC ログインを利用できないため、IdP の復旧後に
`sudo systemctl restart marginalis.service` で Discovery をやり直してください。

## 保留 OIDC ユーザーの承認

`dataDir` を新規に初期化すると、root 資格情報・登録ポリシー・OIDC ユーザー・セッションも
すべて新規になります。`approval` ポリシーでは、利用者が一度 OIDC ログインして保留ユーザーを
作成した後、root が次の手順で有効化します。

1. 初回起動時にだけ、NixOS のシークレット設定から `initialRootPasswordFile` を渡します。
   起動後、root 資格情報が SQLite に保存されたことを確認し、この初期化用シークレットを
   通常運用の設定から外します。
2. 承認対象の利用者が `/auth/oidc/login` を完了します。この時点では通常セッションは
   得られず、保留ユーザーだけが作成されます。
3. `POST /auth/root/login` へ root パスワードを送ります。成功すると root セッション Cookie
   と CSRF Cookie が発行されます。
4. root セッションで `GET /api/v1/admin/users/pending` を実行し、承認対象の `user_id` を
   確認します。
5. 同じセッションで `PUT /api/v1/admin/users/{user_id}/activate` を実行します。Cookie を
   伴う変更操作なので、`X-CSRF-Token` と公開オリジンに一致する `Origin` が必要です。
   `Sec-Fetch-Site` を送る場合は `same-origin` を指定します。成功すると `204` を返します。
6. 対象の利用者が改めて OIDC ログインし、`GET /api/v1/session` が `200` かつ
   `is_root: false` を返せば承認完了です。root セッションが不要になったら
   `POST /auth/logout` で終了します。

root パスワードの誤入力は 15 分間に 5 回までに制限されます。リバースプロキシ配下では
Marginalis から見た接続元がプロキシになるため、利用者単位の制限はプロキシ側で設定して
ください。

`curl` と `jq` による実行例を示します。`BASE_URL` は外部公開 URL、`ORIGIN` はその scheme と
ホストだけにします。root パスワードをコマンド引数・履歴・設定ファイルに書かないでください。

```sh
set -eu

BASE_URL='https://marginalis.sandi05.com'
ORIGIN='https://marginalis.sandi05.com'
COOKIE_JAR="$(mktemp)"
trap 'rm -f "$COOKIE_JAR"' EXIT
read -s ROOT_PASSWORD

{
  printf '{"password":'
  printf '%s' "$ROOT_PASSWORD" | jq -Rs .
  printf '}'
} | curl --fail-with-body --silent --show-error \
  --cookie-jar "$COOKIE_JAR" \
  --header 'Content-Type: application/json' \
  --data-binary @- \
  --output /dev/null \
  --write-out 'root login: HTTP %{http_code}\n' \
  "$BASE_URL/auth/root/login"
unset ROOT_PASSWORD

curl --fail-with-body --silent --show-error \
  --cookie "$COOKIE_JAR" \
  "$BASE_URL/api/v1/admin/users/pending" | jq .

CSRF_TOKEN="$(awk '$6 == "marginalis_csrf" { print $7 }' "$COOKIE_JAR")"
[ -n "$CSRF_TOKEN" ]
read -r PENDING_USER_ID

curl --fail-with-body --silent --show-error \
  --cookie "$COOKIE_JAR" \
  --header "X-CSRF-Token: $CSRF_TOKEN" \
  --header "Origin: $ORIGIN" \
  --header 'Sec-Fetch-Site: same-origin' \
  --request PUT \
  --output /dev/null \
  --write-out 'activation: HTTP %{http_code}\n' \
  "$BASE_URL/api/v1/admin/users/$PENDING_USER_ID/activate"

curl --fail-with-body --silent --show-error \
  --cookie "$COOKIE_JAR" \
  --header "X-CSRF-Token: $CSRF_TOKEN" \
  --header "Origin: $ORIGIN" \
  --header 'Sec-Fetch-Site: same-origin' \
  --request POST \
  --output /dev/null \
  --write-out 'root logout: HTTP %{http_code}\n' \
  "$BASE_URL/auth/logout"
```

最後の root ログアウトは、Cookie をサーバー側でも失効させるため省略しないでください。

## 監査ログの確認

root の監査ログは SQLite に 365 日間保持されます。閲覧用の HTTP API はないため、必要な
ときにサーバー上で読み取り専用に確認します。次の例は `databaseUrl` が既定値の場合です。
別の URL を設定した場合は、その SQLite ファイルを指定してください。

```sh
sudo -u marginalis sqlite3 /var/lib/marginalis/marginalis.sqlite \
  'SELECT action, actor_user_id, target_user_id, target, occurred_at_ms
   FROM root_audit_log ORDER BY audit_id DESC LIMIT 100;'
```

この表に、パスワード、Cookie、セッション ID、OIDC コード、トークンおよびそのハッシュが
保存されることはありません。

## 正本からの投影再構築

バックアップから AsciiDoc 正本を復元した場合や、保守作業で `dataDir/notes/` を直接修正した
場合は、SQLite の検索・アンカー・参照投影を正本から再構築します。

```sh
sudo systemctl start marginalis-rebuild-projections.service
sudo systemctl start marginalis.service
```

再構築ユニットは HTTP サーバーと競合するため、起動時に `marginalis.service` を停止します。
すべての `.adoc` 正本について UTF-8・ノートプロファイル・ファイル名と `note-id` の一致を
先に検証し、その後に 1 つの SQLite トランザクションで投影を置き換えます。検証に失敗した
場合、最後に成功した投影は変更されません。既存ノートの ACL は保持され、正本が存在しなく
なったノートの投影と ACL だけが削除されます。

## バックアップと復元

AsciiDoc 正本と SQLite は、保存場所が別でも 1 組として扱います。`backupDirectory` には、
同じファイルシステムまたは別ボリューム上の絶対パスを指定します。モジュールはこの
ディレクトリを `marginalis` ユーザー所有で用意し、その直下に時刻付きのバックアップ世代を
作ります。

ここで説明する復元は、v0.2.0 系列の新しいデータフォーマット v1 で作ったバックアップに
限ります。v0.1.0 以前または AdocWeave v0.6.1 移行前のバックアップは、同じ `FORMAT=1` でも
互換性を判定できないため、復元入力に使用してはいけません。

```nix
services.marginalis.backupDirectory = "/var/lib/marginalis-backups";
```

バックアップ先の永続化、世代管理、遠隔複製、保持期間は運用者が決めます。自動タイマーは
意図的に提供していません。

バックアップは、HTTP サービスを停止した状態で次の oneshot ユニットが作成します。

```sh
sudo systemctl start marginalis-backup.service
sudo systemctl start marginalis.service
```

成功した各世代には `FORMAT`、`marginalis.sqlite`、`notes/<UUID>.adoc`、`MANIFEST`、
`COMPLETE` マーカーが含まれます。`MANIFEST` にはフォーマットバージョン、作成時刻、SQLite と
全正本の SHA-256 が記録されます。同じ出力パスが既に存在する場合は上書きせずに失敗します。
失敗時には不完全な出力が残ることがあるため、上記ファイルが揃っていないディレクトリを復元に
使わないでください。NixOS モジュールは実行バイナリをシステム全体の `PATH` へ追加しないため、
手動バックアップより、設定値と資格情報を引き継ぐ上記の systemd ユニットを使用します。

復元では、稼働中の保存データへ触れずに、検証済みの候補を新しいパスへ作成します。

```sh
nix run github:KeishiS/Marginalis/v0.2.0-rc.1 -- \
  restore --input <完全なバックアップ世代> --output <存在しない絶対パス>
```

このコマンドは、フォーマットマーカー、マニフェストのハッシュ、SQLite の
`integrity_check`、全正本の UTF-8・ノートプロファイル・ファイル名と note ID の一致を確認した
うえで、検証済みの SQLite と正本を新しい出力ディレクトリへ複製し、`RESTORED` マーカーを
作ります。既存の `dataDir` は変更しません。

実際にどの `dataDir` へ切り替えるかは、同じ新 v1 内で旧データを保持してロールバックできる
ようにするための運用判断です。出力を採用する場合にだけ
`services.marginalis.dataDir = "<出力先>";` とします。`databaseUrl` を明示している構成では、
復元先の `<出力先>/marginalis.sqlite` を指すよう同時に変更します。その後、
`marginalis-rebuild-projections.service`、続けて `marginalis.service` を起動します。既存の
保存データの削除や上書きは、この確認の後に対象パスを明示して別途行ってください。

## MCP の公開

`services.marginalis.mcp.enable` の既定値は `false` です。`true` の場合にだけ、同じベース
URL の下に `/mcp`、OAuth 認可サーバー、Protected Resource Metadata が公開されます。リバース
プロキシを使う場合は、これらのパスも同じオリジンへ転送してください。

`clientMetadataAllowedHosts` は、未知の OAuth クライアントの `client_id` URL からメタデータを
取得してよい HTTPS ホストの許可リストです。この制約により、認可エンドポイントが任意の内部
URL を取得する SSRF の入口になることを防ぎます。空リストでも SQLite に登録済みのクライアントは
利用できますが、初期運用ではメタデータのホストを明示して登録する方式を推奨します。
クライアント側の設定は [MCP と OAuth](mcp.md) を参照してください。

## v0.2.0 系列へ初めて更新する

AdocWeave v0.6.1 への更新では、データフォーマット v1 の識別子を維持したまま内容を
破壊的に上書きします。旧 v1 を検出する仕組みや移行経路はありません。更新前の `dataDir`、
バックアップ、投影は再利用せず、新しいスキーマ、OIDC の利用者情報、ACL、セッション、
`root` 資格情報、AsciiDoc 正本を空の状態から作成します。アプリケーションと
NixOS モジュールが、通常の起動や `nixos-rebuild` で既存 `dataDir` を削除することは
ありません。旧 `dataDir` も `FORMAT` 値は v1 なので、指定したまま起動すれば安全に識別できません。
必ず運用者が削除を完了してから新しいバイナリを起動してください。

既存データを破棄して切り替える場合は、次の順序で行います。

1. `marginalis.service` と `marginalis-prune-audit.timer` を停止し、投影再構築、監査削除、
   バックアップの各 oneshot ユニットが動作していないことを確認する。
2. 設定中の `services.marginalis.dataDir` と `databaseUrl` を確認する。必要なら旧データを、
   更新後の復元には使わないオフラインの退避物として別の場所へ保存する。
3. 削除対象が設定値と一致することを再確認し、旧 `dataDir` 全体を明示して削除する。
   `databaseUrl` が `dataDir` の外の SQLite を指す場合は、その DB ファイルと対応する
   `-wal`・`-shm` も明示して削除するか、新しい空の DB パスへ変更する。
4. モジュール設定を新しいリビジョンへ更新し、`nixos-rebuild switch` を実行する。systemd の
   `StateDirectory`（既定の `dataDir`）または tmpfiles（カスタム `dataDir`）により、空の
   データディレクトリが専用ユーザー所有で作成される。
5. `initialRootPasswordFile` を一度だけ指定して root を初期化し、OIDC ログイン、
   `GET /api/v1/health`、`GET /api/v1/session` を確認する。
6. root の初期化後、`initialRootPasswordFile` を設定から除去する。
