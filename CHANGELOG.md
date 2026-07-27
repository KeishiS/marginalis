# 変更履歴

この文書には利用者に影響する変更だけを記録する。公開 API、データフォーマット、NixOS
モジュールの動作を変えない内部的な再構成は掲載しない。

## 0.4.0 — 2026-07-27

### 破壊的変更

- SQLite schemaを3へ、archiveを`marginalis-archive-2`へ更新した。旧schemaと旧archiveの
  自動移行は提供せず、空のdatabaseから再初期化する。
- ノート本文の解析規則をAdocWeave 0.10.1のStrict modeへ更新した。従来保存できた本文でも、
  現行profileに適合しない場合は拒否する。

### 追加

- RESTとMCPで共通の、安定code、対象field、UTF-8 byte範囲を持つ入力診断を追加した。
- MCPに`get_note_profile`を追加し、入力上限、正規化、許可構文、許可言語、禁止規則、
  有効な本文例を機械可読な形式で公開した。
- AdocWeave 0.10.1の解析診断とHTML描画診断を保存・表示境界で検査するようにした。

### 修正

- 保存時と表示時の構文modeを統一し、保存済みノートが後から描画不能になる経路を閉じた。
- archiveの全階層で未知fieldを拒否し、形式の誤認を防ぐようにした。

## 0.3.1 — 2026-07-27

### 追加

- archiveの構造と論理的な往復を検証するコマンド、隔離した空のSQLite databaseへの復元検証、
  最新成功backupの検証、安全な世代整理を追加した。
- NixOS moduleに日次backup、30世代保持、四半期の復元検証timer、読み取り専用の
  `marginalis diagnose`を追加した。
- Kanidm 1.10、private CA、nginxのsubpath、実ブラウザー、Dynamic Client Registration、
  Authorization Code + PKCE、token rotation、MCP初期化、認可取消を通すNixOS VM試験を追加した。
- backup、復元、purge、OIDC discovery、MCP OAuthをjournaldで追跡する安定event名を追加した。

### セキュリティ

- MCP承認フォームでは不透明Originとの互換性を維持しつつ、異なる具体的Originからの送信を
  拒否するようにした。
- protocol回帰試験の失敗出力からCookie、Bearer、認可コード、OAuth token、CSRF token、
  client secret、PKCE verifierを除去してから保存・表示する検査を追加した。

## 0.3.0 — 2026-07-25

SQLite を単一の正本とし、Kanidm の署名済み OIDC group claim、閲覧用 Web UI、REST API、
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

- OAuth authorization code の消費を PKCE challenge と原子的に結合する。
- 使用済みauthorization codeの再提示時に、発行済みtoken family全体を失効させる。
- 使用済み refresh token の再提示時に token family 全体を失効させる。
- token endpointで未対応のHTTP client認証が提示された場合、OAuth 2.1に従う`401` challengeを返す。
- ブラウザー mutation は session 結合 CSRF token と同一 Origin を要求し、MCP browser request は
  明示した HTTPS Origin のみ許可する。

## 0.2.0 — 2026-07-24

AdocWeave v0.6.1を使うデータフォーマットv1の新しい基準点である。
`/api/v1`のOpenAPI仕様とMCPツール仕様は変更しない。

### 破壊的変更

- データフォーマットv1をAdocWeave v0.6.1の解析規則とノートプロファイルで上書きした。
  以前のv1との互換性や移行経路は提供しない。
- 更新前にサービスを停止し、必要な退避を行ったうえで既存`dataDir`を完全に削除し、
  空のディレクトリから初期化する必要がある。旧バックアップも復元入力には使用できない。

### 変更

- AdocWeaveのRust版とWASM版をv0.6.1の同一コミットへ固定し、文書モデル、HTML、投影、
  属性出現箇所を公開APIへ移行した。
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
  ノートプロファイルで上書きした。以前のv1との互換性や移行経路は提供しない。
- 更新前にサービスを停止し、必要な退避を行ったうえで既存`dataDir`を完全に削除し、
  空のディレクトリから初期化する必要がある。アプリケーションはこの削除を自動実行しない。
- 以前のv1と新しいv1は`FORMAT`マーカーだけでは識別できない。旧`dataDir`や旧バックアップを
  v0.2.0系列で開いてはならない。

### 変更

- AdocWeaveのRust版とWASM版をv0.6.1の同一コミットへ固定し、実行時およびWASM応答の
  `packageVersion`が完全一致することを検証する。
- 文書モデル、HTML、投影、属性出現箇所をAdocWeaveの公開APIへ移行した。
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
- OpenAPI 3.1 仕様、NixOS モジュール、バックアップ・復元、投影再構築、root 監査の
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
