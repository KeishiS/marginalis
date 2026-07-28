# 変更履歴

この文書には利用者に影響する変更だけを記録する。
公開 API、データフォーマット、NixOSモジュールの動作を変えない内部的な再構成は記載しない。

## 未リリース

## 0.8.0 — 2026-07-28

### 破壊的変更

- SQLite schemaを9、archiveを`marginalis-archive-7`、note profileを3へ更新した。v0.7.0が
  作成したschema 6のdatabaseとarchive 6は自動移行せず、更新時はデータを退避して空の
  `dataDir`から初期化する。
- サーバー全体の管理者権限を廃止した。`server-users`は利用可否だけを決め、所属グループによって
  個別ノートのACLを迂回する経路は設けない。
- REST APIを`/api/v3`へ更新した。変更操作の期待revisionはJSON本文ではなく、取得応答の
  `ETag`を指定する`If-Match`ヘッダーで受け取る。
- ノートの作成・更新入力を`title`、`body`、`tags`から、一つの完全なAsciiDoc文書を表す
  `source`へ変更した。題名とタグは文書ヘッダーから導出し、`:sectnums:`など許可した表示属性を
  文書ヘッダーで使用できる。

### 追加

- 一覧、閲覧、編集、共有設定を一つのReactアプリケーションへ統合した。REST型、実行時検査、
  クライアント関数はOpenAPIとMCPツール定義と同じ契約crateから生成する。
- コードブロックへ言語名、背景、余白、横スクロールを追加し、固定したMathJaxでLaTeX数式を
  閲覧画面とプレビューへ組版する。

### 変更

- domain型の不変条件とapplication境界を整理し、HTTP、SQLite、AsciiDoc、OIDCを用途別の
  portから利用する構成へ変更した。旧`marginalis-server` crateは削除した。
- ACL判定、revision確認、削除状態をSQLiteの同一transactionへ拘束し、一覧、詳細、関連ノート、
  更新、削除、復元、共有設定へ同じ認可決定表を適用した。
- Web UIの題名、本文、タグの入力欄を一つのAsciiDoc文書編集欄へ統合した。競合時も完全な文書を
  行単位で比較する。
- 識別子、所有者、時刻、revision、ACL、削除状態をAsciiDoc文書から分離し、利用者が
  サーバー管理属性を記述した場合は位置付きの入力エラーを返す。
- 要件ID、検証階層、版別受入結果を対応づけ、OpenAPI、生成物、文書、要件対応表を独立して
  検査するリリースゲートを追加した。

## 0.7.0 — 2026-07-28

### 破壊的変更

- SQLite schemaを6へ、archiveを`marginalis-archive-6`へ、note profileを2へ更新した。
  以前のdatabaseとarchiveは自動移行せず、更新時は既存データを退避して空の`dataDir`から
  初期化する。
- ノートの所有者モデルへ`issuer`と`subject`を組み合わせたACLを追加した。`read`は閲覧、
  `edit`は閲覧と内容更新を許可し、ACL管理と削除・復元は所有者だけが実行できる。

### 追加

- ReactとTypeScriptによるWeb UIを追加し、ノートの作成、編集、安全な保存前プレビュー、
  入力診断、明示的な保存をブラウザーから行えるようにした。
- revision競合時に編集開始時点、編集中、現在保存済みの内容を比較し、最新revisionを取得して
  修正後に再保存できる画面を追加した。
- ノート参照を保存時に索引化し、現在の利用者に見える直接参照元・参照先を閲覧画面へ追加した。
- 所有者、閲覧者、編集者、対象外利用者の権限境界を実Kanidm環境で確認する
  NixOSブラウザー試験を追加した。

### セキュリティ

- REST、MCP、Web UI、参照解決へ同じACL判定を適用し、権限のないノートと関連情報を
  `not_found`として扱うようにした。
- ACL更新へ同一オリジン、CSRFトークン、revision確認を適用し、不正、重複、所有者自身の
  ACL対象には位置付きの入力診断を返すようにした。
- ノートとACLを同じSQLite読み取りtransactionからアーカイブへ書き出し、整合しない時点の
  snapshotが生成されないようにした。

## 0.6.0 — 2026-07-27

### 変更

- AdocWeaveを0.11.0へ更新し、解析規則、問題の報告方法、執筆時URL、描画時URL、
  HTML出力上限のそれぞれに専用の公開設定を設けた。
- archiveとOpenAPIが記録するAdocWeave package版を0.11.0へ更新した。保存規則は変わらないため、
  SQLite schemaとnote profile版は維持し、復元互換性を明示するarchive形式はv4へ更新した。
- v0.5.0のSQLite schema 4 databaseは`dataDir`を保持したまま更新できる。AdocWeave 0.10.1の
  archiveはv0.6.0へ復元できないため、更新後に0.11.0のarchiveを新しく作成する必要がある。

### 修正

- 所有者identityの長さ、issuer URL、制御文字を一つのdomain規則で検証し、
  不正なarchiveからAsciiDocの管理属性を注入できないようにした。

## 0.5.0 — 2026-07-27

### 破壊的変更

- 利用経路のないノート単位ACLを廃止し、通常利用者は自身が作成したノートだけを操作できる所有者モデルへ単純化した。
  `server-admins`は従来どおりすべてのノートを管理できる。
- SQLite schemaを4へ、archiveを`marginalis-archive-3`へ更新した。
  旧databaseと旧archiveの移行は提供せず、更新時は旧`dataDir`全体を削除して再初期化する。

### 修正

- REST、MCP、Web UIで所有者認可を共通化し、権限のないノートを一覧から除外して個別操作を`not_found`として扱うようにした。
- archiveからACL bundleを削除し、ノートと所有者情報を直接保存する形式へ変更した。

## 0.4.0 — 2026-07-27

### 破壊的変更

- SQLite schemaを3へ、archiveを`marginalis-archive-2`へ更新した。
  旧schemaと旧archiveの自動移行は提供せず、空のdatabaseから再初期化する。
- ノート本文の解析規則をAdocWeave 0.10.1のStrict modeへ更新した。
  従来は保存できた本文でも、現行profileに適合しない場合は拒否する。

### 追加

- RESTとMCPで共通して使える入力検査の結果を追加した。問題の種類をプログラムで判別するcode、
  問題のあるfield、UTF-8 byte単位の位置を結果に含める。
- MCPに`get_note_profile`を追加し、入力上限、正規化、許可構文、許可言語、禁止規則、
  有効な本文例を、MCPクライアントが処理できる形式で公開した。
- AdocWeave 0.10.1が文書の解析とHTML生成で見つけた問題を、保存時と表示時に検査するようにした。

### 修正

- 保存時と表示時の構文modeを統一し、保存済みノートが後から描画不能になるケースを修正した。
- archiveの全階層において、定義外のfieldを拒否し形式の誤認を防ぐようにした。

## 0.3.1 — 2026-07-27

### 追加

- archiveの構造と、書き出し・復元したデータの一致を検証するコマンド、隔離した空の
  SQLite databaseへの復元検証、
  最新成功backupの検証、安全な世代整理を追加した。
- NixOS moduleに日次backup、直近30世代の保持、四半期の復元検証timer、読み取り専用の
  `marginalis diagnose`を追加した。
- Kanidm 1.10、private CA、nginxのsubpath、実ブラウザー、Dynamic Client Registration、
  Authorization Code + PKCE、token rotation、MCP初期化、認可取消を通すNixOS VM試験を追加した。
- backup、復元、purge、OIDC discovery、MCP OAuthをjournaldで継続して追跡できるよう、
  変更されないevent名を追加した。

### セキュリティ

- MCP承認フォームでは不透明Originとの互換性を維持しつつ、
  異なる具体的Originからの送信を拒否するようにした。
- protocol回帰テストの失敗出力からCookie、Bearer、認可コード、OAuth token、CSRF token、
  client secret、PKCE verifierを除去してから保存・表示するテストを追加した。

## 0.3.0 — 2026-07-25

SQLiteを単一の正本とし、Kanidmの署名済みOIDC group claim、閲覧用Web UI、REST API、
OAuth 2.1 で保護した MCP endpoint を一つの認可モデルへ統合する。

### 破壊的変更

- `v0.2.x` の database、ファイル正本、`/api/v1`、ローカル root、MCP token は移行しない。
  空の SQLite database から初期化する。
- 開発中の旧schema version 1も自動移行せず、schema version 2の空のdatabaseから再初期化する。
  Dynamic Client Registrationと利用者のMCP認可をやり直す。
- 公開 REST API を `/api/v2` とし、OpenAPI 正本を `docs/openapi.json` に置く。
- Kanidm の所属定期照会と service account を廃止し、OIDC login 時に検証した `groups` claim を
  Web session と MCP authorization の有効期間中の権限 snapshot とする。

### 変更

- ノート、ACL、ソフトデリート状態を SQLite transaction で更新し、30 日後の日次 purge を提供する。
- ノート単位の AsciiDoc export と、ACL・削除状態を含む JSON archive の import/export を提供する。
- Authorization Code + PKCE S256、refresh token rotation・family replay 失効、Dynamic Client
  Registration、認可取消を備えた MCP Streamable HTTP endpoint を提供する。
- NixOS module に OIDC credential、MCP Origin allowlist、backup 保存先、purge timer を集約する。

### セキュリティ

- PKCEの検証に成功した場合だけauthorization codeを使用済みにし、この二つの処理を一つの
  トランザクションで行う。
- 使用済みauthorization codeの再提示時に、発行済みtoken family全体を失効させる。
- 使用済み refresh token の再提示時に token family 全体を失効させる。
- token endpointで未対応のHTTP client認証が提示された場合、OAuth 2.1に従う`401` challengeを返す。
- ブラウザー mutation は session 結合 CSRF token と同一 Origin を要求し、MCP browser request は
  明示した HTTPS Origin のみ許可する。

## 0.2.0 — 2026-07-24

バージョン0.2.0から、データフォーマットv1の処理にAdocWeave v0.6.1を使用する。
`/api/v1`のOpenAPI仕様とMCPツール仕様は変更しない。

### 破壊的変更

- データフォーマットv1をAdocWeave v0.6.1の解析規則とノートプロファイルで上書きした。
  以前のv1との互換性や移行手順は提供しない。
- 更新前にサービスを停止し、必要な退避を行ったうえで既存`dataDir`を完全に削除し、
  空のディレクトリから初期化する必要がある。旧バックアップを用いた復元機能は提供しない。

### 変更

- AdocWeaveのRust版とWASM版をv0.6.1の同一コミットへ固定し、文書モデル、HTML、
  文書から生成する構造化データ、属性出現箇所を公開APIへ移行した。
- `/acceptance`に、ACLを適用したノート一覧と、作成・取得・更新・検索・削除を確認する
  JavaScript不要のフォームを追加した。
- `marginalis --version`と`marginalis -V`で、実行中のバイナリに組み込まれた
  アプリケーション版を確認できるようにした。

### 修正

- `/acceptance`のHTMLフォームではhidden fieldのセッション連動CSRFトークンを検証し、
  任意ヘッダーを設定できない通常のブラウザーフォームから送信できるようにした。
- レスポンスの`X-Request-Id`と、同じリクエストのtracingスパンへ記録するrequest IDが
  一致するよう、HTTPミドルウェアの適用順を修正した。

## 0.2.0-rc.1 — 2026-07-24

AdocWeave v0.6.1への移行を検証する、v0.2.0の最初のリリース候補である。
`/api/v1` の OpenAPI 仕様と MCP 仕様は変更しない。

### 破壊的変更

- AdocWeaveをv0.6.1へ更新し、データフォーマットv1の意味を新しい解析規則と
  ノートプロファイルで上書きした。以前のv1との互換性や移行手順は提供しない。
- 更新前にサービスを停止し、必要な退避を行ったうえで既存`dataDir`を完全に削除し、
  空のディレクトリから初期化する必要がある。アプリケーションはこの削除を自動実行しない。
- 以前のv1と新しいv1は`FORMAT`マーカーだけでは識別できない。旧`dataDir`や旧バックアップを
  v0.2.0系列で開いてはならない。

### 変更

- AdocWeaveのRust版とWASM版をv0.6.1の同一コミットへ固定し、実行時およびWASM応答の
  `packageVersion`が完全一致することを検証する。
- 文書モデル、HTML、文書から生成する構造化データ、属性出現箇所をAdocWeaveの公開APIへ移行した。
- `note-id`、`creator-id`、`created-at`、`updated-at`の置換を、属性の原文範囲に基づく
  処理として`marginalis-asciidoc`へ集約した。
- Nixパッケージも同じAdocWeaveコミットと固定ハッシュから再現可能にビルドする。

### 修正

- 一部のブラウザーやプロキシ環境から、`/acceptance`のフォームを送信できない問題を修正した。
  このサーバー生成HTMLフォームは、hidden fieldのセッション連動CSRFトークンを検証する。
  REST APIでは公開オリジンとCSRFトークンを引き続き必須とし、`Sec-Fetch-Site`がある場合は
  `same-origin`または`none`だけを許可する。

## 0.1.1 — 2026-07-24

v0.1.0 と同じ機能範囲の保守リリースである。公開 API、データフォーマット、MCP 仕様は
変更しない。

### 修正

- バイナリが報告するバージョンを正しい値に整えた（v0.1.0 タグのビルドは内部的に
  `0.1.0-rc.2` を報告していた）。

## 0.1.0 — 2026-07-23

研究室内で REST API と MCP を実運用するための最初のリリースである。`/api/v1` の OpenAPI
API 仕様とデータフォーマット v1 の互換性保証をこのバージョンから開始した。

### 追加

- OIDC ログイン、root 管理、ユーザーへの直接 ACL、監査ログ。
- AsciiDoc 正本を用いる REST ノート CRUD、FTS5 検索、`ETag` による条件付き更新、物理削除。
- OAuth Authorization Code + PKCE で保護された MCP ツール（検索・取得・参照一覧・作成・
  更新・削除）。
- OpenAPI 3.1 仕様、NixOS モジュール、バックアップ・復元、検索・参照データの再構築、root 監査の
  365 日保持。

### セキュリティ

- Cookie を伴う変更操作で、CSRF トークン・公開オリジン・Fetch Metadata を検証する。
- root 管理ルーターを通常 API から分離し、プロキシの forwarded クライアント IP ヘッダーを
  信頼しない。

### 修正

- NixOS のランタイム VM リリーステストへ `sqlite3` CLI を含め、root 資格情報の検証を実行
  できるようにした。

### 既知の制約

- Web UI、SMTP、招待、ユーザー再有効化、グループ ACL、専用管理オリジン・mTLS は含まれない。
- 実際の Kanidm と MCP クライアントを使う受入確認は、秘密情報を CI へ置かずに手動で行う。
- `/api/v1` への破壊的変更は、新しいバージョンパスと非推奨告知を伴って行う。
