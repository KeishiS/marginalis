# REST API（初期境界）

初期公開ではWeb UIを提供せず、このREST APIを利用境界とする。

## 認証

`/api/v1/notes`以下の操作は、OIDC loginで発行した`marginalis_session` Cookieを必要とする。
sessionがないか、期限切れまたは失効済みなら`401 authentication-required`を返す。

login時には読み取り可能な`marginalis_csrf` Cookieも発行する。`POST`、`PUT`およびlogoutは、
同値を`X-CSRF-Token` headerへ付けなければならない。不在または不一致は`403`で拒否する。

## ノート正本

正本はUTF-8のAsciiDocであり、SQLiteは検索・ACL・参照の投影である。HTTP handlerはSQLiteへ
sourceを直接書き込まない。作成・更新は`NoteWriteService`を呼び、journal、ファイル正本、投影の
順序を調停する。

| 操作 | endpoint | 成功応答 | 認可 |
| --- | --- | --- | --- |
| 作成 | `POST /api/v1/notes` | `201`、`Location` | sessionの`user_id`と文書の`creator-id`が一致 |
| 正本取得 | `GET /api/v1/notes/{note_id}/source` | `200`、AsciiDoc bytes | Read以上 |
| 正本更新 | `PUT /api/v1/notes/{note_id}/source` | `204` | Write以上 |
| ACL更新 | `PUT /api/v1/notes/{note_id}/acl/{user_id}` | `204` | Adminまたはroot |

作成・更新のrequest bodyはAsciiDoc sourceそのものとする。保存前に`marginalis-asciidoc`が、必須
metadata、危険な構文、`xref:note:`、anchor、およびsource言語のprofileを検証して投影を作る。

`PUT`ではURLの`note_id`と文書内`:note-id:`が一致しなければ`422 validation-failed`を返す。
既存ノートの`:creator-id:`は不変であり、更新で変更しようとすると`422 validation-failed`を返す。
`POST`で同じnote IDの正本がすでに存在すれば`409 conflict`を返す。

ACL更新のbodyは`{"permission":"read"}`、`"write"`、`"admin"`、または
`{"permission":null}`（直接ACLの解除）である。最後の直接管理者を解除・降格する更新は拒否する。

対象不在、不正なUUIDv7、またはRead/Write権限がない場合は、存在を推測させないため同じ
`404 not-found`を返す。

## Session終了

`POST /auth/logout`は現在のsessionを失効させ、`marginalis_session` Cookieを削除して`204`を返す。
