# REST API

この文書は、REST APIを利用する人に向けて、認証、主な接続先、権限、入力エラーを説明します。
正式な入出力は[OpenAPI 3.1](openapi.json)で定めています。

## 概要

REST APIは`/api/v2`で提供します。実行中の`GET /api/v2/openapi.json`からもOpenAPIを取得できます。
`/api/v1`とローカル`root`用の管理APIは提供しません。

## 認証と変更操作

Web UIからREST APIを利用する場合は、OIDCログイン時に発行したセッションCookieを使用します。
作成、更新、削除などの変更操作では、同時に発行したCSRFトークンを`X-CSRF-Token`ヘッダーで
送信してください。リクエストの`Origin`が公開ベースURLと一致しない場合は拒否します。

## 主な接続先

| 操作 | 接続先 | 備考 |
| --- | --- | --- |
| 稼働確認 | `GET /api/v2/health` | 認証不要 |
| セッション確認 | `GET /api/v2/session` | Kanidmの利用者識別子と管理者フラグ |
| ノート一覧 | `GET /api/v2/notes` | 閲覧できるノートだけ |
| ノート作成 | `POST /api/v2/notes` | CSRFトークンが必要 |
| ノート取得 | `GET /api/v2/notes/{note_id}` | 閲覧できるノートだけ |
| ノート更新 | `PUT /api/v2/notes/{note_id}` | `expected_revision`が必要 |
| ノート削除 | `DELETE /api/v2/notes/{note_id}` | `expected_revision`が必要 |
| AsciiDoc書き出し | `GET /api/v2/notes/{note_id}/source` | 閲覧できるノートだけ |
| ノート復元 | `POST /api/v2/notes/{note_id}/restore` | 削除後30日以内 |
| MCP認可の取消 | `DELETE /api/v2/mcp-authorizations/{client_id}` | 関連するトークンも失効 |

## ノートの入力と権限

ノートの作成・更新では、JSON形式の`title`、`body`、`tags`を送信します。本文はUTF-8で
512 KiB以下です。

通常利用者は、自身が作成したノートだけを操作できます。`server-admins`グループに属する利用者は、
すべてのノートを操作できます。権限のないノートは、存在を推測できないよう、HTTP状態コード`404`と
`code: "not_found"`を返します。

## 入力内容の検査

入力規則に違反した場合は、HTTP状態コード`422`と`code: "validation_failed"`を返します。
`diagnostics`には、問題の種類を表す`code`、対象項目、本文中の位置、説明を含めます。位置は
UTF-8で符号化した`body`上のバイト範囲です。タイトルとタグの問題には本文中の位置を付けません。

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

## Web UIとの関係

`/`はログイン後の一覧画面、`/notes/{note_id}`は個別のノートを表示する画面です。REST APIと
Web UIには同じ権限確認を適用し、現在の利用者が閲覧できるノートだけを表示します。
