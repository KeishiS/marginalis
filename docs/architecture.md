# Marginalis アーキテクチャ

この文書は、開発者に向けて、コンポーネントの役割、依存関係、一貫して満たすべき設計条件を
説明します。利用者向けの機能は[現行要件](requirements.md)、HTTPの入出力は
[OpenAPI](openapi.json)を参照してください。

## 構成

```text
Web UI / REST (/api/v3) / MCP Protected Resource
                    │
       marginalis-contract（公開契約）
                    │
          application use cases
                    │
SQLite canonical store ─ AsciiDoc ─ Kanidm OIDC ─ Auth0 token検証
                    │
          marginalis-service + NixOS module
```

`marginalis-web`はHTTP、Cookie、CSRF、MCPのリクエストを受け付けます。
`marginalis-contract`はRESTのデータ型とOpenAPI、TypeScriptクライアント、MCP toolの入出力型と
JSON Schemaの生成元です。公開形式を変える場合はこのcrateを変更し、生成物との差分検査を通します。
MCPのtool一覧と実行時応答も同じ`McpToolName`と出力型を使用し、HTTP adapterが公開JSONを手で
組み立てません。
`marginalis-application`はノート操作の手順と業務上の失敗理由を定義します。
ここでいうportは、applicationが外側の実装へ要求する小さなinterfaceです。
`marginalis-sqlite`は永続化port、`marginalis-asciidoc`は文書の検証・描画port、
`marginalis-web`は外部URL生成portを実装します。これにより、ノート操作の単体試験ではSQLite、
AsciiDoc engine、HTTP serverを起動する必要がありません。
`marginalis-asciidoc`の内部では、文書の解析と検査、ACL判定済み参照を使うHTML描画、JSON
archive変換を別々のmoduleへ分けます。AdocWeaveの解析・描画設定とnote profileの安全性規則は
それぞれ一か所に置き、各moduleが同じ設定を使用します。crate外にはapplication portの実装と、
保守コマンドに必要なarchiveおよび参照抽出だけを公開します。
`marginalis-auth-oidc`は外部identity provider portを実装し、OIDC discovery・code exchange・
ID token検証を担当します。利用を許可する`server-users`所属の判断はapplicationが担当します。
`marginalis-auth-oauth`はAuth0のmetadataとJWKSを取得し、MCP access tokenからKanidm identity、
group、scopeを検証するadapterです。クライアント登録、認可、token発行、refresh token、取消は
Auth0の責務であり、MarginalisのapplicationとSQLiteへ状態を持ちません。
MCPのJSON-RPC wire型は、それを利用する唯一のtransportである`marginalis-web::mcp`に置きます。
Streamable HTTPの入口、Bearer tokenとscopeの検証、初期化と通信条件の検査、tool実行は
`marginalis-web::http::mcp_transport`内の別moduleに置きます。これにより、公開toolを変更するときに
認証処理を、認証方式を変更するときにノート出力変換を読み替える必要がありません。
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
- `server-users`所属は、WebではKanidmの署名検証済みID token、MCPではAuth0の署名検証済みaccess
  tokenに格納したKanidm由来claimから決める。どちらも同じ上流`(issuer, subject)`を所有者identityに
  使用する。
- Web session は24時間のsliding idle期限と7日の絶対期限を持つ。有効性の検証とidle期限の延長は、
  SQLiteの一つの条件付き更新で行う。読み取り後に同じ遅延transactionを更新へ切り替えず、
  同じsessionへの並行要求をSQLiteの書き込み待機で直列化する。未完了OIDC login attemptは10分で
  失効し、発行時に期限切れ行を削除したうえで同時保留数を1,024件に制限する。
- OIDC ID tokenの署名方式はKanidm 1.10と結合試験で使う`ES256`だけを許可する。別の署名方式を
  追加する場合は[セキュリティ](security.md)の依存脆弱性判断を先に更新する。
- MarginalisはMCPのauthorization code、access token、refresh token、client登録を保存しない。
  Auth0 access tokenはrequestの検証中だけ扱い、ログや永続領域へ出力しない。MCP clientにKanidm
  tokenを渡さない。
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
├── marginalis-auth-oauth      Auth0 access token検証adapter
├── marginalis-sqlite          SQLite adapter
├── marginalis-web             HTTP adapter
└── marginalis-service         composition rootと実行バイナリ

frontend/
├── src                        React・TypeScriptの実装
└── tests                      ブラウザーに依存しないUI試験
```

公開契約とMCP通信のcrate内moduleは、次の責務に分けます。

```text
marginalis-contract/src/
├── lib.rs          RESTとMCP契約の短い公開入口
├── rest.rs         REST型、OpenAPI、TypeScript生成元
└── mcp.rs          tool名、入出力型、JSON Schema

marginalis-web/src/http/mcp_transport/
├── mod.rs           Streamable HTTPとJSON-RPCの処理順序
├── authorization.rs Bearer token、scope、browser origin
├── protocol.rs      media type、初期化、protocol version
└── tools.rs         入力検査、use case呼出し、契約型への変換
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
取消、最後に成功したHTMLの保持、入力変更時の古い診断の破棄は`useEditorPreview.ts`へ分離し、
編集画面は結果の状態と診断操作だけを表示します。保存を拒否する診断と保存を妨げない診断は
application層でそれぞれ`NoteValidationDiagnostic`と`NoteAdvisoryDiagnostic`に分けます。
AsciiDoc文書の入力は`AsciiDocEditor.tsx`へ閉じ込めたCodeMirrorが担当し、Reactのフォーム状態には
常に完全な文字列を渡します。CodeMirror側でAsciiDocを別の文書モデルへ変換しません。入力補助は
`asciiDocEditing.ts`が返す一回の文字列編集として適用し、解析とHTML生成は引き続きサーバー側の
AdocWeaveだけが担当します。採用理由と操作上の制約は
[CodeMirror採用判断](adr/0003-codemirrorをasciidoc編集基盤に採用.md)と
[AsciiDoc編集画面](web-ui-editor.md)を参照してください。
成功型は`error`を保持できず、失敗型はHTTP境界で常に`error`へ変換します。RESTでは位置と重大度を
共通の`NoteDiagnostic`契約として返し、成功したプレビューも診断を失わず画面へ渡します。
検証時に抽出したノート参照は作成、更新、プレビューで再利用し、同じ入力を参照抽出のためだけに
再解析しません。競合する三つの文書の行対応は`editorConflict.ts`、問題と診断の表示規則は
`editorPresentation.ts`へ置きます。これらはReactや通信に依存しないため、大きな文書を含む
境界条件を単体試験で確認します。base URLのサブパスを画面内URLへ反映する規則は`paths.ts`へ
集約し、一覧と編集画面で同じ処理を使います。
閲覧画面と編集プレビューは、サーバーが検査・生成したHTMLを`RenderedContent.tsx`だけから
表示します。この境界では、AdocWeaveの公開`data-*`属性だけからコードの言語と行番号、
数式の言語と表示形式を受け取ります。コード本文はtextとして行へ分け、数式もtextとして
MathJaxへ渡し、要素名、class、親子関係から意味を推測しません。表と長いコードのスクロール領域、
MathJaxの組版失敗通知も同じ表示境界へ置きます。外部CDNや未検査のHTMLは追加しません。
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
├── mcp_transport/  Protected Resource Metadata、Streamable HTTP、認証、tool実行
├── notes.rs         REST note API
├── ui.rs            閲覧UI
└── security.rs      HTTP security policy

marginalis-sqlite/src/
├── schema.rs        schema検証
├── session.rs       Web/OIDC session
├── notes.rs         noteと所有者認可
└── archive.rs       検証済みarchiveを一つのトランザクションで格納
```

公開routeは`marginalis-web/src/http.rs`、公開型は各crateの`lib.rs`から追跡します。小さな単体試験は
対象moduleの末尾へ置きます。共有fixtureが大きいHTTP試験は`http/tests/`でUI・REST・MCP、
SQLite試験は`marginalis-sqlite/src/tests/`でschema・ノート・sessionに分けます。OIDCとAuth0
access token検証は各認証adapterでmetadata、署名、claimの境界を試験します。

設計を確定した経緯は[再設計判断記録](v0.3.0-design.md)を参照してください。
