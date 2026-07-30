# ブラウザーとMCPプロトコルの回帰テスト

## 目的

この文書は、開発者に向けて、ブラウザーでのログイン、Auth0 access token検証、
MCP Streamable HTTPの仕様を
継続して確認する方法を説明します。外部サービスのUIは自動操作せず、実際のクライアントを使った
確認結果はリリース受入へ記録します。

## 自動テスト

認証adapter、HTTP router、NixOS VMを責務ごとに分け、次を検証します。

- OIDC Authorization Code + PKCE、nonce、group claim、Web session
- Protected Resource MetadataとAuth0 issuer
- Auth0 metadata、JWKS、`RS256`、issuer、audience、期限、上流identity、group、scope
- 無効なtokenの`401`、scope不足の`403`、認証基盤障害の`503`
- MCP initialize、protocol version交渉、initialized notification、tool call
- JSON-RPC error object、batch拒否
- `create_note`と`update_note`の警告拒否、診断の重大度・位置、`isError`、`text`と
  `structuredContent`の一致

NixOS VM試験`kanidm-discovery-vm`は実Kanidm 1.10、private CA、nginx TLS、`/marginalis`
subpathを構築します。TLS越しのmetadata discoveryとlogin開始を検証し、サブパス復帰用Cookieと
Kanidmへの遷移先を確認します。OIDC nonceとstateはCookieではなくサーバー側へ保持します。

`mcp-authorization-vm`はTLS付きのfake Authorization Serverを構築し、metadataとJWKSの取得、
署名tokenによるMCP呼び出し、認証基盤停止時の起動失敗を検証します。Auth0の外部UIとtenant設定は
自動試験に含めず、この決定的な試験と人手受入を組み合わせます。

```sh
nix develop --command cargo make frontend-build
nix develop --command cargo test -p marginalis-auth-oauth
nix develop --command cargo test -p marginalis-web http::tests::mcp_transport
nix build -L .#checks.x86_64-linux.mcp-authorization-vm
nix build -L .#checks.x86_64-linux.kanidm-discovery-vm
nix develop --command cargo make protocol-regression-assets
```

## クライアントとの接続確認に使うテストデータ

Auth0のDCR、認可画面、callback互換性は外部サービスとclientの組合せに依存します。ChatGPT、
Claude Code、Codex CLIごとに実接続を確認し、client固有のrequest形状をMarginalisの一般仕様へ
取り込みません。警告を含む変更toolの回帰試験は、三つのクライアントが共通して解釈できる
標準MCP tool resultを固定し、client固有の表示文言には依存しません。

## テスト失敗時に保存する情報

失敗証跡にはrequest ID、HTTP status、遷移先のscheme・host・path、秘密情報を除いたserver log、
必要な場合の画面だけを保存します。Cookie、access token、refresh token、ID token、authorization
code、client secret、state、nonce、PKCEの値は保存しません。

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
