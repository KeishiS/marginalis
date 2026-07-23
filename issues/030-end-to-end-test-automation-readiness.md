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

## 2026-07-23 事前調査と推奨案（承認待ち）

「着手前に決める事項」の五項目について、調査結果と推奨案を記す。採否の決定は運用者が行う。

### 前提の確認結果

- 現在のnixpkgs pinで実Kanidmを`kanidm_1_9`（1.9.4）または`kanidm_1_8`（1.8.6）として利用
  できる。NixOSには`services.kanidm` moduleが存在する。
- `playwright-driver`（1.61.1）と`playwright-driver.browsers`が利用でき、browser binaryを
  Nixからhermeticに供給できる。ネットワーク取得は不要である。
- 既存のrelease gateはNixOS VM test（module評価、maintenance lifecycle、実binaryの縮退起動）を
  含み、E2Eを同じ`nix flake check`系へ載せる下地がある。

### 推奨案

1. **実行基盤**: 二層構成とする。
   - 第一層: `marginalis-integration-tests` crate（Issue 021の項目3）で、browserなしの
     REST/MCP契約をin-process Axum＋OIDC mockに対して高速に検証する。
   - 第二層: NixOS VM testを主基盤とし、server VM（Marginalis＋nginx＋Kanidm）とclient VM
     （Playwright＋headless Chromium）で実経路を通す。GitHub Actionsは既存release gateと同じ
     `nix flake check`起動とし、browser用の別jobを設けない。
2. **テスト用OIDC**: mock IdPを新規に選定せず、**実KanidmをVM内で使う**。本番IdPと同一実装で
   Discovery・JWKS・`approval` policyまで再現でき、忠実度が最も高い。test user・client secret
   はtestScript内で毎回生成し、リポジトリとCI secretへ置かない。実Kanidm「本番インスタンス」を
   使う確認だけを手動受入に残す。
3. **reverse proxyの再現範囲**: VM内nginxでTLS終端（テスト用自己署名CA）し、`/auth/`・
   `/api/`・`/mcp`・`/.well-known/`・`/oauth/`の転送とsecurity header保持を検証する。subpath
   構成は独立した1シナリオとして持つ。proxy側rate limitはdocs/nixos.mdの設定例をそのまま
   VMへ適用して検証し、limiter単体の境界値はunit testの責務とする。
4. **MCP自動client**: 外部SDKへ依存せず、integration-tests crate内に**最小のRust製test client**
   （Streamable HTTP＋Authorization Code＋PKCE S256＋loopback redirect受け）を実装する。公開
   契約だけを使い、外部runtime（Node等）をrelease gateへ持ち込まない。ChatGPT等の実機連携は
   手動受入に残す。
5. **失敗時artifact**: 失敗時にPlaywright trace・screenshot・`journalctl -u marginalis`・
   `X-Request-Id`対応表を取得し、GitHub Actions artifactとして14日保持する。Cookie・token・
   authorization code・client secretはtestScriptで生成した使い捨て値のみであり、実secretは
   一切CIへ入れない。念のためartifact収集前に`Set-Cookie`値と`code=`パラメータをmaskする。

### 運用者の決定が必要な点

- 上記1〜5の採否（特に、mock IdPではなく実KanidmをVMで使う方針）。
- VM E2Eをrelease gate必須にするか、別workflow（nightly等）から始めるか。VM＋browserは
  実行時間が長く、まず非必須で安定させてから必須化する段階導入を推奨する。
- Kanidm versionの追従方針（`kanidm_1_9`固定か、nixpkgs更新へ追従か）。本番Kanidmの
  versionと合わせることを推奨する。
