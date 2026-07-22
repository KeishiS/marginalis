# 010: NixOS moduleと公開パッケージ

## 目的

Marginalisの公開時に、GitHubのNix flake inputからNixOS moduleを読み込み、
`services.marginalis`として安全に設定・起動できるようにする。

## 目標となる利用形態

```nix
{
  inputs.marginalis.url = "github:<owner>/Marginalis";
  imports = [ inputs.marginalis.nixosModules.default ];

  services.marginalis = {
    enable = true;
    # settingsとsecret参照を設定する。
  };
}
```

## 実装計画

1. flake outputにLinux向けpackage、`nixosModules.default`、NixOS testを追加する。
2. Rustバイナリをpackage化し、`marginalis-server`等の安定した実行名を定める。
3. `services.marginalis` moduleを実装する。
   - `enable`、`package`、`listenAddress`、`baseUrl`、`dataDir`、`databaseUrl`を型付きoptionにする。
   - OIDC issuerとClient IDを設定可能にする。
   - Base URLからcallback URIを文書化し、subpathを壊さない。
4. systemd unitを実装する。
   - 専用user、state/data directory、再起動方針、依存関係を設定する。
   - `DynamicUser`の可否をSQLiteとデータ永続化の要件から評価する。
   - `ProtectSystem`、`PrivateTmp`、`NoNewPrivileges`等を適用し、必要な書込み範囲だけを許可する。
5. secret注入を実装する。
   - OIDC Client Secretと初回ROOT_PASSWORDをNix storeへ書き込まない。
   - systemd credentialまたはsops-nix/agenix等の外部secret管理から環境変数へ渡す経路を提供する。
   - 初期化後にROOT_PASSWORDが不要となることを確認する。
6. reverse proxyとの連携例、バックアップ対象、アップグレード手順をREADMEへ追加する。
7. NixOS VM testで起動、永続化、secret非露出およびHTTP health endpointを検証する。

## 完了条件

- GitHub flake inputをimportし、`services.marginalis.enable = true`で評価・ビルド・起動できる。
- SQLiteとdatadirがNix store外の永続パスに作られる。
- OIDC Client SecretとROOT_PASSWORDがNix store、unitの平文Environmentおよびjournalへ現れない。
- reverse proxy配下のBase URLとOIDC callback URIが一致する。
- package、module評価およびNixOS VM testをCIで実行する。
