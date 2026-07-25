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

詳細な設計判断は [再設計仕様](v0.3.0-design.md) を参照してください。
