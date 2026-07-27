# REST API

## 現行仕様

公開 API は `/api/v2` です。機械可読な正本は [OpenAPI 3.1](openapi.json) であり、実行中の
`GET /api/v2/openapi.json` は同じ内容を返します。`/api/v1` とroot管理APIは提供しません。

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
status code は OpenAPI を参照してください。本文はUTF-8で512 KiB以下です。通常利用者は作成者の
`(issuer, subject)`が自身と一致するノートだけを操作でき、`server-admins`はすべてのノートを操作できます。
権限のないノートは、存在の推測を防ぐため`404 not_found`として扱います。

入力規則に違反した場合は`422`と`validation_failed`を返します。`diagnostics`の各要素は安定した
`code`、対象field、任意の`span`、説明を持ちます。`span`は送信した`body`を基準とするUTF-8 byteの
半開区間です。タイトルとタグの診断には本文の疑似位置を付けません。

```json
{
  "code": "validation_failed",
  "message": "note input is invalid",
  "diagnostics": [
    {
      "code": "invalid_title",
      "target": { "field": "title" },
      "message": "title must be non-empty, single-line, and at most 200 characters"
    }
  ]
}
```

## Web UI

`/` はログイン後の閲覧 UI、`/notes/{note_id}` は個別ノート表示です。HTML 表示と一覧には、当該利用者が
閲覧可能なノートだけを出します。
