# 018: API契約とOpenAPIの互換性方針

状態: 完了。

## 目的

REST APIの仕様を、機械可読な唯一の契約として定義する。Web UI、Tauri、スクリプト、
MCPクライアントが同じ契約を参照できる状態にする。

## 対象

- OpenAPI文書と、REST APIの要求・応答・エラー型
- UUIDv7、RFC 3339、ETag、カーソル、CSRF、認証失敗、非公開情報の存在を隠す`404`応答のスキーマ
- APIのバージョン方針と破壊的変更の扱い
- OpenAPI、ルーター、JSON直列化、HTTP状態コード、HTTPヘッダーの契約試験
- REST APIとMCPツールのスキーマ対応

HTTP固有のCookieとCSRFは、MCPの契約に含めない。

## 採用した方針

- 現在の`/api/v1`を維持する。
- 個人開発中の再構成では、`/api/v1`内の破壊的変更を許容する。
- `v0.1.0-rc.2`のOpenAPI文書を、v0.1.0に向けた固定候補とする。
- RC期間中の破壊的変更は、リリースを妨げる問題の修正に限定する。
- 正式版の受入確認が完了した時点で、`/api/v1`の契約を固定する。
- エラー応答は、安定した`code`と安全な`message`を持つ共通形式へ統一する。
- UI固有の状態、SQLiteの行、秘密情報、トークンハッシュは契約型から除外する。
- OpenAPI文書は、認証不要の`/api/v1/openapi.json`で公開する。
- 同じOpenAPI文書をリリース成果物にも含める。

## 実施内容

- `docs/openapi.json`をOpenAPI 3.1の契約として定義した。
- 同じ文書を、リリース成果物の`share/marginalis/openapi.json`と
  `/api/v1/openapi.json`へ配置した。
- REST APIのJSON境界を`marginalis_web::contract`へ集約した。
- アダプター内部の型、認証情報、トークンハッシュを公開契約から除外した。
- UUIDv7、カーソル、ETag、CSRF、エラー応答、MCP認可のRFC 3339時刻を
  OpenAPIスキーマへ明記した。
- 品質検査で、OpenAPIのバージョン、必須パス、`Problem`スキーマを検証するようにした。
- ルーター試験で、公開エンドポイント、HTTP状態コード、HTTPヘッダーを照合するようにした。
- 互換性方針、廃止手順、MCPツールとの対応をREST API文書、受入手順、MCP仕様へ記録した。

## 完了条件

- OpenAPI文書をビルド成果物として生成し、内容を検証できる。
- REST APIのハンドラーが契約型だけを入出力に使用する。
- 互換性方針と廃止手順がREADMEとリリース手順に記録されている。

上記の条件を満たしたため、このIssueを完了とする。
