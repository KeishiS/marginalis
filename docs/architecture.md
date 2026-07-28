# Marginalis アーキテクチャ

この文書は、開発者に向けて、コンポーネントの役割、依存関係、一貫して満たすべき設計条件を
説明します。利用者向けの機能は[現行要件](requirements.md)、HTTPの入出力は
[OpenAPI](openapi.json)を参照してください。

## 構成

```text
Web UI / REST (/api/v3) / MCP (Streamable HTTP)
                    │
       marginalis-contract（公開契約）
                    │
          application use cases
                    │
SQLite canonical store ─ AsciiDoc import/export ─ Kanidm OIDC
                    │
          marginalis-service + NixOS module
```

`marginalis-web`はHTTP、Cookie、CSRF、OAuthのリクエストを受け付けます。
`marginalis-contract`はRESTのデータ型とOpenAPI、TypeScriptクライアント、MCPツール定義の
生成元です。公開形式を変える場合はこのcrateを変更し、生成物との差分検査を通します。
`marginalis-application`はノート操作の手順と業務上の失敗理由を定義します。
ここでいうportは、applicationが外側の実装へ要求する小さなinterfaceです。
`marginalis-sqlite`は永続化port、`marginalis-asciidoc`は文書の検証・描画port、
`marginalis-web`は外部URL生成portを実装します。これにより、ノート操作の単体試験ではSQLite、
AsciiDoc engine、HTTP serverを起動する必要がありません。
`marginalis-auth-oidc`は外部identity provider portを実装し、OIDC discovery・code exchange・
ID token検証を担当します。利用を許可する`server-users`所属の判断はapplicationが担当します。
MCP OAuthのclient、認可code、token familyの規則もapplicationに置き、
SQLiteはその永続化portを実装します。
MCPのJSON-RPC wire型は、それを利用する唯一のtransportである`marginalis-web::mcp`に置きます。
実行バイナリは`marginalis-service`です。

## 一貫して満たすべき設計条件

- ノートと削除状態の更新はSQLite transactionで完結する。
- 通常利用者は、作成者の`(issuer, subject)`が自身のidentityと一致するノートを所有する。同じ
  `issuer`の個人subjectへ付与されたACLの`read`は閲覧、`edit`は閲覧と本文更新を許可する。
  所有者は変更不能であり、ACL変更と削除・復元は所有者だけに許可する。サーバー全体の管理者権限や、
  ACLを迂回して全ノートを閲覧する権限は設けない。
- identity は `(issuer, subject)` で識別する。アプリケーションはローカル password、root、登録ポリシーを
  持たない。`issuer`はuserinfo、query、fragment、制御文字を含まない絶対HTTP(S) URL、
  `subject`は空でなく制御文字を含まない値とし、長さ上限をdomainで一元検証する。
- ノートなどの永続的な識別子にはUUIDv7だけを受理する。文字列、JSON、データベースからの復元を
  含むすべての入力経路で同じ検査を行い、検査を省略する公開constructorは設けない。
- `Note`は検証済みの所有者identityと正の値だけを表す`Revision`を保持し、作成日時から更新日時までの順序、
  削除日時の範囲を生成時と復元時に検査する。フィールドを直接変更する公開APIは設けない。
  SQLite行とarchive JSONからの復元も同じconstructorを通し、不整合を各adapterで重複して
  検査しない。
- `server-users`所属は、OIDC login時に署名検証した`groups` claimから決める。発行したsessionと
  MCP authorizationは、login時に検証したidentityを有効期間中保持する。
- Web session は24時間のsliding idle期限と7日の絶対期限を持つ。未完了OIDC login attemptは10分で失効し、
  発行時に期限切れ行を削除したうえで同時保留数を1,024件に制限する。
- OIDC ID tokenの署名方式はKanidm 1.10と結合試験で使う`ES256`だけを許可する。別の署名方式を
  追加する場合は[セキュリティ](security.md)の依存脆弱性判断を先に更新する。
- authorization code、access token、refresh tokenはhashだけをSQLiteに保存する。認可codeの消費と
  token pair発行は一つのtransactionで行い、codeまたはrefresh tokenのreplay時はtoken familyを失効する。
  消費済みcodeは対応するtoken familyが残る間だけreplay検知用に保持する。
  MCP clientにKanidm tokenを渡さない。
- HTTP、MCP、Web UIは所有者・ACL認可とrevisionの業務規則を複製しない。
- 実効アクセス水準は`Read < Edit < Manage`の順序で表す。SQLiteの`note_access`投影が、
  所有者の`Manage`とACLの`Read`または`Edit`を同じ判断表へまとめる。

## ソース配置

```text
crates/
├── marginalis-domain          値と設計条件
├── marginalis-contract        REST・MCP・TypeScriptの公開契約
├── marginalis-application     use case実装と内向き・外向きport
├── marginalis-asciidoc        AsciiDoc検証・描画・export
├── marginalis-auth-oidc       Kanidm OIDC adapter
├── marginalis-sqlite          SQLite adapter
├── marginalis-web             HTTP adapter
├── marginalis-service         composition rootと実行バイナリ
└── marginalis-integration-tests

frontend/
├── src                        React・TypeScriptの実装
└── tests                      ブラウザーに依存しないUI試験
```

依存は概ね上から下ではなく、外側から`domain`と`application`へ向かう。
`domain`は他のMarginalis crateへ依存せず、`application`は`domain`だけへ依存する。
HTTP、SQLite、AsciiDocは互いに依存せず、それぞれapplicationのportを実装する。
`service`だけが具象的なadapterを選び、application serviceへ接続する。

ノート操作の内向きportは、問い合わせの`NoteQueries`、変更の`NoteCommands`、表示変換の
`NotePresentation`、ACL管理の`NoteAccessControl`に分けます。複数のtransportへ同じ実装を渡す
場合だけ、これらをまとめた`NoteUseCases`をfacadeとして使います。
閲覧画面は`NotePresentation::read_note_view`だけを呼び出し、ノート、実効アクセス水準、描画HTML、
関連概要を個別の問い合わせから組み立てません。試験用adapterもこの境界を直接実装し、実際には
存在しない分割問い合わせを模倣しません。
applicationから永続化へ要求する外向きportも、読み取りの`NoteQueryRepository`、原子的な変更の
`NoteCommandRepository`、ACL操作の`NoteAclRepository`に分けます。具象的には同じSQLite adapterが
三つを実装しますが、application serviceは用途ごとに必要なportだけを受け取ります。
SQLiteのエラー型やAsciiDoc engineの型はapplicationの公開境界へ出しません。

ノートの正本は、文書題名、`:tags:`などの文書属性、本文を含む完全なAsciiDoc文書です。題名と
タグは保存時にAdocWeaveで解析し、一覧と検索に使う投影としてSQLiteへ同時に保存します。APIから
題名やタグだけを独立して更新する経路は設けません。

一覧のportはAsciiDoc文書を含まない`NoteSummary`と、現在の利用者の`NoteAccess`を組にした
`NoteListEntry`を返します。SQLiteでは概要と実効アクセス水準を一つの問い合わせで取得し、
ノート数に比例して問い合わせを繰り返しません。文書中の参照先もID集合を一度にrepositoryへ渡して
取得します。変更操作は認可、削除状態、期待revisionを一つの条件付きSQLへ含めます。条件に
一致しなかった場合だけ、同じtransaction内で不可視と競合を分類します。
閲覧画面に必要な正本、実効アクセス水準、参照先、関連概要は、一つのSQLite読み取りtransactionで
取得します。描画はこのスナップショットだけを使うため、一画面の途中で別の更新結果が混ざりません。

アーカイブのJSON項目を表す型は、形式を解釈する`marginalis-asciidoc`に置きます。JSONから復元した
`Note`とACLは、`marginalis-application`の`LogicalSnapshot`でノートIDの重複、ACLの参照先、
所有者と共有先の関係を一度だけ検証します。本文から再構築する参照索引も検証した
`RestorePlan`だけをSQLite adapterへ渡します。これにより、ドメイン型を外部形式へ直接公開せず、
SQLite adapterはJSON形式やAsciiDocの解析方法に依存しません。

crateは独立した依存境界または再利用単位にだけ使い、HTTP handlerやSQLite tableごとの整理には
crate内moduleを使う。各crateの`lib.rs`は公開facade、routerまたはcomposition rootとして、
実行経路と公開型を短く一覧できる状態に保つ。

Web UIでは、Rustが認証、認可、初期HTML、REST API、静的アセットの配信を担当し、Reactは
画面遷移とブラウザー内状態を担当します。RESTのTypeScript型、実行時の応答検査、クライアント関数は
`marginalis-contract`から`frontend/src/generated/contracts.ts`へ生成し、手書きで複製しません。
編集内容、保存処理、共有設定の状態遷移は、それぞれ`editorState.ts`、`editorActivityState.ts`、
`accessControlState.ts`の純粋なreducerへ置きます。Reactコンポーネントは入力、REST呼び出し、
副作用の調整を担当し、状態遷移の規則をイベント処理へ分散させません。保存前プレビューの遅延、
取消、最後に成功したHTMLの保持は`useEditorPreview.ts`へ分離し、編集画面は結果の状態と診断操作
だけを表示します。
閲覧画面と編集プレビューは、サーバーが検査・生成したHTMLを`RenderedContent.tsx`だけから
表示します。この境界でコードの言語表示、表のスクロール領域、MathJax入力への変換と組版失敗通知を
加えます。外部CDNや未検査のHTMLを追加せず、AdocWeaveが将来同じ表示情報を公開した場合に
`renderedContentEnhancement.ts`の変換だけを置き換えられる構成とします。
Viteの成果物はGitで管理せず、開発時は`cargo make`、
配布時はNixが`frontend/dist`を生成してRustバイナリーへ埋め込む。アセット、画面遷移、REST APIの
外部URLはViteで固定せず、Rustの`external_path`でbase URLのサブパスを反映する。

`frontend/tests`は入力、プレビュー、保存、競合などの画面状態と組合せをブラウザーなしで高速に
検証する。`tests/browser`は実Kanidm、TLS、サブパスを組み立てるNixOS VMで、ログインから主要な
利用経路までの接続を検証する。細かな入力の組合せはNixOS VMへ重複させない。

閲覧画面の直接参照一覧は、保存時にAsciiDoc文書と同じtransactionで置換した参照先IDだけを投影へ持つ。
閲覧時は投影を現在の認可と削除状態に結合し、題名、タグ、更新日時だけを取得する。本文更新、
ACL変更、ソフトデリート、復元、物理削除へ追従しながら、不可視なノートの存在や件数を漏らさず、
閲覧ごとの全本文解析も行わない。archiveにはACLを含め、派生する参照投影は含めず、復元時に
検証済みAsciiDoc文書から再構築する。復元処理は、ノート、ACL、参照索引を検証済みの復元計画として受け取り、
空のdatabaseへ一つのtransactionで格納する。

主要な外側のadapterは、変更理由に対応して次のmoduleへ分ける。

```text
marginalis-service/src/
├── main.rs          process lifecycleとcommand選択
├── cli.rs           引数仕様
├── config.rs        環境変数とsecret fileの読込
├── runtime.rs       production時刻・乱数adapter
├── serve.rs         HTTP composition root
└── maintenance.rs   purge、archive、backup

marginalis-web/src/http/
├── assets.rs        埋め込み静的アセット
├── auth.rs          browser session、Cookie、CSRF
├── html.rs          共通HTMLレイアウト
├── oauth.rs         MCP OAuth endpoint
├── mcp_transport.rs MCP Streamable HTTP
├── notes.rs         REST note API
├── ui.rs            閲覧UI
└── security.rs      HTTP security policy

marginalis-sqlite/src/
├── schema.rs        schema検証
├── session.rs       Web/OIDC session
├── mcp.rs           MCP OAuth永続化
├── notes.rs         noteと所有者認可
└── archive.rs       検証済みarchiveを一つのトランザクションで格納
```

公開routeは`marginalis-web/src/http.rs`、公開型は各crateの`lib.rs`から追跡します。小さな単体試験は
対象moduleの末尾へ置きます。共有fixtureが大きいHTTP試験は`http/tests/`でUI・REST・MCP・OAuth、
SQLite試験は`marginalis-sqlite/src/tests/`でschema・ノート・session・OAuthに分けます。
複数crateを接続するOIDC・MCP試験だけを`marginalis-integration-tests/tests/`へ置き、
完全な認証経路、利用条件、discoveryを別suiteとして単独実行できるようにします。

設計を確定した経緯は[再設計判断記録](v0.3.0-design.md)を参照してください。
