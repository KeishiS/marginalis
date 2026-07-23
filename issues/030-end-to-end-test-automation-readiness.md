# 030: E2Eテスト自動化の準備と実装

状態: RC.1 リリース後、早期に着手。

## 目的

現在のunit、adapter contract、NixOS VM testおよび手動受入を補完し、実際のbrowser、reverse proxy、
OIDC providerおよびMCP clientを通すE2Eテストを反復可能に自動化する。

実装を始める前に、CIで安全かつ再現可能に動かすための前提を調査・決定する。この調査を飛ばして
browser automationや実IdP依存を追加しない。

## 着手前に決める事項

E2E実装を開始する前に、次の五項目を決定し、このIssueの実施結果へ記録する。

1. **実行基盤**: GitHub ActionsのUbuntu runner、Nix、Playwrightを主基盤とするか。browserとNixOS VMを
   同一workflowで動かすか、責務別jobへ分けるか。
2. **テスト用OIDC**: CI内のtest IdPを使い、実Kanidmは手動受入に残すか。test user、client、secretの
   生成・注入・cleanup方法をどうするか。
3. **reverse proxyの再現範囲**: TLS終端、subpath、`/auth/`、`/api/`、`/mcp`、`/.well-known/`、`/oauth/`を
   CIでどこまで通すか。proxy側rate limitをE2Eで検証するか。
4. **MCP自動client**: Authorization Code + PKCEとStreamable HTTPを実行するclient実装・libraryを何にするか。
   ChatGPT実機連携は手動受入として残すか。
5. **失敗時artifactと保持**: browser trace、screenshot、server log、request IDの取得・保持期間を決める。
   Cookie、token、authorization code、client secretをartifact・logへ出さないmasking方針を固定する。

## 事前調査・準備

1. **実行基盤**: Playwright等のbrowser driver、headless browser、Nix devShell/CI runner、NixOS VM testの
   どれを主基盤にするか比較する。Linux CIでのbrowser binary、sandbox、画面記録・trace・screenshotの
   保存方針と失敗時の取得物を決める。
2. **OIDC test provider**: 実Kanidmを使わず、Authorization Code + PKCE、Discovery、JWKS、callback、
   `approval` policyを再現できるtest IdPを選定する。test user、client secret、password、Cookie、tokenを
   CI log・artifact・repositoryへ出さない注入とcleanup方法を定義する。
3. **network topology**: TLS終端を含むreverse proxy、Base URLのsubpath、`/auth/`、`/api/`、`/mcp`、
   `/.well-known/`、`/oauth/`の経路を、CI内でどこまで再現するか決める。trusted headerを用いないroot
   login rate limitの検証責務をproxy/E2E/unitのどこへ置くか明確にする。
4. **MCP client**: Streamable HTTPとOAuth Authorization Code + PKCEを実装する自動clientを選定する。
   Protected Resource Metadata、authorization、tool呼出し、認可取消後のtoken失効を非対話で検証する方法を
   確立する。
5. **fixtureと隔離**: UUIDv7、clock、dataDir、SQLite、OIDC identity、MCP client registration、backup
   storageをtestごとに隔離する。並列実行、失敗後cleanup、再実行、backup/restore候補の検証方法を定義する。
6. **受入との分担**: [実環境受入確認](../docs/acceptance.md)から自動化できる項目と、実Kanidm・本番proxy・
   永続backup storageを使う手動確認に残す項目を表で固定する。

## 実装候補シナリオ

1. OIDC login、`approval`によるpending作成、root承認、通常session取得、logout。
2. RESTの作成・更新・ETag競合・検索・ACL非漏洩・確認付き物理削除。
3. MCP OAuth、REST/MCPの可視性一致、`search_notes`、認可取消後のaccess/refresh token失効。
4. subpath、reverse proxy、CSRF/Origin/Fetch Metadata、OIDC login CSRF失敗経路。
5. backup generation、非破壊restore候補、projection再構築、maintenance timer、実行中OpenAPI contract。

## 完了条件

- 実行基盤、test IdP、MCP client、network topology、secret/artifact policyが文書化され、CIで再現可能である。
- 主要シナリオが独立・並列実行可能なE2E suiteとして実装される。
- 失敗時のtrace、server log、request IDおよび必要最小限の非秘密artifactを取得できる。
- 自動E2Eと手動実環境受入の責務分担が`docs/acceptance.md`とrelease gateに反映される。
