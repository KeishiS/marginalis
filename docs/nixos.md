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
変更してください。現行のSQLite schema versionは12です。旧versionを起動時に自動移行しません。
schema 10または9から更新する場合は、AdocWeave 0.11.0を使用する旧実行環境でarchive 7を作成し、
現行の`migrate-archive`でarchive 13へ変換してから、空のschema 12へ取り込んでください。
この経路では全ノートをAdocWeave 0.23.0の規則で再検証し、題名、タグ、参照索引を再構築します。
ノート、所有者、削除状態、revision、ノート間参照、共有権限が一致することをCIで検証しています。

切戻す場合はserviceを停止し、更新後に作成したdatabaseを保全してから、更新前に退避した`dataDir`と
実行環境を組み合わせて戻します。異なる版のserviceを同時に同じdatabaseへ接続してはいけません。
archive 7、8、9、10、11以外の旧archiveには移行操作を提供しません。

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
混同しないでください。この違いを意図的に維持する理由は
[認証基盤の停止時にWebとMCPで起動可否を分ける](adr/0005-認証基盤の停止時にwebとmcpで起動可否を分ける.md)に
記録しています。

Kanidmのdiscoveryに失敗した場合、serviceは動作したままログインだけができません。起動失敗より
気付きにくいため、`oidc.discovery.failed`を監視対象へ含めてください。

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
現行archiveは`marginalis-archive-13`で、AdocWeave package版`0.23.0`とnote profile版`4`を記録します。
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

## 以前のarchiveからの移行

v0.18.0が作成したarchive 11、v0.16.1が作成したarchive 10、v0.16.0が作成したarchive 9、
v0.15.0が作成したarchive 8は、復元前に現行のarchive 13へ変換します。SQLite schema 12には書誌ライブラリーを追加したため、
稼働中の旧databaseファイルはそのまま使用できません。archiveを書き出してから変換し、空の
schema 12へ復元してください。

```sh
sudo -u marginalis marginalis migrate-archive \
  --input /srv/marginalis-migration/archive-9.json \
  --output /srv/marginalis-migration/archive-13.json
sudo -u marginalis marginalis verify-restore \
  --input /srv/marginalis-migration/archive-13.json
```

archive 8を入力にする場合も、出力はarchive 13です。変換では全ノートをAdocWeave 0.23.0で
再検証します。入力archiveは変更されず、出力先が既に存在する場合は上書きしません。詳しい判断は
[0.23移行判断](adocweave-v0.23-migration.md)を参照してください。

## v0.10.0からの移行

v0.10.0のschema 10には、AdocWeave 0.11.0で導出した題名とタグが保存されています。現行版は
AdocWeave 0.23.0でmetadataを再構築するため、databaseファイルを直接引き継ぎません。

1. v0.10.0のserviceとdatabaseを使ってarchive 7を書き出し、同じ実行環境の
   `validate-archive`と`verify-restore`を実行します。
2. v0.10.0のNixOS generation、`dataDir`、archive 7を削除せずに保全します。
3. 現行版の実行ファイルでarchive 7をarchive 13へ変換します。入力と出力には異なる絶対pathを
   指定してください。入力は変更されず、既存の出力を上書きしません。

   ```sh
   sudo -u marginalis <現行版のmarginalis> migrate-archive \
     --input /srv/marginalis-migration/archive-7.json \
     --output /srv/marginalis-migration/archive-13.json
   sudo -u marginalis <現行版のmarginalis> verify-restore \
     --input /srv/marginalis-migration/archive-13.json
   ```

4. 移行が失敗した場合はarchive 13を取り込まず、v0.10.0へ戻ります。改行を残す複数行タグや
   header後の属性操作など、[0.17移行判断](adocweave-v0.17-migration.md)に記載した差を
   v0.10.0で修正し、archive 7の書き出しからやり直します。エラーに示された`position`は、
   archiveの`notes`または`note_acl`配列内の1から始まる位置です。診断へ本文や識別子は
   出力されません。
5. 成功したarchive 13を、次節の手順で空のschema 12へ取り込みます。

CIでは、旧実行環境が作成したschema 9のarchive 7にも同じ移行操作を適用し、入力archiveの不変、
旧archiveの直接取込拒否、schema 12でのノート、ACL、参照、削除状態、revisionの一致、archive 13の
再書き出し一致を検査します。schema 10も同じarchive 7契約を使用するため、移行入口は一つです。

## 他の道具で読める形での取り出し

保存しているノートと書誌情報を、Marginalis以外の道具で読める形へ一度に書き出せます。ノートは
保存しているAsciiDocのまま`.adoc`ファイルへ、書誌情報はCSL-JSONの配列へ書き出します。CSL-JSONは
pandocやZoteroなど多くの文献管理ツールが読み取る形式です。

```sh
sudo -u marginalis marginalis export-documents \
  --output /srv/marginalis-export/2026-07-31.tar.xz
```

出力は`tar.xz`形式の書庫1つです。展開すると、出力ファイル名から作った最上位ディレクトリーの下に
所有者ごとの内容が並びます。引用は作成者のライブラリーで解決するため、この分け方でノートと
文献の対応が保たれます。

```text
2026-07-31/
  id.example.test/alice/notes/先行研究の整理-019f0000-….adoc
  id.example.test/alice/bibliography.json
  manifest.json
```

```sh
tar -xJf /srv/marginalis-export/2026-07-31.tar.xz
```

ファイル名は題名とnote IDを並べた形です。題名が重複しても、ファイル名に使えない文字を含んでも、
note IDを付けるため衝突しません。削除済み（ソフトデリート）のノートは書き出しません。
`cite:`は解決せず本文のまま書き出すため、受け取った側の道具が同じ出力のCSL-JSONで解決できます。

`manifest.json`は、各ファイルとnote IDの対応、所有者、日時、revision、タグ、ACLに加えて、
形式名`marginalis-documents-1`、Marginalisの版、解析に使ったAdocWeave packageの版、ノートの
受理規則の版を持ちます。版の意味はarchiveと同じです。取り込む側は、稼働しているMarginalisの版と
比べて再検証や移行が必要かどうかを判断できます。

出力先のファイルが既に存在する場合は失敗します。書庫ファイルは`600`で作成し、書庫の中の
ディレクトリーは`700`、ファイルは`600`として記録します。展開する側の設定によらず、所有者だけが
読める状態になります。

### 書き出した内容を取り込む

書き出した書庫は`import-documents`で取り込めます。別の道具で`.adoc`ファイルを編集してから戻す
場合と、別の環境へ移す場合に使います。

```sh
sudo -u marginalis env \
  MARGINALIS_DATABASE_URL=sqlite:/srv/marginalis-restore/marginalis.sqlite \
  marginalis import-documents --input /srv/marginalis-export/2026-07-31.tar.xz
```

本文の正は`.adoc`ファイル、識別子、所有者、日時、revision、ACLの正は`manifest.json`です。
manifestが挙げるファイルが無い場合は失敗します。manifestに無いファイルは無視するため、
ファイルを置くだけではノートを増やせません。

manifestが記録するAdocWeave packageの版とノート受理規則の版が稼働中の値と違う場合は、全ノートを
現行の規則で再検証してから取り込みます。一件でも現行規則を満たさない場合はdatabaseを変更せず、
manifest内の位置で失敗を示します。診断へ本文や識別子は出力されません。

取り込み先は空のdatabaseに限ります。既存のノートまたは認証状態があるdatabaseへの取り込みは
失敗し、暗黙に上書きされません。書庫の展開では、`..`や絶対path、symlink、通常ファイルと
ディレクトリー以外の項目を拒否し、展開後の大きさにも上限を設けています。

**この出力はバックアップの代わりではありません。** 削除済みノートを含まないため、
日次の退避と復元には次節の`export-archive`と`import-archive`を使用してください。archiveだけが
Web sessionを除く全状態を検証付きで往復できます。

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

設定の報告は`configuration.variables`に環境変数名を鍵として並びます。各項目の`set`は設定の有無、
`required`は現在の構成で必須かどうかを示します。`value`は秘密でも保存先でもない変数にだけ付き、
`element_count`はカンマ区切りの変数の要素数です。`configuration.mcp_enabled`はMCPを有効と判断した
結果で、`MARGINALIS_MCP_AUTHORIZATION_ISSUER`が設定されているかどうかと一致します。
serviceの起動処理と`diagnose`は同じ宣言から値を読むため、両者の判断が食い違いません。
値の前後の空白は取り除いて扱い、空白だけの値は未設定とみなします。
`status`が`failed`の場合は`database.error`と各検査の`actual`、`expected`を確認します。
SQLを実行できなかった場合は、`database.failures`の`check`で失敗した検査、`category`で
ロック、読み取り専用、入出力エラーなどの分類を確認できます。`sqlite_code`はSQLiteが返した
数値コードです。schema版が古いだけの場合は`schema.ok`が`false`になりますが、
`database.failures`は出力されません。

安定したjournal event名、共通field、記録禁止情報、障害時の絞り込み方は
[ログと障害診断](observability.md)を参照してください。監視や通知では、人向けのログ本文ではなく
`event`を使用します。

```bash
journalctl -u marginalis.service --since today
journalctl -u marginalis.service _SYSTEMD_INVOCATION_ID="$(systemctl show marginalis.service -p InvocationID --value)"
journalctl -u marginalis-purge-expired.service -g 'maintenance.purge.'
journalctl -u marginalis-backup.service -g 'maintenance.backup.'
find /srv/marginalis-backups -mindepth 1 -maxdepth 1 -type d \
  -exec test -f '{}/COMPLETE' ';' -print | sort | tail
```

HTTP logは`request_id`で一連の処理を追跡できます。OIDC到達不能時もloginだけを503で閉じたまま
livenessを維持します。ノートのプレビューに成功したHTTP logでは、`note_diagnostic_count`が
保存を妨げない診断の件数を示します。
保守unitの失敗はHTTP serviceの停止を意味しません。unitの`Result`と同じinvocationのjournalを確認し、
保存先容量、権限、SQLite診断結果を修正してからunitを再実行します。
