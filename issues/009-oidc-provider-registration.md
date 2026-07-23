# 009: OIDCプロバイダ登録と実環境結合試験

## 目的

KanidmのclientごとのOIDC issuer `https://id.sandi05.com/oauth2/openid/marginalis` を用いた、実運用環境でのログインを有効化し、
Discovery、Authorization Code Flow with PKCE、ID Token検証およびサーバ側セッションを
結合試験する。

## 利用者または運用者が用意する情報

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

## アプリケーション側の作業

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
- `open`、`approval`および`invite-only`の各登録ポリシーで期待どおりに扱われる。
- token、secret、authorization code、state、nonce、PKCE verifierがログまたはSQLiteの
  恒久データへ露出しない。
- IdPが返す認可拒否、state不一致、期限切れ、署名不正およびtoken交換失敗が安全な共通の
  失敗応答になる。

## 2026-07-23時点の実環境確認

- `https://marginalis.sandi05.com/api/v1/health`が`200`、
  `https://marginalis.sandi05.com/api/v1/readiness`がOIDC `available`として`200`を返すことを確認した。
- ブラウザからKanidmへのredirect、認証、callbackおよびMarginalisへのsession確立を確認した。
- 次の実環境確認は、OIDC sessionを使うREST CRUD、MCP OAuth client認可、MCP tool呼出しである。
