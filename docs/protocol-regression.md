# ブラウザーとMCPプロトコルの回帰テスト

## 目的

この文書は、開発者に向けて、ブラウザーでのログイン、OAuth認可、MCP Streamable HTTPの仕様を
継続して確認する方法を説明します。外部サービスのUIは自動操作せず、実際のクライアントを使った
確認結果はリリース受入へ記録します。

## 自動テスト

`oauth_flow`は本番adapterとAxum routerのHTTP境界を通し、次を検証します。

- OIDC Authorization Code + PKCE、nonce、group認可、Web session
- Protected Resource Metadata、Authorization Server Metadata、DCR
- OAuth Authorization Code + PKCE S256、resource indicator、正常なtoken refreshとrotation
- consentのCSRF・Origin検証、認可取消後のaccess tokenとrefresh tokenの拒否
- MCP initialize、protocol version交渉、initialized notification、tool call
- JSON-RPC error object、batch拒否

NixOS VM試験`kanidm-discovery-vm`は実Kanidm 1.10、private CA、nginx TLS、`/marginalis`
subpathを構築します。TLS越しのmetadata discoveryとlogin開始を検証し、サブパス復帰用Cookieと
Kanidmへの遷移先を確認します。OIDC nonceとstateはCookieではなくサーバー側へ保持します。callbackの
正常経路は決定的なmock IdPを使う`oauth_flow`で検証し、実Kanidmの対話UI変更と認証要素に
依存させません。

```sh
nix develop --command cargo test -p marginalis-integration-tests --test oauth_flow
nix build -L .#checks.x86_64-linux.kanidm-discovery-vm
nix develop --command cargo make protocol-regression-assets
```

## クライアントとの接続確認に使うテストデータ

[client-compatibility.json](../crates/marginalis-integration-tests/fixtures/client-compatibility.json)
は標準仕様の試験データではなく、client相互運用fixtureです。

- ChatGPT型のquery付き`POST /oauth/authorize`
- opaque Originを持つconsent
- Claude Code型の明示port付きloopback callback
- Codex CLI型のOriginを送らないnative client

fixtureは観測したrequest形状だけを固定し、client固有の非標準挙動を一般仕様として許可しません。

## テスト失敗時に保存する情報

失敗証跡にはrequest ID、HTTP status、遷移先のscheme・host・path、秘密情報を除いたserver log、
必要な場合の画面だけを保存します。Cookie、access token、refresh token、ID token、authorization
code、client secretは保存しません。

生ログを保存する前に、次のようにsanitizeと漏洩検査を実行します。

```sh
bash .github/scripts/protocol-artifact.sh sanitize raw.log artifact.log
bash .github/scripts/protocol-artifact.sh check artifact.log
```

URLのqueryは原則として保存しません。調査に必要な場合も、`code`などをsanitizeした後に保存します。
artifactは公開CIへ無期限に保持せず、リリース調査に必要な期間だけアクセス制限付きで保持します。

## 実際のクライアントを使う確認

リリース前にChatGPT、Claude Code、Codex CLIでmetadata discovery、認可、refresh、tool call、
認可取消を確認します。[リリース受入](acceptance.md)には必須項目の完了日と成否だけを記録し、
環境やclientの詳細、秘密情報、認可URL全体は記録しません。
