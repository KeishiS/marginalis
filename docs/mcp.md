# MCP と OAuth

Marginalis は MCP の OAuth Authorization Server と Protected Resource を同一オリジンで提供します。
Kanidm token を MCP client に渡すことはありません。

| 対象 | endpoint |
| --- | --- |
| MCP | `POST B/mcp` |
| Protected Resource Metadata | `B/.well-known/oauth-protected-resource/mcp` |
| Authorization Server Metadata | `B/.well-known/oauth-authorization-server` |
| Dynamic Client Registration | `POST B/oauth/register` |
| Authorization | `GET` / `POST B/oauth/authorize` |
| Token | `POST B/oauth/token` |

ここで `B` は外部 base URL です。クライアントは Dynamic Client Registration を行い、Authorization
Code + PKCE S256 を使います。未ログインで authorization endpoint を開いた場合は OIDC login へ移動し、
認可リクエストへ安全に戻ります。

scope は `notes:read`、`notes:write`、`notes:delete` です。scope だけでは不十分であり、Web と同じ
ノート ACL が必ず適用されます。`server-admins` はすべてのノートに管理者相当でアクセスします。

利用者は Web session と CSRF token を使って、`DELETE /api/v2/mcp-authorizations/{client_id}` から
個別 client の認可を取り消せます。取り消し後、その client の access token と refresh token は使えません。

access token の利用時と refresh 時には Kanidm membership を最大 5 分ごとに検査します。`server-users`
から外れた利用者は拒否され、管理者グループから外れた利用者は直後の再検査から管理権限を失います。
