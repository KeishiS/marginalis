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

`marginalis-web`はHTTP、Cookie、CSRF、OAuthのリクエストを受け付けます。`marginalis-server`は
設定と外部接続用の処理をアプリケーション層へ接続し、`marginalis-sqlite`はSQLiteへの保存を
担当します。
`marginalis-auth-oidc` は OIDC discovery・code exchange・ID token 検証を担当します。MCP の
JSON-RPC wire 型は、それを利用する唯一の transport である `marginalis-web::mcp` に置きます。
実行バイナリは `marginalis-service` です。

## 一貫して満たすべき設計条件

- ノートと削除状態の更新はSQLite transactionで完結する。
- 通常利用者は、作成者の`(issuer, subject)`が自身のidentityと一致するノートだけを操作できる。
  `server-admins`は所有者にかかわらずすべてのノートを操作できる。
- identity は `(issuer, subject)` で識別する。アプリケーションはローカル password、root、登録ポリシーを
  持たない。`issuer`はuserinfo、query、fragment、制御文字を含まない絶対HTTP(S) URL、
  `subject`は空でなく制御文字を含まない値とし、長さ上限をdomainで一元検証する。
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
- HTTP、MCP、Web UIは所有者認可とrevisionの業務規則を複製しない。

## ソース配置

```text
crates/
├── marginalis-domain          値と設計条件
├── marginalis-application     portとuse case interface
├── marginalis-asciidoc        AsciiDoc検証・描画・export
├── marginalis-auth-oidc       Kanidm OIDC adapter
├── marginalis-sqlite          SQLite adapter
├── marginalis-server          production adapterの組立部品
├── marginalis-web             HTTP adapter
├── marginalis-service         composition rootと実行バイナリ
└── marginalis-integration-tests

frontend/
├── src                        React・TypeScriptの実装
└── tests                      ブラウザーに依存しないUI試験
```

依存は概ね上から下ではなく、外側から`domain`と`application`へ向かう。
`domain`は他のMarginalis crateへ依存せず、`application`は`domain`だけへ依存する。
HTTPとSQLiteは互いに依存せず、`service`が`server`を介して組み立てる。

crateは独立した依存境界または再利用単位にだけ使い、HTTP handlerやSQLite tableごとの整理には
crate内moduleを使う。各crateの`lib.rs`は公開facade、routerまたはcomposition rootとして、
実行経路と公開型を短く一覧できる状態に保つ。

Web UIでは、Rustが認証、認可、初期HTML、REST API、静的アセットの配信を担当し、Reactは
編集画面のブラウザー内状態を担当する。Viteの成果物はGitで管理せず、開発時は`cargo make`、
配布時はNixが`frontend/dist`を生成してRustバイナリーへ埋め込む。アセット、画面遷移、REST APIの
外部URLはViteで固定せず、Rustの`external_path`でbase URLのサブパスを反映する。

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
