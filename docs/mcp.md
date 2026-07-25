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

`/mcp` は Cookie を使わず、すべての request を `Authorization: Bearer` で認可します。`Origin` がある
browser client は DNS rebinding 対策として完全一致の許可リストで検証し、NixOS module の既定値は
`https://chatgpt.com` です。Codex CLI と Claude Code のように `Origin` を送らない native client はこの
制約の対象外です。

同じ許可リストは MCP OAuth の承認 form POST にも使います。ChatGPT Web が cross-site POST を行う場合でも、
session と session 結合済み CSRF token の照合が必須であり、通常の Web API の Origin 制約は緩和しません。

scope は `notes:read`、`notes:write`、`notes:delete` です。scope だけでは不十分であり、Web と同じ
ノート ACL が必ず適用されます。`server-admins` はすべてのノートに管理者相当でアクセスします。

利用者は Web session と CSRF token を使って、`DELETE /api/v2/mcp-authorizations/{client_id}` から
個別 client の認可を取り消せます。取り消し後、その client の access token と refresh token は使えません。

OIDC login 時に検証した `groups` claim を Web session と MCP authorization の権限スナップショットとします。
group 変更は次回 login から反映され、既存 token は有効期限または認可取消まで発行時の権限を保持します。
