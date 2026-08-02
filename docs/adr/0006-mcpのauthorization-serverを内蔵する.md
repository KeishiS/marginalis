# MCPのAuthorization Serverを内蔵する

- 状態: 採用
- 決定日: 2026-08-02
- 関連Issue: #235
- 置換対象: [ADR 0001](0001-auth0をmcpのauthorization-serverに採用.md)

## 背景

ADR 0001では、OAuthプロトコルの実装範囲を減らすためにAuth0を採用しました。しかし実運用では、
Auth0 tenant、API、Enterprise Connection、Action、claim mapping、DCRを別々に設定する必要があり、
Marginalisの配備より認可基盤の準備が複雑になりました。また、Auth0でgrantを取り消してから発行済みJWT
access tokenが無効になるまで遅延があり、Marginalisだけでは即時失効を保証できませんでした。

過去の内蔵実装には、Authorization Code、PKCE S256、DCR、resourceとscopeの検査、不透明token、
refresh token rotation、再利用時のtoken family失効、SQLiteへのhash保存がありました。この実装を現行の
MCP transportと運用要件へ合わせて再導入できることを確認しました。

## 決定

MarginalisはMCP用Authorization Serverを内蔵し、外部Authorization Serverとの切替機能を提供しません。
利用者の認証は既存のKanidm OIDCへ委ね、認可code、MCP client、access token、refresh token、grantの失効は
Marginalisが管理します。

- Authorization CodeとPKCE S256だけを許可します。
- access tokenとrefresh tokenは不透明な値とし、SQLiteにはSHA-256 hashだけを保存します。
- refresh tokenを交換し、再利用を検出した場合は同じtoken familyを失効します。
- RFC 7009のtoken失効とWeb UIからの接続取消を提供します。
- Client ID Metadata Documentを優先し、外部文書の取得先、redirect、応答量を制限します。
- DCRは既存クライアントとの互換経路として提供し、登録数と要求頻度を制限します。
- 外部Authorization Serverのissuer、claim名、署名鍵の取得設定を削除します。

## 影響

配備時にAuth0を準備する必要がなくなり、認可取消をMarginalis内で即時に反映できます。一方、OAuthの安全性、
クライアント互換性、tokenの保存と削除はMarginalisの保守範囲になります。そのため、redirect URI、PKCE、
resource、scope、code再利用、refresh token再利用、登録上限、失効を自動試験で継続して確認します。

SQLite schemaは13から14へ更新します。既存Auth0のtokenとclient登録は移行せず、利用者は更新後にMCP
クライアントを接続し直します。

## 今後の見直し

Client ID Metadata DocumentのInternet-Draft更新を追跡し、取得時のcache方針と安全上の制約を見直します。
Authorization Serverの実装負担が継続的に過大となった場合は、Kanidmが提供するOAuth機能への統合も
比較します。ただし、外部製品固有のclaim変換を再導入する方式は既定案にしません。
