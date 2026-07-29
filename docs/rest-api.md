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
| ノート一覧 | `GET /api/v3/notes` | 閲覧できるノートの概要と実効アクセス水準 |
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

## ノートの入力と権限

ノートの作成・更新では、JSON形式の`source`へ完全なAsciiDoc文書を入れて送信します。題名は
文書題名、タグは文書ヘッダーの`:tags:`属性として記述します。`source`はUTF-8で512 KiB以下です。
`:tags:`を複数回記述した場合は最後の値、`:tags!:`で解除した場合はタグなしになります。属性参照と
`\`による複数行値は、その位置でAdocWeaveが評価した最終値からタグを導出します。改行を残す
`+ \`は単一行というタグ規則に合わないため拒否します。詳しい移行判断は
[AdocWeave 0.17移行判断](adocweave-v0.17-migration.md)を参照してください。

```json
{
  "source": "= 新規ノート\n:tags: new, research\n:sectnums:\n\n== 見出し1\n\nこれはテスト用の本文です。"
}
```

`:sectnums:`、`:toc:`、`:toclevels:`、`:stem:`は表示用の文書属性として使用できます。これらの
属性操作は文書header内だけで使用できます。`.題名`と`[source,言語名,linenums,start=開始行]`を
指定したコードブロックは題名、言語名、行番号の開始位置を表示し、長い行はコードブロック内で
横にスクロールできます。`:stem: latexmath`を指定した`stem:[]`と`[latexmath]`の数式は、
Web UIの配布物へ固定したMathJaxで組版します。外部のCDNへは接続しません。
横幅を超える表とブロック数式も、それぞれの表示領域内で横にスクロールできます。MathJaxの
読み込みまたは組版に失敗した場合は、画面上に失敗を表示します。
サーバーが管理する識別子、所有者、時刻、revision、ACLはAsciiDoc文書へ記述せず、APIの別項目で
扱います。

保存前プレビューにも同じ入力を送り、成功時は安全なHTMLと`diagnostics`を受け取ります。
`warning`、`information`、`hint`の診断は保存を妨げません。たとえば、文章と`xref:`の間に
空白がない場合は、AdocWeaveの`macro-boundary`を`warning`として返します。プレビューは保存処理を
行いませんが、ログイン中の利用者だけが同一オリジンとCSRFトークンを確認したうえで利用できます。
入力規則に違反した`error`の診断は、作成・更新と同じ形式でHTTP状態コード`422`を返します。

```json
{
  "html": "<article><p>安全に変換した本文</p></article>",
  "diagnostics": [
    {
      "code": "macro-boundary",
      "severity": "warning",
      "target": { "field": "source" },
      "span": { "start": 34, "end": 38, "unit": "utf8_byte" },
      "message": "a space is required before the inline macro"
    }
  ]
}
```

通常利用者は、自身が作成したノートと、同じ発行者内でACLにより共有されたノートを操作できます。
`read`は閲覧、`edit`は閲覧と内容の更新を許可します。ACL管理と削除・復元は所有者だけが
実行できます。権限のないノートは、存在を推測できないよう、HTTP状態コード
`404`と`code: "not_found"`を返します。

ノート一覧は本文を返さず、ノートID、題名、タグ、更新日時、revision、現在の利用者に適用される
`read`、`edit`、`manage`のいずれかの実効アクセス水準を返します。Web UIではタグと更新日で
表示対象を絞り込み、20件ずつ表示します。絞り込み条件とページはURLへ保存するため、閲覧画面や
編集画面から一覧へ戻っても同じ状態を再現できます。

更新、削除、復元、ACL更新では、直前の取得応答に含まれる`ETag`を`If-Match`へそのまま指定します。
たとえば`ETag: "rev-3"`を受け取った場合は`If-Match: "rev-3"`を送ります。ヘッダーがない場合は
`428 precondition_required`、形式が不正な場合は`400 invalid_request`、他の操作によりrevisionが
進んでいる場合は`409 conflict`を返します。RESTのJSON本文に`expected_revision`は含めません。
MCPではHTTPヘッダーを使わないため、変更ツールの型付き引数`expected_revision`を使用します。

## 入力内容の検査

入力規則に違反した場合は、HTTP状態コード`422`と`code: "validation_failed"`を返します。
`diagnostics`には、問題の種類を表す`code`、重大度を表す`severity`、対象項目、文書中の位置、
説明を含めます。`severity`は`error`、`warning`、`information`、`hint`のいずれかです。位置は
UTF-8で符号化した`source`上のバイト範囲です。AdocWeave由来の診断では、AdocWeaveの安定した
`code`をそのまま使用します。画面の日本語表示は英語の`message`ではなく`code`から決定します。
ACLの対象が不正、重複、または所有者自身である場合は、対象を`acl_entry`とその添字で示します。

```json
{
  "code": "validation_failed",
  "message": "note input is invalid",
  "diagnostics": [
    {
      "code": "invalid_title",
      "severity": "error",
      "target": { "field": "source" },
      "message": "title must be non-empty, single-line, and at most 200 characters"
    }
  ]
}
```

## Web UIとの関係

`/`はログイン後の一覧画面、`/notes/{note_id}`は個別のノートを表示する画面です。
`/notes/new`でノートを作成し、`/notes/{note_id}/edit`で完全なAsciiDoc文書を一つの欄で編集します。
所有者は`/notes/{note_id}/access`でACLを編集します。
これらはすべて一つのReactアプリケーションを起点に描画します。Rustが返す初期HTMLにはノート本文を
埋め込まず、生成済みTypeScriptクライアントがREST APIの応答を実行時に検査してから画面へ渡します。
編集画面はこの文書で説明するREST APIを利用し、明示的な保存操作でCSRFトークンと現在の
`ETag`を送信します。入力検査に失敗した場合も、ブラウザー内の編集内容を維持します。
位置を含む診断は、該当するAsciiDoc文書の範囲を入力欄で選択できます。プレビュー更新に失敗した
場合は、失敗を表示したうえで最後に成功したプレビューを残します。保存できる入力に警告がある
場合は、安全に変換した最新のプレビューと警告を同時に表示します。入力を変更した時点で古い
診断を取り除き、新しい入力上の位置として誤って表示しません。
編集操作、表示方式、入力補助、スクロール同期の制約は
[AsciiDoc編集画面](web-ui-editor.md)で説明します。

REST APIとWeb UIには同じ権限確認を適用し、現在の利用者が閲覧できるノートだけを表示します。
