# MCPのAuthorization Serverを内蔵する

- 状態: 採用
- 日付: 2026-08-02
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
- RFC 7009のtoken失効と、利用者本人の認証を要するREST APIからの接続取消を提供します。
- Client ID Metadata Documentを優先し、外部文書の取得先、redirect、応答量を制限します。
- DCRは既存クライアントとの互換経路として提供し、登録数と要求頻度を制限します。
- 外部Authorization Serverのissuer、claim名、署名鍵の取得設定を削除します。

## 結果

配備時にAuth0を準備する必要がなくなり、認可取消をMarginalis内で即時に反映できます。一方、OAuthの安全性、
クライアント互換性、tokenの保存と削除はMarginalisの保守範囲になります。そのため、redirect URI、PKCE、
resource、scope、code再利用、refresh token再利用、登録上限、失効を自動試験で継続して確認します。

SQLite schemaは13から14へ更新します。既存Auth0のtokenとclient登録は移行せず、利用者は更新後にMCP
クライアントを接続し直します。

Authorization Serverを内蔵したことで、ADR 0005が定めた「Auth0のdiscoveryに失敗した場合は起動を
失敗させる」規則は対象を失いました。MCPの認可に外部依存がなくなるため、起動可否はKanidmの
discoveryだけで決まります。

## 代替案

**Auth0の設定を簡素化して継続する**: tenantの初期設定をTerraformなどで自動化すれば準備の手間は
減ります。しかし、発行済みJWT access tokenが失効するまでの遅延はAuth0側の設計に由来するため、
即時失効の要件を満たせません。

**KanidmのOAuth2機能をAuthorization Serverとして使う**: 認証基盤を一つに保てます。ただし、
MCPが求めるDCRとClient ID Metadata Documentの対応状況が現時点で不十分で、resource指示子の
扱いも確認できていません。Kanidm側の対応が進んだ時点で再検討します。

**内蔵と外部を設定で切り替える**: 移行期の選択肢は増えますが、認可経路が二重になり、失効の
即時性やclient登録の扱いが構成によって変わります。試験と文書の対象も倍になるため採りません。

## 再検討条件

- Client ID Metadata DocumentのInternet-Draftが更新され、取得時のcache方針や安全上の制約が変わった場合
- KanidmがDCRとClient ID Metadata Documentへ対応し、resource指示子を扱えるようになった場合
- Authorization Serverの保守負担が、Marginalis本体の開発を継続的に圧迫するようになった場合
- MCP仕様が認可方式を変更し、Authorization Code + PKCE S256以外の対応が必要になった場合
