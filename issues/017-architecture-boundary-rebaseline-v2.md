# 017: 依存境界を強制するアーキテクチャ再設計 v2

状態: 完了。

## 目的

HTTP/MCP transportがapplicationのportだけに依存し、SQLite、filesystem、AsciiDoc、OIDCおよびHTTP clientの
具体adapterを本番依存として参照しない構成へ破壊的に移行する。

## 移行前の問題

- `marginalis-web`はtest helperのために`marginalis-sqlite`、`marginalis-files`、`marginalis-server`等へ
  本番依存している。設計文書の「transportは具体adapterを参照しない」という不変条件をCargoが保証しない。
- `marginalis-server`が設定読込、adapter組立、ノート、認証、MCP OAuthの実装を同時に持つ。
- backup/rebuildがOIDC secretを含む`ServerConfig`に依存し、storage保守だけを単独実行できない。

## 目標構成

```text
domain ── application ── transport-web / transport-mcp
              │
              ├── adapters-sqlite / adapters-files / adapters-asciidoc / adapters-oidc
              │
              └── runtime（設定・組立・起動）

maintenance ── storage config + adapters-sqlite/files/asciidoc
integration-tests ── runtime + concrete adapters
```

crate名は実装時に決めるが、transportから具体adapterへの依存を禁止することを優先する。

## 実施内容

1. application facadeを`notes`、`identity/session`、`administration`、`mcp oauth`の能力別interfaceへ分割した。
2. `marginalis-web`と`marginalis-mcp`をapplication/domain/contractだけへ依存させた。
3. concrete adapterを使うHTTP/MCP試験を`marginalis-integration-tests`へ移した。
4. `StorageConfig`、`HttpConfig`、`OidcConfig`およびsecretを分離した。
5. backup/rebuild/restore/audit-pruneを、OIDC設定なしで動くmaintenance組立へ移した。
6. CIでtransportの禁止依存を検査する`cargo make dependency-boundaries`を追加した。

## 完了条件

- web/MCP production dependencyにsqlx、filesystem、AdocWeave、openidconnect、reqwestが含まれない。
- maintenance操作はOIDC issuer/client secretがなくてもstorageだけを検証・操作できる。
- runtime以外は環境変数を直接読まない。
- 結合試験の具体adapter依存がproduction crateの`[dev-dependencies]`へ閉じるか、専用crateへ隔離される。

## 実施結果

- `marginalis-web`のproduction dependencyはapplication/domain/MCP contractとHTTP transportだけに限定した。
  SQLite、filesystem、AsciiDoc、OIDCおよびserver adapterはtest-only dependencyへ移した。
- OIDCの具体型は`marginalis-service`だけが直接組み立て、Web transportは`OidcAuthenticationUseCases`、
  `WebSessionUseCases`、`UserAdministrationUseCases`の能力別interfaceだけを受け取る。
- `StorageConfig`、`HttpConfig`、`OidcConfig`と`SecretConfig`を分離し、backup/rebuild/audit-pruneはOIDC issuer・
  client secret・HTTP設定を読まない。
- `cargo make dependency-boundaries`をquality gateへ加え、web/MCPの通常dependency graphに禁止したconcrete
  adapterが混入すると失敗するようにした。
