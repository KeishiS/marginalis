# 018: API契約・OpenAPI・互換性方針

状態: 提案。

## 目的

REST APIをhandler内部の型ではなく、機械可読な唯一の契約として定義する。Web UI、Tauri、script、MCPとの
相互運用が増える前に、request/response/error/pagination/concurrencyの意味をfreeze可能にする。

## 範囲

- OpenAPI documentと、REST request/response/error型の単一source of truth。
- UUIDv7、RFC 3339、ETag、cursor、CSRF、認証失敗、ACL非漏洩404のschema化。
- API version policyとbreaking change policy。
- contract test: OpenAPI、router、JSON serialization、status/headerの一致検証。
- MCP tool schemaとの対応表。HTTP固有のCookie/CSRFをMCP contractへ漏らさない。

## 方針案

- 現在の`/api/v1`は維持する。個人開発の再構成期間は、このpath内の破壊的変更を許容する。
  OpenAPI導入後に、外部client向けの互換性方針とfreeze時点を改めて決める。
- errorは安定した`code`と安全な`message`を持つproblem responseへ統一する。
- UI専用状態、SQLite row、secret、token hashはcontract型から除外する。
- OpenAPIは認証不要の`/api/v1/openapi.json`で公開し、同一内容をrelease artifactにも含める。

## 完了条件

- OpenAPIがbuild artifactとして生成・検証される。
- REST handlerはcontract型だけを入出力に用いる。
- compatibility policyとdeprecation手順がREADMEおよびrelease手順に記録される。

## 要判断事項

- OpenAPI導入後に`/api/v1`をfreezeする時期。現時点では未決とする。
