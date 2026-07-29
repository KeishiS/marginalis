# MCP向けAuthorization Serverの評価手順

## 目的と対象

この文書は、[Issue #24](https://github.com/KeishiS/marginalis/issues/24)で外部Authorization Serverへの
移行可否を判断するために、候補ごとに同じ接続条件と認可条件を確認した手順と、再利用できる
Auth0の設定上の注意点を記録します。最終判断は
[ADR 0001](adr/0001-auth0をmcpのauthorization-serverに採用.md)を正とします。

対象はWorkOS AuthKit、Auth0、Keycloak、現在のMarginalis内蔵実装です。候補製品の一般的な機能比較や、
Marginalis以外のシステムへの適性は扱いません。

## 評価時の原則

- 未実施の項目を成功として扱いません。
- 各候補で同じ利用者、ノート、scope、MCP操作を使用します。
- 製品の説明だけで対応済みと判断せず、対象クライアントとの通信結果を確認します。
- token、Cookie、authorization code、client secret、実際の利用者情報を成果物へ記録しません。
- 外部サービスの無料枠、機能、制限は、確認日とプラン名を添えて記録します。
- 評価用設定を本番のKanidm、Marginalis、利用者データと共有しません。

## 固定する利用者とノート

評価環境に次の利用者を用意します。`subject`は候補ごとに異なっても構いませんが、同じ役割との対応を
記録します。

| 利用者 | group | 用途 |
| --- | --- | --- |
| `user-a` | `server-users` | 自身が所有するノートの操作 |
| `user-b` | `server-users` | 他の通常利用者が所有するノートの非開示確認 |
| `former-admin` | `server-users`、`server-admins` | groupによって個別ノートのACLを迂回できないことの確認 |
| `denied-user` | なし | Marginalisを利用できないことの確認 |

`user-a`が所有する`note-a`と、`user-b`が所有する`note-b`を用意します。題名と本文には秘密情報を
含めず、評価専用と分かる固定文字列を使用します。

## 固定する認可

各クライアントについて、次の認可を別々に確認します。

| 認可 | scope | 期待する操作 |
| --- | --- | --- |
| 読み取り | `notes:read` | 一覧と取得だけ成功 |
| 読み書き | `notes:read notes:write` | 一覧、取得、作成、更新が成功 |
| 全操作 | `notes:read notes:write notes:delete` | 読み書きとソフトデリートが成功 |

scopeを持っていても所有範囲は拡張しません。`user-a`による`note-b`の取得、更新、削除は拒否し、
存在も開示しないことを確認します。ACLで`user-a`へ共有した場合だけ、付与した権限の範囲で
取得または更新に成功することを確認します。`former-admin`も通常利用者と同じ規則を適用し、
所有または共有されていないノートの存在を開示しません。

## 実接続前の自動検査

内蔵実装では、実クライアントを接続する前に次の試験を実行します。

```sh
nix develop --command cargo make protocol-regression-assets
nix develop --command cargo make frontend-build
nix develop --command cargo test -p marginalis-auth-oauth
nix develop --command cargo test -p marginalis-web http::tests::mcp_transport
```

前者はChatGPTのブラウザー送信、Claude Codeのloopback redirect URI、Codex CLIの`Origin`を
送らない通信を表す固定データを検査します。後者は動的クライアント登録、Authorization Codeと
PKCE S256、`resource`、scope、所有者認可、token更新、認可取消を本番用adapterとHTTP境界で
検査します。

これらの成功はMarginalis側の事前条件を示すだけです。対象クライアントの実際の版が同じ通信を行い、
接続と再接続に成功したことを示さないため、接続結果は「未実施」のままとします。

## クライアントごとの接続確認

ChatGPT、Claude Code、Codex CLIごとに、次の順序で確認します。

1. クライアントへMarginalisのMCP URLだけを設定します。
2. Protected Resource MetadataからAuthorization Serverを発見できることを確認します。
3. Dynamic Client RegistrationまたはClient ID Metadata Documentによって、クライアントを識別
   できることを確認します。
4. Authorization Code + PKCE S256で利用者がログインし、要求されたscopeを確認して同意します。
5. `resource`がMarginalisのMCP URLと一致し、access tokenの`audience`が同じ対象を示すことを
   Marginalis側で確認します。
6. `initialize`、`tools/list`、読み取り、書き込み、削除の順に実行します。
7. MarginalisまたはAuthorization Serverから認可を取り消し、既存のaccess tokenとrefresh tokenが
   使えなくなることを確認します。
8. 再認可後に接続を回復できることを確認します。

クライアントの版、実行環境、登録方式、redirect URIの種類を結果へ記録します。ChatGPTのように
クライアントの版を確認できない場合は、確認日と利用した画面を記録します。

## 候補ごとの確認

### WorkOS AuthKit

- stagingとproductionで利用できる機能の差
- Dynamic Client RegistrationとClient ID Metadata Documentの設定
- Resource Indicatorとaccess tokenの`aud` claim
- Kanidmをログイン元として使用する方法と費用
- 利用者、組織、roleまたはpermissionをgroupへ対応させる方法
- 認可取消と鍵の更新

### Auth0

- Dynamic Client RegistrationとResource Parameter Compatibility Profileの設定
- DCRで作成したthird-party applicationに与える既定のAPIとscope
- API identifier、`resource`、`audience`の対応
- KanidmをEnterprise Connectionとして使用する方法と費用
- Actionまたはclaim設定によるgroupの引き渡し
- 認可取消とrefresh token rotation

#### 評価環境の設定

Auth0側には、次の値を設定します。API identifierはMarginalisの公開MCP URLと完全に一致させ、
署名アルゴリズムには`RS256`を使用します。APIには`notes:read`、`notes:write`、
`notes:delete`を定義します。Resource Parameter Compatibility Profileを有効にし、MCPクライアントが
送る`resource`をAPI identifierと同じ対象へ結び付けます。Maximum Access Token Lifetimeは評価時に
`300`秒とし、Refresh Token RotationとAllow Offline Accessを有効にします。

| 設定 | 値 |
| --- | --- |
| Auth0 tenant domain | 評価用tenantのdomain |
| API identifier | `https://評価用Marginalisのホスト/mcp` |
| 上流issuer claim | API identifierと同じ管理下にある名前空間のURL |
| 上流subject claim | API identifierと同じ管理下にある名前空間のURL |
| group claim | API identifierと同じ管理下にある名前空間のURL |

DCRで作成されるapplicationはthird-party applicationとして扱われ、APIがAllow Allでも明示的な
client grantがなければaccess tokenを取得できません。APIのDefault Permissions for Third Party Appsで
User-Delegated Accessだけを有効にし、`notes:read`、`notes:write`、`notes:delete`を要求可能なscopeとして
設定します。Client Accessは有効にせず、利用者を伴わないClient Credentials Flowを許可しません。
最終的なaccess tokenのscopeが、クライアントの要求、default permission、利用者の同意の共通部分に
なることを、読み取り・読み書き・全操作の3回に分けて確認します。

評価tenantでは次も確認します。

- Tenant SettingsでDynamic Client RegistrationとResource Parameter Compatibility Profileを有効化します。
- DCRで作成されたapplicationのclient IDが`tpc_`で始まり、third-partyとして表示されることを確認します。
- public clientの`token_endpoint_auth_method`が`none`、grant typeが`authorization_code`と
  `refresh_token`、PKCEが必須であることを確認します。
- KanidmのOIDC Enterprise Connectionをdomain-level connectionへ昇格し、評価用third-party
  applicationだけでログインできることを確認します。
- DCR endpointは認証なしで登録を受け付けるため、作成数とclient IDを秘密情報なしで記録し、
  評価終了時に作成したapplicationだけを削除します。
- DCRの5 requests/second/tenantという制限を超えない通常の接続だけを行い、負荷試験は実施しません。

Kanidmは評価用のOIDC Enterprise Connectionとして接続します。claimの元は、その接続で検証した
KanidmのID tokenまたはUserInfo応答だけに限定します。Auth0の利用者が変更できる
`user_metadata`や、ほかのconnectionから同名で渡された値は使用しません。Post Login Actionでは
connectionを名前またはIDで限定し、次の値を名前空間付きcustom claimとしてaccess tokenへ設定します。

- Kanidmの`iss`を上流issuer claimへ設定します。
- Kanidmの`sub`を上流subject claimへ設定します。
- Kanidmの`groups`文字列配列をgroup claimへ設定します。

実際のtenantでOIDC claim mapping後の属性名を確認してからActionを確定します。Actionのsource、
tenant名、client ID、秘密情報はリポジトリへ保存しません。

#### DCR接続時の障害と確認順序

Auth0の設定項目は、API、Enterprise Connection、tenant、Universal Loginに分かれています。
MCPクライアントの画面に一般的な接続失敗だけが表示される場合は、次の順序で確認します。

| 症状 | 原因 | 確認と対処 |
| --- | --- | --- |
| DCR endpointがHTTP 400で登録を拒否 | tenant全体のDCRが無効 | Tenant SettingsのAdvancedでDynamic Client Registrationを有効化 |
| 同意前にAPI accessを拒否 | DCRで作成したthird-party applicationにclient grantがない | APIのDefault Permissions for Third Party AppsでUser-Delegated Accessと必要な`notes:*` scopeだけを許可 |
| Enterprise Connectionを選択できない | third-party applicationから通常のconnectionを利用できない | 対象のOIDC Enterprise Connectionをdomain-level connectionへ昇格 |
| userinfo用audienceは許可されないという認可エラー | MCPクライアントが送る`resource`をAuth0がAPI identifierとして扱っていない | Resource Parameter Compatibility Profileを有効化し、API identifierとMCP URLの完全一致を確認 |
| third-party clientではClassic Loginを利用できないというエラー | tenantがClassic Universal Loginを使用 | New Universal Loginへ切り替え、旧Login pageのHTML customizationを無効化 |
| 同意後のMCP requestがHTTP 401 | access tokenに上流identityまたはgroupのcustom claimがない、または形式が不正 | Post Login Actionのconnection条件と、値ではなく型だけを記録した診断結果を確認 |

OIDC Enterprise Connectionの`use_map`では、上流の配列claimがカンマ区切り文字列としてAuth0の
利用者属性へ保存される場合があります。Post Login Actionは、group属性が文字列なら空要素を除いて
配列へ戻し、配列としてcustom claimへ設定します。この変換は、上流のgroup名にカンマを許さないことを
前提とします。カンマを許すIdentity Providerでは区切り文字による復元を行わず、配列を保持できる
mapping modeまたは別の属性引き渡し方法を選びます。

Actionの早期終了を調べるときは、connection名とstrategy、subjectの型、groupが配列か、group数だけを
一時的に記録します。subject、group名、access tokenは記録しません。Actionを修正した後は新しい
access tokenが必要なため、MCPクライアントで再認可します。

ChatGPT Web UIは接続確認中にAccept headerが異なるrequestを送ることがあります。サーバーログで
HTTP 406の後に401が続く場合は、406だけで接続失敗と判断せず、Bearer tokenを伴うrequestの401を
調べます。401は署名、`iss`、`aud`、有効期限、上流identity、`server-users`、scopeのいずれかを
検証できなかったことを示します。秘密情報をログへ追加せず、Authorization Serverのtoken交換成功と
型だけを記録するAction診断を照合して原因を絞ります。

NixOSでは、MCP設定に次のoptionを設定します。claim名はAuth0 Actionで設定した
名前空間付きcustom claimと完全に一致させます。4項目の一部だけを設定した構成と、MCPを無効にした
構成はモジュール評価時に拒否されます。

```nix
services.marginalis.mcp = {
  enable = true;
  authorization = {
    issuer = "https://評価用tenantのdomain/";
    upstreamIssuerClaim = "https://評価用Marginalisのホスト/claims/upstream-issuer";
    upstreamSubjectClaim = "https://評価用Marginalisのホスト/claims/upstream-subject";
    groupsClaim = "https://評価用Marginalisのホスト/claims/groups";
  };
};
```

この設定を有効にすると、Protected Resource MetadataはAuth0をAuthorization Serverとして案内し、
MCP endpointはAuth0が`RS256`で署名したaccess tokenだけを受理します。Marginalisは署名に加えて
`iss`、公開MCP URLと一致する`aud`、有効期限、上流のKanidm issuer、`server-users`、
`notes:*` scopeを検証します。Auth0上の`sub`は所有者IDに使用せず、上流issuer claimと
上流subject claimの組を既存の所有者IDとして使用します。
refresh tokenの発行に必要な`offline_access`は検証後に受理しますが、ノート操作の権限には変換しません。
それ以外の未知のscopeを含むaccess tokenは拒否します。

Auth0のrefresh tokenまたはgrantを取り消しても、発行済みのJWT access tokenをMarginalisが
Auth0へ問い合わせて即時に無効化する仕組みはありません。Auth0の公式資料でも、利用者が既存tokenを
使えなくなるのは現在のaccess tokenが期限切れになった後と説明されています。Marginalisは期限検証に
5秒だけ時刻差の猶予を設けるため、300秒のtokenでは取消から拒否までの理論上限を305秒とします。
接続解除でauthorization grant全体を取り消すため、tenant設定の
`Refresh Token Revocation Deletes Grant`を有効にします。この設定はtenant内の全applicationに影響します。
実接続では次を秒単位で記録します。

1. 対象クライアントで認可を完了し、MCP読み取りに成功することを確認します。
2. 対象クライアントからRFC 7009 endpointへ最新refresh tokenの取消要求を送ります。
3. 再認可せず、同じクライアント接続から10秒ごとにMCP読み取りを実行し、最後に成功した時刻と
   最初に認証失敗となった時刻を記録します。
4. access tokenの期限後も再認可せずにMCP読み取りを実行し、refreshによって接続が回復しないことを
   確認します。
5. 再認可して接続を回復できることを確認します。

clientが保持するtoken値は取得または記録しません。この測定は、利用者が実際に経験する
「取消後も操作できる時間」を対象とします。
Auth0 Dashboardでは対象利用者のAuthorized Applicationsからauthorization grantが削除されたことも
確認します。

305秒の取消遅延を許容できない場合、Auth0を採用しません。Marginalis側にtoken denylistやAuth0の
通知処理を追加すると、外部化によって実装と保存データを減らす目的に反するため、評価adapterには
追加しません。

ChatGPT Web UIでは、DCR、Kanidmでのログイン、scope同意、MCP接続、ノートの作成・更新・削除を
実環境で確認しました。所有者identityにはAuth0固有の`sub`ではなくKanidm由来claimが使用され、
既存の所有者・ACL認可と一致することも確認しました。tenant domain、connection名、利用者情報、
tokenなどの環境固有値は記録しません。

この結果を基にAuth0を採用し、内蔵Authorization Serverを撤去しました。Claude Code、Codex CLI、
取消遅延の実測はリリース前の人手受入として残します。

Auth0の設定では、次の公式資料を参照します。

- [OpenID Connect Discovery](https://auth0.com/docs/get-started/applications/configure-applications-with-oidc-discovery)
- [JSON Web Key Sets](https://auth0.com/docs/secure/tokens/json-web-tokens/locate-json-web-key-sets)
- [Access tokenのcustom claim](https://auth0.com/docs/secure/tokens/json-web-tokens/create-custom-claims)
- [OIDC Enterprise Connectionのclaim mapping](https://auth0.com/docs/authenticate/identity-providers/enterprise-identity-providers/configure-pkce-claim-mapping-for-oidc)
- [Access tokenの有効期間](https://auth0.com/docs/secure/tokens/access-tokens/update-access-token-lifetime)
- [Refresh tokenの取消](https://auth0.com/docs/secure/tokens/refresh-tokens/revoke-refresh-tokens)
- [Authorization Code + PKCEのtoken交換](https://auth0.com/docs/api/authentication/authorization-code-flow-with-pkce/get-token-pkce)
- [Dynamic Client Registration](https://auth0.com/docs/get-started/applications/dynamic-client-registration)
- [Third-party applicationの設定](https://auth0.com/docs/get-started/applications/third-party-applications/configure-third-party-applications)
- [Resource Parameter Compatibility Profile](https://auth0.com/ai/docs/mcp/guides/resource-param-compatibility-profile)
- [Third-party applicationの障害対応](https://auth0.com/docs/get-started/applications/third-party-applications/troubleshooting)
- [OIDC attribute mappingで配列が文字列になる制約](https://support.auth0.com/center/s/article/Arrays-mapped-to-string-in-OIDC-or-Okta-Workforce-connection)

### Keycloak

- 使用したKeycloakの版と配備方法
- 匿名Dynamic Client Registrationのpolicy
- MCPクライアントが登録するredirect URIへの制限
- `resource`とaccess tokenの`aud` claimを一致させる設定
- Kanidmとのidentity brokeringまたは利用者同期
- session、consent、access token、refresh tokenの取消
- database、鍵、更新、監視、backupに増える運用負担

### Marginalis内蔵実装

- 3種類の対象クライアントによる現行DCR経路
- SQLiteに保存するclient、認可code、access token、refresh token
- token family単位のreplay検知と失効
- Kanidmのgroupをログイン時に固定する現在の動作
- 登録上限、日次削除、診断、backupの運用負担

## 結果の記録

個々の確認結果には次の状態だけを使用します。

- **成功**: 固定手順の期待結果を実際の通信で確認
- **失敗**: 実際の通信が期待結果と不一致
- **保留**: 設定、契約、障害など外部条件の解消後に再確認
- **未実施**: 通信をまだ確認していない

失敗と保留には、秘密情報を除いたHTTP status、OAuth error、クライアントの表示、再現手順を記録します。
成功には、成功と判断した操作と、サーバー側で確認した検証項目を記録します。画面の画像を残す場合は、
利用者名、メールアドレス、tenant名、client ID、ノート本文を伏せます。

## 採否

3種類の対象クライアントを接続でき、利用者、group、resource、audience、scope、失効、所有者認可を
すべて確認できた候補だけを採用対象とします。条件を満たす候補が複数ある場合は、Marginalisから削除
できるコードと保存データ、無料枠を超えた場合の費用、障害時の影響、日常の運用負担を比較します。

最終判断は`docs/adr/NNNN-短い名称.md`へ記録します。外部化を採用する場合は、削除するSQLiteテーブル、
HTTP endpoint、NixOS option、定期処理と、既存環境からの移行方法を実装前に決めます。
