# MCP向けAuthorization Serverの評価手順

## 目的と対象

この文書は、[Issue #24](https://github.com/KeishiS/marginalis/issues/24)で外部Authorization Serverへの
移行可否を判断するために、候補ごとに同じ接続条件と認可条件を確認する手順を定めます。評価結果と
最終判断はこの文書ではなく、Issue #24と承認済みのADRへ記録します。

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
nix develop --command cargo test -p marginalis-integration-tests --test oauth_flow --all-features
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
