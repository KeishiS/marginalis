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
            baseUrl = "https://marginalis.sandi05.com";
            listenAddress = "127.0.0.1:3000";
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

`openFirewall`は`listenAddress`のTCP portをNixOS firewallで許可するだけである。既定の
`127.0.0.1:3000`は外部から到達不能なままであり、公開には同一Base URLを終端するreverse proxyが必要である。
reverse proxyは`/auth/`、`/api/`、`/mcp`、`/.well-known/`、`/oauth/`を同じoriginへ転送する。TLSはproxyで
終端してよいが、`baseUrl`には外部から見えるHTTPS URLを設定する。

## 適用後の確認

`nixos-rebuild switch`後は、次を順に確認する。

1. `systemctl status marginalis.service`で`active (running)`であることを確認する。
2. `curl -fsS https://marginalis.sandi05.com/api/v1/health`でhealth responseを確認する。
3. `https://marginalis.sandi05.com/auth/oidc/login`からKanidmへ移動し、ログイン後にBase URLへ戻ることを確認する。
4. 初回はroot login後、`GET /api/v1/admin/users/pending`で保留ユーザーを確認し、有効化する。
5. MCPを有効にした場合は`/.well-known/oauth-protected-resource/mcp`がJSONを返すことを確認する。

失敗の詳細は`journalctl -u marginalis.service -b --no-pager`で確認する。ログにはOIDC code、token、
client secret、root passwordを出力しない。

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
