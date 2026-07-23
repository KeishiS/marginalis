# 010: NixOS moduleと公開パッケージ

状態: 初期公開に必要な実装は完了。GitHub公開後の実環境導入とrelease automationは後続である。

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

## 実装方針

このIssueは公開直前まで延期しない。サーバーの設定境界、永続パスおよびsystemd実行条件を
早期に固定するため、ノートCRUD・Web UI・MCPより先にmodule骨格を実装する。OIDCの実環境
結合試験およびsecret注入は、対応するアプリケーション境界の完成後に追加する。

## 実装計画

1. **早期基盤**: Rustバイナリをpackage化し、flake outputにLinux向けpackageと
   `nixosModules.default`を追加する。
2. **早期基盤**: `services.marginalis` moduleを実装する。
   - `enable`、`package`、`listenAddress`、`baseUrl`、`dataDir`、`databaseUrl`を型付きoptionにする。
   - OIDC issuerとClient IDを設定可能にする。
   - Base URLからcallback URIを文書化し、subpathを壊さない。
3. **早期基盤**: systemd unitを実装する。
   - 専用user、state/data directory、再起動方針、依存関係を設定する。
   - `DynamicUser`の可否をSQLiteとデータ永続化の要件から評価する。
   - `ProtectSystem`、`PrivateTmp`、`NoNewPrivileges`等を適用し、必要な書込み範囲だけを許可する。
4. **設定境界完成後**: secret注入を実装する。
   - OIDC Client Secretと初回ROOT_PASSWORDをNix storeへ書き込まない。
   - systemd credentialまたはsops-nix/agenix等の外部secret管理から環境変数へ渡す経路を提供する。
   - 初期化後にROOT_PASSWORDが不要となることを確認する。
5. **早期基盤**: reverse proxyとの連携例と、運用上の永続パスをREADMEへ追加する。
6. **段階的検証**: module評価とHTTP health endpointのNixOS VM testを追加する。ノート永続化、
   secret非露出および実OIDC callbackのVM testは、それぞれの機能完成時に追加する。

## 完了条件

- GitHub flake inputをimportし、`services.marginalis.enable = true`で評価・ビルド・起動できる。
- SQLiteとdatadirがNix store外の永続パスに作られる。
- OIDC Client SecretとROOT_PASSWORDがNix store、unitの平文Environmentおよびjournalへ現れない。
- reverse proxy配下のBase URLとOIDC callback URIが一致する。
- package、module評価およびNixOS VM testをCIで実行する。

## 2026-07-23時点の実装状況

- flakeは`packages.<system>.default`と`nixosModules.default`を公開する。NixOS moduleは専用の
  `marginalis` system user、永続`dataDir`、systemd credential、sandbox、任意の`openFirewall`を設定する。
- `initialRegistrationPolicy`は新規SQLite DBにだけ適用される。運用中の登録policyはroot REST APIで
  管理し、`nixos-rebuild`で上書きしない。
- `marginalis-rebuild-projections.service`は正本からSQLite投影を再構築するoneshot unitである。HTTP serviceと
  排他的に実行し、同じcredentialとsandboxを用いる。
- `backupDirectory`を明示した場合だけ`marginalis-backup.service`を提供する。これはHTTP serviceと
  排他的にSQLiteと検証済みAsciiDoc正本を一組で出力し、既存backupを上書きしない。保存先、保持期間、
  off-site複製および自動timerは運用policyとして後続に残す。
- flake checkはmodule評価を行い、NixOS VMでserviceのcredential注入・永続directory・再起動・
  投影再構築unitの排他動作を実行する。
