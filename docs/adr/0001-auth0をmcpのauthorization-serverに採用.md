# Auth0をMCPのAuthorization Serverに採用

- 状態: 採用
- 日付: 2026-07-29

## 背景

MarginalisはMCPのAuthorization ServerとProtected Resourceを一つのserviceで提供していました。
この構成では、DCR、認可画面、PKCE、token発行、refresh token rotation、取消、永続化、定期削除を
アプリケーション側で保守する必要があります。MCPクライアントごとの互換性とOAuthの安全性を、
ノート管理機能と同じ開発単位で継続して検証する負担もあります。

KanidmはWeb UIの利用者認証と利用可否を決める正本です。MCPでもKanidmの
`(issuer, subject)`をノート所有者identityとして維持し、`server-users`所属を検証する必要があります。

Issue #24では、内蔵実装、Auth0、Keycloak、WorkOS AuthKitを比較しました。Auth0とKanidmを接続した
実環境で、ChatGPT Web UIのDCR、ログイン、scope同意、ノートの作成・更新・削除、所有者とACLの一貫性を
確認しました。

## 決定

Auth0をMCPのAuthorization Serverに採用し、MarginalisはProtected Resourceだけを提供します。

- クライアント登録、ログイン、同意、token発行、refresh token、grant取消はAuth0の責務
- 利用者認証と`server-users`所属の正本はKanidm
- Auth0の署名、issuer、MCP URLのaudience、scope、上流identity claim、group claimの検証はMarginalisの責務
- Protected Resource MetadataによるAuth0 issuerの案内
- Auth0固有の`sub`ではなく、Kanidmの上流`issuer`と`subject`による所有者identity
- 起動時のmetadata・JWKS取得失敗時の起動停止
- 無効なtokenの`401`、scope不足の`403`、認証基盤障害の`503`
- token、claim値、利用者identityを含めない構造化ログ

内蔵Authorization ServerのHTTP endpoint、application use case、SQLiteテーブル、定期削除処理、
認可取消APIは削除します。後方互換は提供せず、SQLite schema versionを更新します。

## 結果

OAuthプロトコルとcredential lifecycleの保守範囲がAuth0へ移ります。Marginalisはノート認可と
Resource Serverとしてのtoken検証に集中できます。一方、Auth0 tenant、API、DCR、Enterprise
Connection、Login Action、Universal Loginの設定と可用性が運用上の依存になります。

Auth0でgrantやrefresh tokenを取り消しても、発行済みJWT access tokenは有効期限まで利用できる場合が
あります。最大遅延を受入試験で測定し、許容値を運用判断として記録します。
接続解除ではrefresh tokenだけを残さず、同じ利用者・application・API audienceのauthorization grantも
削除するため、tenant設定の`Refresh Token Revocation Deletes Grant`を有効にします。この設定が
tenant内の全applicationへ影響することを運用上の前提とします。

## 代替案

### 内蔵Authorization Server

外部serviceへの依存は増えませんが、OAuthの安全性、相互運用、永続状態、取消を継続して自前で
保守する必要があるため採用しません。

### Keycloak

self-hostでき、DCRにも対応します。一方、評価時点ではMCPが要求するRFC 8707 `resource`への対応が
不足し、運用するdatabase、鍵、更新、監視、バックアップも増えるため採用しません。

### WorkOS AuthKit

DCRとMCP向け機能を提供しますが、任意scope、Kanidm由来claim、取消動作、費用の確認が不足したため
採用しません。

## 再検討条件

- Auth0が必要なDCR、RFC 8707、scope、Enterprise Connectionを提供しなくなった場合
- 発行済みaccess tokenの取消遅延を運用上許容できなくなった場合
- データ所在地、費用、可用性または利用規約が要件を満たさなくなった場合
- 受入対象のMCPクライアントがAuth0の登録・認可方式と互換でなくなった場合
