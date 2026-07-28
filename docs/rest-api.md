# REST API

この文書は、REST APIを利用する人に向けて、認証、主な接続先、権限、入力エラーを説明します。
正式な入出力は[OpenAPI 3.1](openapi.json)で定めています。

## 概要

REST APIは`/api/v3`で提供します。実行中の`GET /api/v3/openapi.json`からもOpenAPIを取得できます。
`/api/v1`とローカル`root`用の管理APIは提供しません。

## 認証と変更操作

Web UIからREST APIを利用する場合は、OIDCログイン時に発行したセッションCookieを使用します。
作成、更新、削除などの変更操作では、同時に発行したCSRFトークンを`X-CSRF-Token`ヘッダーで
送信してください。リクエストの`Origin`が公開ベースURLと一致しない場合は拒否します。

## 主な接続先

| 操作 | 接続先 | 備考 |
| --- | --- | --- |
| 稼働確認 | `GET /api/v3/health` | 認証不要 |
| セッション確認 | `GET /api/v3/session` | Kanidmの利用者識別子 |
| ノート一覧 | `GET /api/v3/notes` | 閲覧できるノートだけ |
| ノート作成 | `POST /api/v3/notes` | CSRFトークンが必要 |
| 保存前プレビュー | `POST /api/v3/notes/preview` | 保存と同じ検査・HTML変換 |
| ノート取得 | `GET /api/v3/notes/{note_id}` | 閲覧できるノートだけ |
| 閲覧画面取得 | `GET /api/v3/notes/{note_id}/view` | 正本、権限、描画HTML、関連概要の一貫した組 |
| ノート更新 | `PUT /api/v3/notes/{note_id}` | `If-Match`が必要 |
| ノート削除 | `DELETE /api/v3/notes/{note_id}` | `If-Match`が必要 |
| AsciiDoc書き出し | `GET /api/v3/notes/{note_id}/source` | 閲覧できるノートだけ |
| ノート復元 | `POST /api/v3/notes/{note_id}/restore` | 削除後30日以内 |
| ACL取得 | `GET /api/v3/notes/{note_id}/acl` | 所有者だけ |
| ACL更新 | `PUT /api/v3/notes/{note_id}/acl` | CSRFトークンと`If-Match`が必要 |
| MCP認可の取消 | `DELETE /api/v3/mcp-authorizations/{client_id}` | 関連するトークンも失効 |

## ノートの入力と権限

ノートの作成・更新では、JSON形式の`title`、`body`、`tags`を送信します。本文はUTF-8で
512 KiB以下です。

保存前プレビューにも同じ入力を送り、成功時は安全なHTMLを受け取ります。プレビューは保存処理を
行いませんが、ログイン中の利用者だけが同一オリジンとCSRFトークンを確認したうえで利用できます。
入力規則に違反した場合の診断は、作成・更新と同じ形式です。

通常利用者は、自身が作成したノートと、同じ発行者内でACLにより共有されたノートを操作できます。
`read`は閲覧、`edit`は閲覧と内容の更新を許可します。ACL管理と削除・復元は所有者だけが
実行できます。権限のないノートは、存在を推測できないよう、HTTP状態コード
`404`と`code: "not_found"`を返します。

更新、削除、復元、ACL更新では、直前の取得応答に含まれる`ETag`を`If-Match`へそのまま指定します。
たとえば`ETag: "rev-3"`を受け取った場合は`If-Match: "rev-3"`を送ります。ヘッダーがない場合は
`428 precondition_required`、形式が不正な場合は`400 invalid_request`、他の操作によりrevisionが
進んでいる場合は`409 conflict`を返します。RESTのJSON本文に`expected_revision`は含めません。
MCPではHTTPヘッダーを使わないため、変更ツールの型付き引数`expected_revision`を使用します。

## 入力内容の検査

入力規則に違反した場合は、HTTP状態コード`422`と`code: "validation_failed"`を返します。
`diagnostics`には、問題の種類を表す`code`、対象項目、本文中の位置、説明を含めます。位置は
UTF-8で符号化した`body`上のバイト範囲です。タイトルとタグの問題には本文中の位置を付けません。
ACLの対象が不正、重複、または所有者自身である場合は、対象を`acl_entry`とその添字で示します。

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

`/`はログイン後の一覧画面、`/notes/{note_id}`は個別のノートを表示する画面です。
`/notes/new`でノートを作成し、`/notes/{note_id}/edit`で題名、AsciiDoc本文、タグを編集します。
所有者は`/notes/{note_id}/access`でACLを編集します。
これらはすべて一つのReactアプリケーションを起点に描画します。Rustが返す初期HTMLにはノート本文を
埋め込まず、生成済みTypeScriptクライアントがREST APIの応答を実行時に検査してから画面へ渡します。
編集画面はこの文書で説明するREST APIを利用し、明示的な保存操作でCSRFトークンと現在の
`ETag`を送信します。入力検査に失敗した場合も、ブラウザー内の編集内容を維持します。

REST APIとWeb UIには同じ権限確認を適用し、現在の利用者が閲覧できるノートだけを表示します。
