# REST API

## 現行契約

公開 API は `/api/v2` です。機械可読な正本は [OpenAPI 3.1](openapi-v3.json) であり、実行中の
`GET /api/v2/openapi.json` は同じ内容を返します。`/api/v1` と root 管理 API は v3 では提供しません。

ブラウザー API は OIDC session Cookie を用います。変更操作には session と同時に発行される CSRF
token を `X-CSRF-Token` で送ります。`Origin` が公開 base URL と一致しない要求は拒否されます。

| 操作 | endpoint | 備考 |
| --- | --- | --- |
| liveness | `GET /api/v2/health` | 認証不要 |
| session | `GET /api/v2/session` | Kanidm subject と管理者フラグ |
| ノート一覧・作成 | `GET` / `POST /api/v2/notes` | 作成は CSRF 必須 |
| 取得・更新・削除 | `GET` / `PUT` / `DELETE /api/v2/notes/{note_id}` | 更新・削除は `expected_revision` 必須 |
| AsciiDoc export | `GET /api/v2/notes/{note_id}/source` | 可視ノートだけ |
| 復元 | `POST /api/v2/notes/{note_id}/restore` | 30 日以内の削除済みノート |
| MCP 認可取消 | `DELETE /api/v2/mcp-authorizations/{client_id}` | CSRF 必須。token family を失効 |

ノートの作成・更新は JSON の `title`、`body`、`tags` を受け取ります。成功応答の詳細な形式、エラー、
status code は OpenAPI を参照してください。アクセス可否は直接 ACL と Kanidm group 認可の両方で決まります。

## Web UI

`/` はログイン後の閲覧 UI、`/notes/{note_id}` は個別ノート表示です。HTML 表示と一覧には、当該利用者が
閲覧可能なノートだけを出します。
