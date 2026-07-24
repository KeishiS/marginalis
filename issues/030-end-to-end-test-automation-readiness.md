# 030: E2Eテストの自動化

## 状態

実装中。第一段階のプロセス内結合試験は2026-07-23に完了した。第二段階のNixOS VM、
実Kanidm、Playwrightを使う試験は、KVMを利用できる環境での実装と検証が必要である。
このIssueはv0.1.0公開後の最優先作業とする。

## 目的

単体試験、連携層の契約試験、NixOS VM試験、手動受入確認を補完する。ブラウザー、
リバースプロキシ、OIDCプロバイダー、MCPクライアントを通る操作を反復可能なE2E試験にする。

CIで安全かつ再現可能に実行するため、実行基盤、秘密情報の扱い、失敗時に保存する情報を
先に決定する。

## 調査した事項

2026-07-23に、次の五項目を調査して方針を決定した。結論は「決定事項」に記す。

1. **実行基盤**: GitHub Actions、Nix、Playwrightの役割と、ブラウザー試験とNixOS VM試験の分け方。
2. **テスト用OIDC**: CI内で使うIdPと、テスト用利用者、クライアント、秘密情報の生成・破棄方法。
3. **reverse proxyの再現範囲**: TLS終端、subpath、`/auth/`、`/api/`、`/mcp`、`/.well-known/`、`/oauth/`を
   CIでどこまで通すか。proxy側rate limitをE2Eで検証するか。
4. **MCP自動クライアント**: Authorization Code + PKCEとStreamable HTTPを実行する実装。
   ChatGPTとの接続確認を手動受入に残すか。
5. **失敗時の記録**: ブラウザーのtrace、画面、サーバーログ、request IDの保存期間。
   Cookie、トークン、認可コード、クライアント秘密情報を記録しない方法。

## 実装範囲

1. **実行基盤**: Playwrightなどのブラウザードライバー、ヘッドレスブラウザー、Nix開発環境、
   CI、NixOS VM試験を比較する。Linux CIでの実行ファイル、sandbox、trace、画面の保存方法を決める。
2. **OIDCプロバイダー**: Authorization Code + PKCE、Discovery、JWKS、callback、
   `approval`規則を再現できるIdPを選ぶ。テスト用利用者、秘密情報、Cookie、トークンを
   CIログ、成果物、リポジトリへ出さない生成・破棄方法を定義する。
3. **ネットワーク構成**: TLS終端を含むリバースプロキシ、ベースURLのサブパス、`/auth/`、`/api/`、`/mcp`、
   `/.well-known/`、`/oauth/`の経路を、CI内でどこまで再現するか決める。trusted headerを用いないroot
   login rate limitの検証責務をproxy/E2E/unitのどこへ置くか明確にする。
4. **MCPクライアント**: Streamable HTTPとOAuth Authorization Code + PKCEを実装する自動クライアントを選ぶ。
   Protected Resource Metadata、authorization、tool呼出し、認可取消後のtoken失効を非対話で検証する方法を
   確立する。
5. **テストデータと隔離**: UUIDv7、時刻、`dataDir`、SQLite、OIDC利用者、
   MCPクライアント登録、バックアップ先を試験ごとに隔離する。並列実行、失敗後の後始末、
   再実行、復元候補の検証方法を定義する。
6. **受入との分担**: [実環境受入確認](../docs/acceptance.md)から自動化できる項目と、実Kanidm・本番proxy・
   永続backup storageを使う手動確認に残す項目を表で固定する。

## E2Eシナリオ

1. OIDC login、`approval`によるpending作成、root承認、通常session取得、logout。
2. RESTの作成・更新・ETag競合・検索・非公開情報を漏らさない・確認付き物理削除。
3. MCP OAuth、REST/MCPの可視性一致、`search_notes`、認可取消後のaccess/refresh token失効。
4. subpath、reverse proxy、CSRF/Origin/Fetch Metadata、OIDC login CSRF失敗経路。
5. バックアップ作成、非破壊の復元候補、検索用データの再構築、保守タイマー、
   実行中サーバーのOpenAPI仕様。

## 完了条件

- 実行基盤、test IdP、MCP client、network topology、secret/artifact policyが文書化され、CIで再現可能である。
- 主要シナリオが独立・並列実行可能なE2E suiteとして実装される。
- 失敗時のtrace、server log、request IDおよび必要最小限の非秘密artifactを取得できる。
- 自動E2Eと手動受入確認の分担が`docs/acceptance.md`とリリース前の必須検証に反映される。

## 決定事項（2026-07-23）

調査結果に基づいて次の方針を採用した。実装上の問題が判明した場合は、この節を更新する。

### 前提の確認結果

- 現在のnixpkgs pinで実Kanidmを`kanidm_1_9`（1.9.4）または`kanidm_1_8`（1.8.6）として利用
  できる。NixOSには`services.kanidm` moduleが存在する。
- `playwright-driver`（1.61.1）と`playwright-driver.browsers`を利用できる。
  ブラウザー実行ファイルはNixから再現可能に供給でき、実行時のネットワーク取得は不要である。
- 既存のリリース前検証にはNixOS VM試験が含まれる。E2Eも同じ`nix flake check`から実行できる。

### 決定した方針

1. **実行基盤**: 二層構成とする。
   - 第一層: `marginalis-integration-tests`クレート（Issue 021の項目3）で、
     ブラウザーを使わないREST・MCP試験をプロセス内のAxumとOIDC mockに対して実行する。
   - 第二層: NixOS VM試験を主基盤とする。サーバーVM（Marginalis、nginx、Kanidm）と
     クライアントVM（Playwright、ヘッドレスChromium）で実経路を通す。GitHub Actionsは
     `nix flake check`から実行し、ブラウザー専用の別jobは設けない。
2. **テスト用OIDC**: mock IdPを新規に選定せず、**実KanidmをVM内で使う**。本番IdPと同一実装で
   Discovery・JWKS・`approval` policyまで再現でき、忠実度が最も高い。test user・client secret
   はtestScript内で毎回生成し、リポジトリとCI secretへ置かない。実Kanidm「本番インスタンス」を
   使う確認だけを手動受入に残す。
3. **reverse proxyの再現範囲**: VM内nginxでTLS終端（テスト用自己署名CA）し、`/auth/`・
   `/api/`・`/mcp`・`/.well-known/`・`/oauth/`の転送とsecurity header保持を検証する。subpath
   構成は独立した1シナリオとして持つ。proxy側rate limitはdocs/nixos.mdの設定例をそのまま
   VMへ適用して検証し、limiter単体の境界値はunit testの責務とする。
4. **MCP自動client**: 外部SDKへ依存せず、integration-tests crate内に**最小のRust製test client**
   （Streamable HTTP＋Authorization Code＋PKCE S256＋loopback redirect受け）を実装する。
   公開仕様だけを使い、外部の実行環境（Nodeなど）をリリース前の必須検証へ持ち込まない。
   ChatGPTなどとの接続確認は手動受入に残す。
5. **失敗時artifact**: 失敗時にPlaywright trace・screenshot・`journalctl -u marginalis`・
   `X-Request-Id`対応表を取得し、GitHub Actions artifactとして14日保持する。Cookie・token・
   authorization code・client secretはtestScriptで生成した使い捨て値のみであり、実secretは
   一切CIへ入れない。念のためartifact収集前に`Set-Cookie`値と`code=`パラメータをmaskする。

## 実施結果

### 完了: プロセス内結合試験（2026-07-23）

- `marginalis-integration-tests` crateを追加した（Issue 021 項目3の基盤を兼ねる）。
- 試験用のHS256 OIDC provider（Discovery・JWKS・token endpointを実HTTP listenerで提供）を
  実装し、`openidconnect`による実HTTPのDiscovery・code交換・PKCE検証・ID token検証を通した。
- 実装済みのシナリオ:
  - approval policyでの初回login→pending作成、rootによる承認、再loginでのsession取得、
    stateの一回性、保護metadataのserver置換、ノート作成・取得・検索（シナリオ1と2の骨格）。
  - `If-Match`による更新成功、旧revisionの`409`、削除準備→確認tokenによる物理削除、削除後の
    `404`と検索からの消失（シナリオ2）。
  - 二利用者間の非公開情報を漏らさない: 他者の非公開ノートはsource取得`404`かつ検索結果に現れない
    （シナリオ2）。
  - MCP OAuth（Authorization Code + PKCE）のHTTP flowと、REST/MCPの検索可視性一致
    （シナリオ3）。
  - CSRF token・Origin・Fetch Metadataの欠落・不一致がすべて`403`となる失敗経路
    （シナリオ4のapplication側）。
- 未実装の範囲は、サブパス・リバースプロキシ・TLSを通す経路、バックアップと復元、
  認可取消後のトークン失効を確認するHTTP試験である。

### 未完了: NixOS VMによるE2E試験

NixOS VM、実Kanidm、Playwrightを使う試験にはKVMが必要である。現在の開発環境では
KVMを利用できないため、対応する環境で実装・検証する。

### 付随する決定

- VM E2Eは、まずリリース前の任意検証として安定させ、その後に必須検証へ昇格する。
- Kanidm versionは本番と同系列の`kanidm_1_9`へ固定し、本番のversion更新に追従して上げる。
- 実装順序: 第一層（`marginalis-integration-tests` crateのin-process試験）を先に整備し、
  第二層（NixOS VM＋Playwright）はKVMを利用できる環境で検証しながら追加する。
