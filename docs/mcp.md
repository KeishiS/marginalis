# MCPとAuth0

この文書は、MCPを利用する人と運用者に向けて、クライアントの接続方法、Auth0が行う認可、
Marginalisが行うaccess token検証、MCPの通信仕様を説明します。配備設定は
[NixOSでの運用](nixos.md)、採用理由は
[Auth0をMCPのAuthorization Serverに採用](adr/0001-auth0をmcpのauthorization-serverに採用.md)を
参照してください。

## 構成

MarginalisはMCPのProtected Resourceです。Authorization Serverの機能は持たず、クライアント登録、
ログイン、同意、token発行、refresh token、認可取消をAuth0へ委ねます。Auth0はKanidmをOIDC
Enterprise Connectionとして利用します。これにより、MCPクライアントへKanidmのtokenや利用者の
パスワードを渡しません。

| 対象 | 接続先 |
| --- | --- |
| MCP | `POST B/mcp` |
| Protected Resource Metadata | RFC 9728で`B/mcp`から導出するURL |
| Authorization Server Metadata、DCR、認可、token | Protected Resource Metadataが示すAuth0 issuer |

ここで`B`は外部から利用するベースURLです。たとえば`B`が
`https://notes.example.test/marginalis`の場合、MCP URLは
`https://notes.example.test/marginalis/mcp`、Protected Resource Metadataは
`https://notes.example.test/.well-known/oauth-protected-resource/marginalis/mcp`です。
well-known suffixをhostとsubject pathの間へ挿入します。

Marginalisは`/oauth/*`、Authorization Server Metadata、クライアントごとの認可取消APIを公開しません。
クライアントはProtected Resource MetadataからAuth0を発見し、Auth0のDCRとAuthorization Code +
PKCE S256を使用します。

## Auth0に必要な設定

Auth0では、次の設定を一組として管理します。具体的な画面操作と障害対応は
[MCP向けAuthorization Serverの評価記録](mcp-authorization-server-evaluation.md)を参照してください。

- MCP URLと完全に一致するAPI identifier
- `notes:read`、`notes:write`、`notes:delete`のAPI permission
- Dynamic Client Registrationと第三者アプリケーション用の既定permission
- Kanidmへ接続するdomain-level OIDC Enterprise Connection
- New Universal Login
- RFC 8707の`resource`をAPI audienceへ対応付けるResource Parameter Compatibility Profile
- Kanidmで検証した上流`issuer`、`subject`、`groups`を名前空間付きclaimへ格納するLogin Action

Auth0固有の`sub`は所有者identityに使用しません。Marginalisは署名検証済みaccess tokenから上流の
`issuer`と`subject`を読み、Webログインと同じKanidm identityを復元します。`groups`には
`server-users`が必要です。claim名はNixOS設定で明示し、利用者が変更できるmetadataをidentityへ
変換しません。

## Access tokenの検証

Marginalisは起動時にAuth0のAuthorization Server MetadataとJWKSを取得します。取得または設定検証に
失敗した場合は、MCPを認証なしで起動せず、serviceの起動を失敗させます。実行中に未知の`kid`を受け取ると、
短時間の連続取得を避けながらJWKSを更新します。

受理するaccess tokenには、次の条件をすべて適用します。

- `RS256`署名とJWKS上の一致する鍵
- Auth0 issuerとの`iss`完全一致
- 公開MCP URLとの`aud`完全一致
- 有効な`exp`と`nbf`
- 設定した上流`issuer` claimとKanidm issuerの一致
- 空でない上流`subject` claim
- Login Actionで正規化された文字列の配列である`groups` claim
- `server-users`所属
- 空白区切りの`scope`

tokenの最大長、claim名、group数、group長、scope数、scope長には上限があります。不正なtokenは
`401 invalid_token`、必要なscopeを持たないtokenは`403 insufficient_scope`です。Auth0のmetadataや
JWKSを取得できない場合は`503`とし、無効な利用者tokenと運用障害を区別します。

ログにはtoken、claim値、利用者identityを記録しません。失敗種別は
`mcp.authentication.failed`、`mcp.authentication.unavailable`、
`mcp.authorization.discovery.failed`、`mcp.authorization.jwks_refresh.failed`の`reason`で確認します。
token拒否の`reason`は、`token-format`、`standard-claims`、
`identity-claims`、`groups-claim`、`scope-claim`のいずれかです。

## MCPへのリクエスト元

`/mcp`はCookieを使わず、すべてのrequestを`Authorization: Bearer`で認可します。`Origin`がある
browser clientはDNS rebinding対策として完全一致の許可リストで検証します。ChatGPT Web UIを使う場合は
`https://chatgpt.com`を明示します。Codex CLIやClaude Codeのように`Origin`を送らないnative clientは
この制約の対象外です。

## クライアントの接続

### ChatGPT Web UI

ChatGPTでcustom connectorを作成し、MCP URLを指定します。認証方式はOAuth、クライアント登録はDCRを
選択します。Auth0の同意画面で必要なscopeだけを許可します。接続後に一覧、作成、更新、削除を確認します。

### Claude Code

remote Streamable HTTP serverとして追加し、Claude Code内の`/mcp`からbrowser認証します。

```bash
claude mcp add --transport http marginalis https://notes.example.test/mcp
```

Auth0がClaude Codeのloopback callbackをDCRで受理する必要があります。SSH、container、WSLでは、
browserからcallback listenerへ到達できる構成も必要です。

### Codex CLI

remote Streamable HTTP serverとしてMCP URLを登録し、CodexのOAuth loginを開始します。クライアントが
送る`resource`が公開MCP URLと完全に一致することを確認します。

## Scopeとノート認可

| tool | scope | 用途 |
| --- | --- | --- |
| `get_note_profile` | `notes:read`または`notes:write` | 現行の入力制約と動作例の取得 |
| `list_notes` | `notes:read` | 可視ノートの一覧 |
| `get_note` | `notes:read` | 可視ノートの取得 |
| `create_note` | `notes:write` | ノートの作成 |
| `update_note` | `notes:write` | revisionを指定した更新 |
| `delete_note` | `notes:delete` | revisionを指定したソフトデリート |

scopeは操作の種類だけを制限し、操作できるノートの範囲を広げません。利用者は自身が作成したノートと、
ACLで直接共有されたノートだけをscopeの範囲で操作できます。

Auth0でrefresh tokenやgrantを取り消しても、すでに発行された自己完結型JWT access tokenは有効期限まで
受理される場合があります。運用上許容する最大遅延と測定方法は
[評価記録](mcp-authorization-server-evaluation.md)に従います。即時失効が必要になった場合は、
token denylistまたはtoken introspectionを別途設計します。

## MCPの通信仕様

MCP transportはJSON-RPC 2.0の`jsonrpc`、`method`、`params`、`id`を検証します。request IDは文字列
または整数だけを許可し、batchは受理しません。parse errorは`-32700`、不正なrequestは`-32600`、
不明methodは`-32601`、不正paramsは`-32602`です。tool実行時の業務エラーはJSON-RPC errorではなく
MCP tool resultの`isError: true`で返します。

初期化時は`2025-11-25`と`2025-03-26`を交渉します。以後の`MCP-Protocol-Version`が未知なら
HTTP 400で拒否します。現行transportは`MCP-Session-Id`を発行しないstateless構成です。

`create_note`または`update_note`の前に`get_note_profile`を呼び出してください。入力は題名、
`:tags:`などの文書属性、本文を含む完全なAsciiDoc文書です。詳しい入力制約と診断形式はtoolが返す
profileを正とします。
