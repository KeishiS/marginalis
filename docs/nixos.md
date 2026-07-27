# NixOS での運用

本書を現行のNixOS設定と日常運用の正本とします。v0.2 の root、`dataDir` 内の
AsciiDoc 正本、`/api/v1` の手順は適用しません。

```nix
services.marginalis = {
  enable = true;
  baseUrl = "https://notes.example.test/marginalis";
  listenAddress = "127.0.0.1:3000";
  backupDirectory = "/srv/marginalis-backups";
  backupRetention = 30;
  restoreCheck.enable = true;
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
- `backupDirectory`を設定すると`marginalis-backup.timer`が毎日実行されます。単一のSQLite read
  transactionからsnapshotを取得するため、HTTP serviceを停止しません。
- 成功世代は`backup-<Unix時刻ミリ秒>`というディレクトリで、`marginalis-archive.json`と
  `COMPLETE`を含みます。archiveの検証が完了するまで`COMPLETE`は作成されません。
- `backupRetention`は検証済み成功世代の保持数で、既定値は30です。保持処理は
  `backupDirectory`を正規化し、その直下にある既定名・marker・archiveをすべて検証できた世代だけを
  対象にします。不完全な世代、symlink、運用者が置いたファイルは数えず、削除もしません。
- `restoreCheck.enable = true`を明示すると、最新成功世代を隔離した空の一時databaseへ復元して
  再exportとの論理一致を調べる`marginalis-restore-check.timer`が有効になります。既定scheduleは
  各四半期の最初の土曜日3時です。`restoreCheck.calendar`で変更できます。

保存媒体のsnapshot、off-site複製、暗号化はhost側で構成します。30世代に加え、復元用の一時databaseと
SQLiteの一時領域を確保してください。必要量の目安は、正本と最大archiveの合計に作業余裕を加えた容量です。

## Backupの確認

archive単体の検証と、隔離復元の検証を手動で実行できます。どちらもノート本文を標準出力やlogへ出しません。

```sh
sudo -u marginalis marginalis validate-archive \
  --input /srv/marginalis-backups/backup-<時刻>/marginalis-archive.json
sudo systemctl start marginalis-restore-check.service
sudo journalctl -u marginalis-backup.service -u marginalis-restore-check.service
```

backup作成または検証が失敗した場合、保持処理は実行されず、既存の成功世代は残ります。破損した成功世代を
保持処理が検出した場合も削除前に失敗します。容量不足、権限、mount状態、SQLite errorを上記journalと
`systemctl status`で確認し、原因を直してから再実行してください。不完全なディレクトリを削除する場合は、
`COMPLETE`がないことと対象pathが`backupDirectory`直下であることを運用者が確認します。

## 復元と切戻し

本番databaseへ直接importしてはいけません。次の手順では、先に別の空databaseへ復元し、確認後に
service停止中の切替を行います。

1. 対象archiveを`validate-archive`と`verify-restore`で検証します。

   ```sh
   sudo -u marginalis marginalis verify-restore \
     --input /srv/marginalis-backups/backup-<時刻>/marginalis-archive.json
   ```

2. `dataDir`とは別の永続領域に空の復元先を作り、archiveをimportします。既存ノートまたは認証状態が
   あるdatabaseへのimportは失敗し、暗黙に上書きされません。

   ```sh
   sudo -u marginalis env \
     MARGINALIS_DATABASE_URL=sqlite:/srv/marginalis-restore/marginalis.sqlite \
     marginalis import-archive \
     --input /srv/marginalis-backups/backup-<時刻>/marginalis-archive.json
   ```

3. 復元先を再exportし、`validate-archive`で検証します。ノート数、ACL、削除状態、revisionを
   運用上の期待値とも照合します。archiveはWeb sessionやMCP tokenなどの認証状態を含まないため、
   切替後は再ログインと必要に応じたMCP再認可が必要です。
4. maintenance windowでserviceを停止し、現在の`dataDir`を削除せず退避します。復元済みdatabaseを
   新しい`dataDir`へ配置して所有者とmodeを確認した後、serviceを起動します。
5. health、OIDC login、一般利用者と管理者の可視性、ソフトデリート状態、MCP authorizationを確認します。

切戻しではserviceを再度停止し、復元後のdatabaseを別名で保全してから、手順4で退避した元の`dataDir`へ
戻します。復元先の確認が終わるまで元databaseを上書きまたは削除しないでください。

初回配備後は `GET /api/v2/health`、OIDC login、一般利用者と管理者の可視性、MCP authorization を確認します。

## 障害診断

公開livenessは`GET /api/v2/health`で確認します。この応答はHTTP processの稼働だけを表し、
外部IdPの一時的な停止では失敗しません。SQLiteと設定はserviceと同じ実行環境で診断します。

```bash
curl --fail http://127.0.0.1:3000/api/v2/health
systemctl is-active marginalis.service
systemctl show marginalis.service -p InvocationID -p ExecMainStatus
systemctl show marginalis-purge-expired.service -p Result -p ExecMainStatus
systemctl show marginalis-backup.service -p Result -p ExecMainStatus
systemctl list-timers marginalis-purge-expired.timer
systemctl cat marginalis.service
systemctl start marginalis-diagnose.service
journalctl -u marginalis-diagnose.service -o cat -n 20
```

`diagnose`はSQLiteの読み取り可否、schema version、`PRAGMA quick_check`、
外部キー違反件数と非秘密設定だけをJSONで出力します。全検査が正常な場合だけ終了status 0です。
databaseを作成・移行せず、OIDC client secret、Cookie、token、ノート本文は出力しません。
`status`が`failed`の場合は`database.error`と各検査の`actual`、`expected`を確認します。

主要なjournal event名は次のとおりです。

- service起動: `service.listening`
- OIDC discovery成功・失敗: `oidc.discovery.completed`、`oidc.discovery.failed`
- MCP OAuth token成功・失敗: `mcp.oauth.token.completed`、`mcp.oauth.token.failed`
- MCP OAuth処理成功・失敗: `mcp.oauth.operation.completed`、`mcp.oauth.operation.failed`
  （`operation`は`registration`、`authorization`、`consent`、`revocation`）
- purge成功・失敗: `maintenance.purge.completed`、`maintenance.purge.failed`
- backup成功・失敗: `maintenance.backup.completed`、`maintenance.backup.failed`
- archive検証成功・失敗: `maintenance.archive_validation.completed`、
  `maintenance.archive_validation.failed`
- 復元検証成功・失敗: `maintenance.restore_verification.completed`、
  `maintenance.restore_verification.failed`
- backup検証成功・失敗: `maintenance.backup_verification.completed`、
  `maintenance.backup_verification.failed`
- backup世代整理成功・失敗: `maintenance.backup_prune.completed`、
  `maintenance.backup_prune.failed`
- command失敗: `command.failed`（`command` fieldで保守処理を識別）

```bash
journalctl -u marginalis.service --since today
journalctl -u marginalis.service _SYSTEMD_INVOCATION_ID="$(systemctl show marginalis.service -p InvocationID --value)"
journalctl -u marginalis-purge-expired.service -g 'maintenance.purge.'
journalctl -u marginalis-backup.service -g 'maintenance.backup.'
find /srv/marginalis-backups -mindepth 1 -maxdepth 1 -type d \
  -exec test -f '{}/COMPLETE' ';' -print | sort | tail
```

HTTP logは`request_id`で一連の処理を追跡できます。OIDC到達不能時は
`oidc.discovery.failed`を記録し、loginだけを503で閉じたままlivenessを維持します。
保守unitの失敗はHTTP serviceの停止を意味しません。unitの`Result`と同じinvocationのjournalを確認し、
保存先容量、権限、SQLite診断結果を修正してからunitを再実行します。
