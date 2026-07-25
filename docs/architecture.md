# Marginalis アーキテクチャ

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

`marginalis-web` は HTTP、Cookie、CSRF、OAuth の境界を担当します。`marginalis-server` は設定と
adapter を application port に接続し、`marginalis-sqlite` は単一の SQLite database を実装します。
`marginalis-auth-oidc` は OIDC discovery・code exchange・ID token 検証を担当します。MCP の
JSON-RPC wire 型は、それを利用する唯一の transport である `marginalis-web::mcp` に置きます。
実行バイナリは `marginalis-service` です。

## 不変条件

- ノート、ACL、削除状態の更新は SQLite transaction で完結する。
- identity は `(issuer, subject)` で識別する。アプリケーションはローカル password、root、登録ポリシーを
  持たない。
- `server-users` と `server-admins` は、OIDC login 時に署名検証した `groups` claim から決め、その session と
  MCP authorization の有効期間は固定する。
- access token と refresh token は hash だけを SQLite に保存する。MCP client に Kanidm token を渡さない。
- HTTP、MCP、Web UI は可視性・ACL・revision の業務規則を複製しない。

## ソース配置

```text
crates/
├── marginalis-domain          値・不変条件
├── marginalis-application     portとuse case契約
├── marginalis-asciidoc        AsciiDoc検証・描画・export
├── marginalis-auth-oidc       Kanidm OIDC adapter
├── marginalis-sqlite          SQLite adapter
├── marginalis-server          production adapterの組立部品
├── marginalis-web             HTTP adapter
├── marginalis-service         composition rootと実行バイナリ
└── marginalis-integration-tests
```

依存は概ね上から下ではなく、外側から`domain`と`application`へ向かう。
`domain`は他のMarginalis crateへ依存せず、`application`は`domain`だけへ依存する。
HTTPとSQLiteは互いに依存せず、`service`が`server`を介して組み立てる。

crateは独立した依存境界または再利用単位にだけ使い、HTTP handlerやSQLite tableごとの整理には
crate内moduleを使う。各crateの`lib.rs`は公開facade、routerまたはcomposition rootとして、
実行経路と公開型を短く一覧できる状態に保つ。

設計を確定した経緯は[再設計判断記録](v0.3.0-design.md)を参照してください。
