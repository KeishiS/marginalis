# 046: browser・MCP protocol回帰試験

## 状態

自動化範囲は実装完了。実Kanidm、Playwright、MCP test client、相互運用fixture、
秘密情報を除去した失敗証跡をrelease gateへ追加した。外部clientの結果はIssue 048へ記録する。

## 目的

Kanidm login、subpath、OAuth authorization、MCP Streamable HTTPの主要経路を自動化し、
ブラウザーや外部MCP clientの更新による回帰を公開前に検出する。実サービス固有の画面操作だけを
手動受入へ残す。

## 前提

- KanidmのgroupはOIDC login時に署名検証し、sessionとMCP authorizationの権限snapshotにする。
- MarginalisがMCP OAuth authorization serverを担当し、Kanidm tokenをMCP clientへ渡さない。
- OAuth、Protected Resource Metadata、Dynamic Client Registration、JSON-RPC、Streamable HTTPの
  公開仕様を試験の正とする。特定clientの挙動を一般仕様として実装へ混入させない。

## 作業内容

1. NixOS VMの実Kanidm 1.10、private CA、nginx、subpath構成でbrowser loginとcallbackを通す。
2. Playwrightでlogin開始、nonce Cookie、callback、同一sessionのconsent、CSRF・Origin拒否を検証する。
3. 最小の仕様準拠MCP test clientでmetadata discovery、DCR、Authorization Code + PKCE S256、
   resource indicator、token refresh、tool call、認可取消を検証する。
4. ChatGPT型のquery付き`POST /oauth/authorize`、opaque Originを持つconsent、Claude Code型の
   明示port付きloopback callbackを相互運用fixtureとして保持する。
5. JSON-RPC 2.0のrequest、notification、error object、batch拒否方針とMCP protocol version交渉を
   独立した通信仕様試験にする。
6. 失敗時artifactからCookie、token、authorization code、client secretを除去し、request ID、
   server log、画面、遷移先だけを保存する。
7. ChatGPT、Claude Code、Codex CLIの実client確認は、版と結果をrelease issueへ記録する。

## 対象外

- 外部サービスのUIをCIから自動操作すること。
- client固有の非標準挙動を無条件に許可すること。
- Kanidm groupをlogin後に定期照会すること。

## 完了条件

- Kanidm 1.10、TLS、subpath、browser、MCP test clientを通る主要経路がNixOS VMで再現できる。
- 標準仕様の試験とclient相互運用fixtureが区別されている。
- ChatGPT、Claude Code、Codex CLIに必要な互換経路が回帰試験で保護されている。
- 秘密情報を含まない失敗証跡から、request ID単位で原因を追跡できる。
