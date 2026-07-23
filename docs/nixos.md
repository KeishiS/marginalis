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
