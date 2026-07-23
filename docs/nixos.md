# NixOSによるMarginalisの運用

公開時にはGitHubのflake inputからNixOS moduleを取り込む。

```nix
{
  inputs.marginalis.url = "github:KeishiS/Marginalis";
  outputs = { self, nixpkgs, marginalis, ... }: {
    nixosConfigurations.example = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        marginalis.nixosModules.default
        {
          services.marginalis = {
            enable = true;
            # reverse proxyを使わず直接待受けを公開する場合だけ有効にする。
            openFirewall = false;
            # journalctlで観測するtracing filter。
            logFilter = "info,marginalis_auth_oidc=info";
            baseUrl = "https://marginalis.sandi05.com";
            listenAddress = "127.0.0.1:3000";
            # 新規SQLite DBにだけ適用する。既存DBのroot設定は上書きしない。
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

`clientSecretFile`と`initialRootPasswordFile`は実行時の絶対パスである。Nix storeへsecret値を
書き込んではならない。sops-nixまたはagenixを使う場合は、復号後のruntime fileのpathを指定する。
moduleはsystemd `LoadCredential`でsecretを渡し、unitの環境変数にはsecret自体を置かない。

初回起動時だけ`initialRootPasswordFile`が必要である。root初期化後はこのoptionを削除できる。
SQLite DBと将来のAsciiDoc正本は`dataDir`（既定値は`/var/lib/marginalis`）に永続化される。
`initialRegistrationPolicy`は新規DBにだけ`open`または`approval`を設定する。既存DBではroot APIから
変更した現在の登録policyを保持するため、NixOS再適用で上書きされない。

`openFirewall`は`listenAddress`のTCP portをNixOS firewallで許可するだけである。既定の
`127.0.0.1:3000`は外部から到達不能なままであり、公開には同一Base URLを終端するreverse proxyが必要である。
reverse proxyは`/auth/`、`/api/`、`/mcp`、`/.well-known/`、`/oauth/`を同じoriginへ転送する。TLSはproxyで
終端してよいが、`baseUrl`には外部から見えるHTTPS URLを設定する。

root loginの接続元rate limitは、Marginalis内ではTCP peer addressだけを使う補助制限である。proxy配下で
利用者ごとの粗い制限を行う場合は、proxy側で実施する。Marginalisは`X-Forwarded-For`、`Forwarded`などの
client IP headerを信頼しない。Cookieを伴うREST変更操作は、公開originと`Origin`・`Sec-Fetch-Site`を照合するため、
proxyはこれらのbrowser headerを削除・書換えしてはならない。

`logFilter`は`RUST_LOG`としてserviceと投影再構築unitへ渡される。障害調査では一時的に
`"info,marginalis_auth_oidc=debug"`のように狭いmoduleだけをdebugへ上げる。request body、password、
OIDC code、tokenおよびsecretを記録する設定は提供しない。

## 適用後の確認

`nixos-rebuild switch`後は、次を順に確認する。

1. `systemctl status marginalis.service`で`active (running)`であることを確認する。
2. `curl -fsS https://marginalis.sandi05.com/api/v1/health`でhealth responseを確認する。
3. `curl -fsS https://marginalis.sandi05.com/api/v1/readiness`が`ready`を返すことを確認する。`503`なら
   OIDC Discovery障害でroot-only縮退起動中である。
4. `https://marginalis.sandi05.com/auth/oidc/login`からKanidmへ移動し、ログイン後にBase URLへ戻ることを確認する。
5. 初回はroot login後、`GET /api/v1/admin/users/pending`で保留ユーザーを確認し、有効化する。
6. MCPを有効にした場合は`/.well-known/oauth-protected-resource/mcp`がJSONを返すことを確認する。

個人運用での段階的な初回公開は、reverse proxyを接続したまま`/api/v1/health`と`/api/v1/readiness`を
確認し、root login、OIDC login、RESTでのノート作成・検索・削除、最後にMCP client認可と`search_notes`を
順番に行う。各段階で`journalctl -u marginalis.service -b --no-pager`と`X-Request-Id`を記録すると、
問題を安全に切り分けられる。

失敗の詳細は`journalctl -u marginalis.service -b --no-pager`で確認する。ログにはOIDC code、token、
client secret、root passwordを出力しない。

OIDC Discoveryが一時的に失敗した場合も、serviceはroot緊急ログインだけを有効にして起動する。通常の
OIDC loginは利用不可となるため、IdP復旧後に`sudo systemctl restart marginalis.service`を実行して
Discoveryをやり直す。

root監査は`dataDir`内のSQLite DBに365日保持する。専用のHTTP APIは公開しないため、必要時にはサーバ上で
読み取り専用に確認する。

```sh
sudo -u marginalis sqlite3 /var/lib/marginalis/marginalis.sqlite \
  'SELECT action, actor_user_id, target_user_id, target, occurred_at_ms
   FROM root_audit_log ORDER BY audit_id DESC LIMIT 100;'
```

この表にはpassword、cookie、session ID、OIDC code、tokenまたはtoken hashを保存しない。

## 正本からの投影再構築

バックアップからAsciiDoc正本を復元した場合や、保守作業で`dataDir/notes/`を直接修正した場合は、次で
SQLiteの検索・anchor・xref投影を正本から再構築する。

```sh
sudo systemctl start marginalis-rebuild-projections.service
sudo systemctl start marginalis.service
```

再構築unitはHTTP serverと競合するため、起動時に`marginalis.service`を停止する。全`.adoc`正本を
先にUTF-8・ノートprofile・ファイル名と`note-id`の一致まで検証し、その後に一つのSQLite transactionで
投影を置換する。検証エラー時は最後に成功したSQLite投影を変更しない。既存ノートのACLは保持し、正本が
なくなったノートの投影とACLだけを削除する。

## バックアップと復元

`dataDir`はAsciiDoc正本とSQLiteを一組で扱う。`backupDirectory`には、同じfilesystemまたは別volume上の
絶対pathを指定する。moduleはこのdirectoryを`marginalis`所有で用意し、その直下に時刻付きの新しい
backup generationを作る。backup先の
永続化、世代管理、off-site複製および保持期間は運用者が決める。自動timerは意図的に提供しない。

```nix
services.marginalis.backupDirectory = "/var/lib/marginalis-backups";
```

指定後、次のoneshot unitでHTTP serviceを停止した状態のbackupを作成する。

```sh
sudo systemctl start marginalis-backup.service
sudo systemctl start marginalis.service
```

成功した各generationには`FORMAT`、`marginalis.sqlite`、`notes/<UUID>.adoc`、`MANIFEST`および`COMPLETE`
markerが含まれる。`MANIFEST`はformat v1、作成時刻、SQLiteと全正本のSHA-256を記録する。同じ出力pathが
既に存在する場合は上書きせず失敗する。失敗した場合も不完全な出力を残すため、これらが揃わないdirectoryを
復元に使用してはならない。個別の出力pathを指定する手動実行には
`marginalis backup --output /absolute/path`を使う。

復元時は、まず`marginalis restore --input <完全なbackup> --output <存在しない絶対path>`で、format marker、
manifest hash、SQLiteの`integrity_check`、全正本のUTF-8・ノートprofile・ファイル名とのnote ID一致を確認する。この
commandは既存`dataDir`を変更せず、検証済みSQLiteと正本を新しい出力directoryへ複製して`RESTORED`
markerを作る。

実際にどの`dataDir`へ切り替えるかは、旧dataDirを保持してrollback可能にする運用判断である。出力を採用する
場合だけ、`services.marginalis.dataDir`（必要なら`databaseUrl`）をその出力へ変更し、
`marginalis-rebuild-projections.service`、続けて`marginalis.service`を起動する。既存dataDirの削除や
in-place上書きは、この確認後に対象pathを明示して別途実施する。

## MCPの公開

`services.marginalis.mcp.enable`の既定値は`false`である。`true`の場合に限り、同じBase URL配下へ
`/mcp`、OAuth Authorization Server、Protected Resource Metadataを公開する。reverse proxyを用いる
場合は、これらのpathを含めて同じoriginへ転送する。

`clientMetadataAllowedHosts`は、未知のOAuth clientの`client_id` URLからmetadataを取得してよい
HTTPS hostの許可リストである。この制約により、認可endpointが任意の内部URLを取得するSSRFの入口に
なることを防ぐ。空リストでも既にSQLiteへ登録済みのclientは利用できるが、初期運用ではmetadata hostを
明示して登録する方式を推奨する。MCP client側の設定は[MCP仕様](mcp.md)を参照する。

## API-first再基線化時の初期化

現在の破壊的な再基線化では、旧dataDirを移行しない。新しいschema、OIDC identity、ACL、session、
root credentialおよびAsciiDoc正本は空の状態から作成する。アプリケーションとNixOS moduleは通常の
起動・`nixos-rebuild`で既存dataDirを削除しない。

デプロイ先の既存データを破棄して切り替える場合は、次の順序を用いる。

1. `sudo systemctl stop marginalis.service`でサービスを停止する。
2. 必要なら`dataDir`をバックアップする。復帰の可能性が不要なら、対象の`dataDir`だけを明示して
   削除する。
3. module設定を新しいMarginalis revisionへ更新し、`nixos-rebuild switch`を実行する。
   systemdの`StateDirectory`により空のdata directoryが専用ユーザー所有で作成される。
4. `initialRootPasswordFile`を一度だけ指定してroot credentialを初期化し、OIDC login、
   `GET /api/v1/health`および`GET /api/v1/session`を確認する。
5. root初期化後は`initialRootPasswordFile`をNixOS設定から除去する。

再基線化の完了時には、旧SQLiteを新しいserverへ指定した場合、互換migrationを試みずversion不一致
として起動を停止する。これは誤って旧ACLやsessionを新しい認可モデルへ持ち込まないためである。
