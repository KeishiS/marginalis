# 018: API契約・OpenAPI・互換性方針

状態: 完了。

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

## 判断結果

- `v0.1.0-rc.1`のOpenAPI documentをv0.1.0のfreeze候補とする。RC期間中の破壊的変更はrelease blocker
  修正だけに限定し、正式版の受入完了時に`/api/v1`をfreezeする。

## 実施結果

- `docs/openapi.json`をOpenAPI 3.1 contractとして、release artifactの`share/marginalis/openapi.json`と
  `/api/v1/openapi.json`へ同一内容で配置した。
- REST JSON boundaryは`marginalis_web::contract`へ集約し、adapter内部の型・credential・token hashを公開しない。
- UUIDv7、cursor、ETag、CSRF、problem responseおよびMCP認可のRFC 3339時刻をOpenAPI schemaへ明示した。
- quality gateはOpenAPI version・必須path・Problem schemaを検証し、router testは公開endpoint・status・headerを照合する。
- compatibility/deprecation policyとMCP toolとの対応表をREST API、受入手順、MCP仕様へ記録した。
