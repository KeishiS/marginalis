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
            baseUrl = "https://marginalis.sandi05.com";
            listenAddress = "127.0.0.1:3000";
            oidc = {
              issuerUrl = "https://id.sandi05.com";
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
