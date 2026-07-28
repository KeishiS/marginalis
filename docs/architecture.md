# Marginalis アーキテクチャ

この文書は、開発者に向けて、コンポーネントの役割、依存関係、一貫して満たすべき設計条件を
説明します。利用者向けの機能は[現行要件](requirements.md)、HTTPの入出力は
[OpenAPI](openapi.json)を参照してください。

## 構成

```text
Web UI / REST (/api/v2) / MCP (Streamable HTTP)
                    │
          application use cases
                    │
SQLite canonical store ─ AsciiDoc import/export ─ Kanidm OIDC
                    │
          marginalis-service + NixOS module
```

`marginalis-web`はHTTP、Cookie、CSRF、OAuthのリクエストを受け付けます。
`marginalis-application`はノート操作の手順と業務上の失敗理由を定義します。
ここでいうportは、applicationが外側の実装へ要求する小さなinterfaceです。
`marginalis-sqlite`は永続化port、`marginalis-asciidoc`は文書の検証・描画port、
`marginalis-web`は外部URL生成portを実装します。これにより、ノート操作の単体試験ではSQLite、
AsciiDoc engine、HTTP serverを起動する必要がありません。
`marginalis-server`は認証とOAuthの移行前のapplication serviceを保持します。
`marginalis-auth-oidc` は OIDC discovery・code exchange・ID token 検証を担当します。MCP の
JSON-RPC wire 型は、それを利用する唯一の transport である `marginalis-web::mcp` に置きます。
実行バイナリは `marginalis-service` です。

## 一貫して満たすべき設計条件

- ノートと削除状態の更新はSQLite transactionで完結する。
- 通常利用者は、作成者の`(issuer, subject)`が自身のidentityと一致するノートを所有する。同じ
  `issuer`の個人subjectへ付与されたACLの`read`は閲覧、`edit`は閲覧と本文更新を許可する。
  所有者は変更不能であり、ACL変更と削除・復元は所有者または`server-admins`だけに許可する。
  `server-admins`は所有者にかかわらずすべてのノートを操作できる。
- identity は `(issuer, subject)` で識別する。アプリケーションはローカル password、root、登録ポリシーを
  持たない。`issuer`はuserinfo、query、fragment、制御文字を含まない絶対HTTP(S) URL、
  `subject`は空でなく制御文字を含まない値とし、長さ上限をdomainで一元検証する。
- ノートなどの永続的な識別子にはUUIDv7だけを受理する。文字列、JSON、データベースからの復元を
  含むすべての入力経路で同じ検査を行い、検査を省略する公開constructorは設けない。
- `server-users` と `server-admins` は、OIDC login 時に署名検証した `groups` claim から決め、その session と
  MCP authorization の有効期間は固定する。
- Web session は24時間のsliding idle期限と7日の絶対期限を持つ。未完了OIDC login attemptは10分で失効し、
  発行時に期限切れ行を削除したうえで同時保留数を1,024件に制限する。
- OIDC ID tokenの署名方式はKanidm 1.10と結合試験で使う`ES256`だけを許可する。別の署名方式を
  追加する場合は[セキュリティ](security.md)の依存脆弱性判断を先に更新する。
- authorization code、access token、refresh tokenはhashだけをSQLiteに保存する。認可codeの消費と
  token pair発行は一つのtransactionで行い、codeまたはrefresh tokenのreplay時はtoken familyを失効する。
  消費済みcodeは対応するtoken familyが残る間だけreplay検知用に保持する。
  MCP clientにKanidm tokenを渡さない。
- HTTP、MCP、Web UIは所有者・ACL認可とrevisionの業務規則を複製しない。

## ソース配置

```text
crates/
├── marginalis-domain          値と設計条件
├── marginalis-application     use case実装と内向き・外向きport
├── marginalis-asciidoc        AsciiDoc検証・描画・export
├── marginalis-auth-oidc       Kanidm OIDC adapter
├── marginalis-sqlite          SQLite adapter
├── marginalis-server          移行前の認証・OAuth application service
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

ノート操作では、HTTPやMCPから呼び出す`NoteUseCases`を内向きport、applicationから
永続化へ要求する`NoteRepository`を外向きportと呼ぶ。前者は利用者の操作を表し、後者は
認可とrevisionを含む一つの原子的な保存操作を表す。SQLiteのエラー型やAsciiDoc engineの型は
applicationの公開境界へ出さない。

crateは独立した依存境界または再利用単位にだけ使い、HTTP handlerやSQLite tableごとの整理には
crate内moduleを使う。各crateの`lib.rs`は公開facade、routerまたはcomposition rootとして、
実行経路と公開型を短く一覧できる状態に保つ。

Web UIでは、Rustが認証、認可、初期HTML、REST API、静的アセットの配信を担当し、Reactは
編集画面のブラウザー内状態を担当する。Viteの成果物はGitで管理せず、開発時は`cargo make`、
配布時はNixが`frontend/dist`を生成してRustバイナリーへ埋め込む。アセット、画面遷移、REST APIの
外部URLはViteで固定せず、Rustの`external_path`でbase URLのサブパスを反映する。

`frontend/tests`は入力、プレビュー、保存、競合などの画面状態と組合せをブラウザーなしで高速に
検証する。`tests/browser`は実Kanidm、TLS、サブパスを組み立てるNixOS VMで、ログインから主要な
利用経路までの接続を検証する。細かな入力の組合せはNixOS VMへ重複させない。

閲覧画面の直接参照一覧は、保存時に本文と同じtransactionで置換した参照先IDだけを投影へ持つ。
閲覧時は投影を現在の認可と削除状態に結合し、題名、タグ、更新日時だけを取得する。本文更新、
ACL変更、ソフトデリート、復元、物理削除へ追従しながら、不可視なノートの存在や件数を漏らさず、
閲覧ごとの全本文解析も行わない。archiveにはACLを含め、派生する参照投影は含めず、復元時に
検証済み本文から再構築する。

主要な外側のadapterは、変更理由に対応して次のmoduleへ分ける。

```text
marginalis-service/src/
├── main.rs          process lifecycleとcommand選択
├── cli.rs           引数仕様
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

公開routeは`marginalis-web/src/http.rs`、公開型は各crateの`lib.rs`から追跡する。unit testは
対象moduleの末尾へ置き、HTTP・OIDC・MCPを一気通貫で通す試験だけを
`marginalis-integration-tests/tests/`へ置く。

設計を確定した経緯は[再設計判断記録](v0.3.0-design.md)を参照してください。
