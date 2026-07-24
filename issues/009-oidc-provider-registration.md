# 009: OIDCプロバイダー登録と実環境統合試験

状態: 初期受入完了。RESTと実MCPクライアントを含む追加のE2E試験は
[Issue 030](030-end-to-end-test-automation-readiness.md)で管理する。

## 目的

KanidmのclientごとのOIDC issuer `https://id.sandi05.com/oauth2/openid/marginalis` を用いた、実運用環境でのログインを有効化し、
Discovery、Authorization Code Flow with PKCE、ID Token検証およびサーバ側セッションを
結合試験する。

## 運用時に用意する情報

- アプリケーションのBase URLは`https://marginalis.sandi05.com`とする。
- IdPへ登録するredirect URIは
  `https://marginalis.sandi05.com/auth/oidc/callback`とする。
- Client IDは`marginalis`とする。
- IdPが発行するClient Secret。これはチャット、Git、SQLite、設定ファイルおよびログへ
  保存しない。

Client Secretはデプロイ環境でのみ次の環境変数へ設定する。

```text
OIDC_ISSUER_URL=https://id.sandi05.com/oauth2/openid/marginalis
OIDC_CLIENT_ID=marginalis
OIDC_CLIENT_SECRET=<issued-client-secret>
```

KanidmのDiscovery URLはissuerの末尾に付加する。

```text
https://id.sandi05.com/oauth2/openid/marginalis/.well-known/openid-configuration
```

## 実装要件

- 起動時にDiscoveryとJWKS取得を行い、HTTP redirectを追跡しない。
- `state`、`nonce`およびPKCE verifierを一回限り・有効期限付きで保存する。
- callbackでcodeを交換し、ID Tokenの署名、issuer、audience、期限、発行時刻およびnonceを
  検証する。
- Kanidmの`client_secret_post`を用いてtoken endpointへclient secretを送る。reverse proxyの
  `Authorization` header転送には依存しない。
- `(issuer, subject)`を内部ユーザーUUIDへ対応付ける。
- 成功時だけSecure、HttpOnly、SameSite=LaxかつBase URLのサブパスをPathとする
  サーバ側セッションCookieを発行する。
- 起動時のDiscoveryが一時的に失敗してもroot緊急ログインは維持する。この状態ではOIDC loginを安全に
  拒否し、IdP復旧後のservice再起動でDiscoveryを再試行する。

## 完了条件

- `https://id.sandi05.com/oauth2/openid/marginalis`で登録したClientからログインとログアウトができる。
- Base URLがサブパスを含む場合もredirect URIとCookie Pathが一致する。
- RC.1では`open`および`approval`の登録ポリシーで期待どおりに扱われる。`invite-only`は招待機能を
  導入するfuture releaseで検証する。
- token、secret、authorization code、state、nonce、PKCE verifierがログまたはSQLiteの
  恒久データへ露出しない。
- IdPが返す認可拒否、state不一致、期限切れ、署名不正およびtoken交換失敗が安全な共通の
  失敗応答になる。

## 実施記録（2026-07-23）

- `https://marginalis.sandi05.com/api/v1/health`が`200`、
  `https://marginalis.sandi05.com/api/v1/readiness`がOIDC `available`として`200`を返すことを確認した。
- ブラウザーからKanidmへのredirect、認証、callbackおよびMarginalisへのsession確立を確認した。
- OIDCセッションを使うREST CRUD、MCP OAuthクライアント認可およびMCPツール呼出しの
  継続的な検証は、Issue 030へ移管した。
