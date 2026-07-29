# NixOSでの運用

この文書は、NixOSでMarginalisを配備・運用する人に向けて、設定、秘密情報、データの保存、
バックアップ、復元、問題発生時の確認方法を説明します。以前のバージョンから更新する場合は、
[変更履歴](../CHANGELOG.md)も確認してください。

## 基本設定

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
    allowedOrigins = [ "https://chatgpt.com" ];
    authorization = {
      issuer = "https://example.auth0.com/";
      upstreamIssuerClaim = "https://notes.example.test/claims/upstream-issuer";
      upstreamSubjectClaim = "https://notes.example.test/claims/upstream-subject";
      groupsClaim = "https://notes.example.test/claims/groups";
    };
  };
};
```

`services.marginalis.enable = true;`にすると、サービスが使用する`services.marginalis.package`と
同じパッケージの`marginalis`管理コマンドをシステムの`PATH`へ追加します。別途
`environment.systemPackages`へ追加する必要はありません。反映後は次のコマンドで版を確認できます。

```sh
/run/current-system/sw/bin/marginalis --version
```

## 秘密情報とデータの保存先

`clientSecretFile` は systemd credential として渡され、Nix store に現れてはなりません。内部 CA の
Kanidm を使う場合は `caCertificateFile` に PEM trust anchor を指定し、OIDC Discovery と token exchange
に適用します。
SQLiteデータベースは`dataDir`（既定値`/var/lib/marginalis`）直下の`marginalis.sqlite`に固定します。
任意のdatabase URLは指定できません。正本を別volumeへ置く場合は、`dataDir`自体をその絶対pathへ
変更してください。現行のSQLite schema versionは11です。旧versionを起動時に自動移行しません。
schema 10または9から更新する場合は、AdocWeave 0.11.0を使用する旧実行環境でarchive 7を作成し、
現行の`migrate-archive`でarchive 8へ変換してから、空のschema 11へ取り込んでください。
この経路では全ノートをAdocWeave 0.17.0の規則で再検証し、題名、タグ、参照索引を再構築します。
ノート、所有者、削除状態、revision、ノート間参照、共有権限が一致することをCIで検証しています。

切戻す場合はserviceを停止し、更新後に作成したdatabaseを保全してから、更新前に退避した`dataDir`と
実行環境を組み合わせて戻します。異なる版のserviceを同時に同じdatabaseへ接続してはいけません。
archive 7以外の旧archiveには移行操作を提供しません。

reverse proxyは`/auth/`、`/api/`、`/mcp`、`/.well-known/`を同一オリジンへ転送します。
サブパスでは通常endpointの外部prefixをupstreamへ渡す前に除去します。一方、RFC 8414/9728の
`/.well-known/` URLはhost rootから始まりsubject pathを末尾に持つため、pathを除去せずupstreamへ渡します。
`baseUrl` と OIDC redirect URI は一致させます。
`mcp.allowedOrigins` は HTTPS origin の完全一致であり、path、query、userinfo を含む値や HTTP origin は
起動時に拒否されます。

MCPを有効にする場合は`mcp.authorization`の全項目が必須です。`issuer`にはAuth0 tenantのHTTPS issuerを
末尾の`/`を含めて指定します。三つのclaim名はAuth0 Login Actionがaccess tokenへ格納する名前空間付き
claimと完全に一致させます。MCP URLは`baseUrl`から自動的に導出され、Auth0 API identifierにも同じ値を
設定します。

起動時にAuth0のAuthorization Server MetadataとJWKSを取得できない場合、serviceは起動しません。
Web用Kanidm OIDC discoveryは従来どおり一時的な障害時にfail closedで起動します。二つの挙動を
混同しないでください。

## Kanidmから受け取るグループ情報

MarginalisはOIDC callbackで署名検証済みID tokenの`groups` claimを読み、`server-users`がなければ
ログインを拒否します。サーバー全体の管理者グループは使用しません。Kanidmのgroup変更は次回の
OIDC loginから反映され、既存のWeb sessionとMCP tokenは有効期限または明示的な認可取消まで
login時のidentityを保持します。
Web session は最終利用から24時間で失効し、ログインから7日を絶対期限とする。継続利用中は
アイドル期限だけを延長するため、group変更を直ちに反映する必要がある場合は、対象利用者に再ログインを
依頼するか、7日の絶対期限まで待つ。

KanidmのOAuth2 clientは`groups_name` scopeを許可し、文字列配列の`groups` claimに
`server-users`を含めるよう設定します。

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
  期限切れ・失効済みのWeb/OIDC認証状態を削除します。MCP tokenはAuth0が管理します。
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

## バックアップの確認

archive単体の検証と、隔離復元の検証を手動で実行できます。どちらもノート本文を標準出力やlogへ出しません。
現行archiveは`marginalis-archive-8`で、AdocWeave package版`0.17.0`とnote profile版`4`を記録します。
形式またはいずれかの版が実行中のMarginalisと一致しないarchiveは、databaseを変更する前に拒否されます。
同じ段階で、ノートの識別子、所有者、revision、日時、本文、ACLの参照先と重複も検証します。
本文から参照索引を再構築した後に復元計画が確定するため、検証に失敗した内容の一部だけがdatabaseへ
書き込まれることはありません。

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

## v0.10.0からの移行

v0.10.0のschema 10には、AdocWeave 0.11.0で導出した題名とタグが保存されています。現行版は
AdocWeave 0.17.0でmetadataを再構築するため、databaseファイルを直接引き継ぎません。

1. v0.10.0のserviceとdatabaseを使ってarchive 7を書き出し、同じ実行環境の
   `validate-archive`と`verify-restore`を実行します。
2. v0.10.0のNixOS generation、`dataDir`、archive 7を削除せずに保全します。
3. 現行版の実行ファイルでarchive 7をarchive 8へ変換します。入力と出力には異なる絶対pathを
   指定してください。入力は変更されず、既存の出力を上書きしません。

   ```sh
   sudo -u marginalis <現行版のmarginalis> migrate-archive \
     --input /srv/marginalis-migration/archive-7.json \
     --output /srv/marginalis-migration/archive-8.json
   sudo -u marginalis <現行版のmarginalis> verify-restore \
     --input /srv/marginalis-migration/archive-8.json
   ```

4. 移行が失敗した場合はarchive 8を取り込まず、v0.10.0へ戻ります。改行を残す複数行タグや
   header後の属性操作など、[0.17移行判断](adocweave-v0.17-migration.md)に記載した差を
   v0.10.0で修正し、archive 7の書き出しからやり直します。エラーに示された`position`は、
   archiveの`notes`または`note_acl`配列内の1から始まる位置です。診断へ本文や識別子は
   出力されません。
5. 成功したarchive 8を、次節の手順で空のschema 11へ取り込みます。

CIでは、旧実行環境が作成したschema 9のarchive 7にも同じ移行操作を適用し、入力archiveの不変、
旧archiveの直接取込拒否、schema 11でのノート、ACL、参照、削除状態、revisionの一致、archive 8の
再書き出し一致を検査します。schema 10も同じarchive 7契約を使用するため、移行入口は一つです。

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

3. 復元先を再exportし、`validate-archive`で検証します。ノート数、所有者、削除状態、revisionを
   運用上の期待値とも照合します。archiveはWeb sessionやMCP tokenなどの認証状態を含まないため、
   切替後は再ログインと必要に応じたMCP再認可が必要です。
4. maintenance windowでserviceを停止し、現在の`dataDir`を削除せず退避します。復元済みdatabaseを
   新しい`dataDir`へ配置して所有者とmodeを確認した後、serviceを起動します。
5. health、OIDC login、所有者・ACL共有先・対象外利用者の可視性、ソフトデリート状態、
   MCP authorizationを確認します。

切戻しではserviceを再度停止し、復元後のdatabaseを別名で保全してから、手順4で退避した元の`dataDir`へ
戻します。復元先の確認が終わるまで元databaseを上書きまたは削除しないでください。

初回配備後は`GET /api/v3/health`、OIDC login、所有者・ACL共有先・対象外利用者の可視性、
MCP authorizationを確認します。

## 問題が発生したときの確認

公開livenessは`GET /api/v3/health`で確認します。この応答はHTTP processの稼働だけを表し、
外部IdPの一時的な停止では失敗しません。SQLiteと設定はserviceと同じ実行環境で診断します。

```bash
curl --fail http://127.0.0.1:3000/api/v3/health
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
外部キー違反件数と、Auth0設定の有無を含む非秘密設定だけをJSONで出力します。全検査が正常な場合だけ
終了status 0です。
databaseを作成・移行せず、OIDC client secret、Cookie、token、ノート本文は出力しません。
`status`が`failed`の場合は`database.error`と各検査の`actual`、`expected`を確認します。
SQLを実行できなかった場合は、`database.failures`の`check`で失敗した検査、`category`で
ロック、読み取り専用、入出力エラーなどの分類を確認できます。`sqlite_code`はSQLiteが返した
数値コードです。schema版が古いだけの場合は`schema.ok`が`false`になりますが、
`database.failures`は出力されません。

主要なjournal event名は次のとおりです。

- service起動: `service.listening`
- OIDC discovery成功・失敗: `oidc.discovery.completed`、`oidc.discovery.failed`
- Auth0 discovery成功・失敗: `mcp.authorization.discovery.completed`、
  `mcp.authorization.discovery.failed`
- JWKS更新成功・失敗: `mcp.authorization.jwks_refresh.completed`、
  `mcp.authorization.jwks_refresh.failed`
- MCP token拒否・検証基盤障害: `mcp.authentication.failed`、
  `mcp.authentication.unavailable`
- MCP scope不足: `mcp.authorization.failed`
- purge成功・失敗: `maintenance.purge.completed`、`maintenance.purge.failed`
- backup成功・失敗: `maintenance.backup.completed`、`maintenance.backup.failed`
- archive検証成功・失敗: `maintenance.archive_validation.completed`、
  `maintenance.archive_validation.failed`
- archive移行成功・失敗: `maintenance.archive_migration.completed`、
  `maintenance.archive_migration.failed`
- 復元検証成功・失敗: `maintenance.restore_verification.completed`、
  `maintenance.restore_verification.failed`
- backup検証成功・失敗: `maintenance.backup_verification.completed`、
  `maintenance.backup_verification.failed`
- backup世代整理成功・失敗: `maintenance.backup_prune.completed`、
  `maintenance.backup_prune.failed`
- SQLite診断失敗: `maintenance.diagnostics.failed`
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
ノートのプレビューに成功したHTTP logでは、`note_diagnostic_count`が保存を妨げない診断の件数を
示します。このfieldと診断・保守eventには、ノート本文、ノートID、利用者identity、Cookie、
token、認可code、client secretを記録しません。
保守unitの失敗はHTTP serviceの停止を意味しません。unitの`Result`と同じinvocationのjournalを確認し、
保存先容量、権限、SQLite診断結果を修正してからunitを再実行します。
