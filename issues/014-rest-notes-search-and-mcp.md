# 014: RESTノートAPI・検索・MCP連携

状態: 機能実装済み。実MCP clientを用いるHTTP/OAuth end-to-end試験とNixOS VM結合試験を後続作業とする。

Web UIより先に、認証済み利用者がREST APIだけでノートの作成、取得、更新、検索および物理削除を
完結できるようにする。同じapplication use caseをMCP toolから再利用し、MCPを後付けのHTTP handlerや
SQLite直結の別実装にしない。

## 背景

現在のREST APIにはsourceの作成・取得・更新・削除とACL更新がある。一方、ノート一覧、検索結果、
安定した検索paginationおよびMCP transportは未実装である。ブラウザーUIを先に導入すると、検索・ACL・
エラー表現がUI専用になり、MCPとの二重実装を招く。先にAPI契約を確定する。

## 範囲

- RESTによるノート一覧、メタデータ取得、source CRUD、検索、pagination
- タイトル、タグ、本文およびanchorを対象とするACL非漏洩検索
- 作成・更新・物理削除と検索投影の整合・復旧
- REST検索use caseを再利用するMCP search/read tool
- MCP専用の認証・認可境界、rate limitおよび監査方針
- NixOS package/moduleからMCP serverを有効化する設定

Web UI、ベクトル検索、招待、rootによるMCP接続は範囲外とする。

## 実装順序

1. **REST CRUD契約の完成**
   - `GET /api/v1/notes`で、閲覧可能なノートのID、title、更新時刻およびcursorを返す。
   - `GET /api/v1/notes/{note_id}`で閲覧用メタデータとprojectionを返す。
   - 既存のsource CRUDをrevision（ETagまたは明示的revision）で条件付き更新可能にし、競合を`409`で表現する。
   - source取得で強い`ETag`を返し、更新・削除は`If-Match`を必須として同時更新を拒否する。
   - 不可視ノートは一覧、検索、取得の全てで存在を推測させない。

2. **検索投影とREST検索**
   - Issue 005および012の調査結果を用いて、SQLite FTS5を最初の検索実装候補として評価・採用判断する。
   - `GET /api/v1/search?q=...&limit=...&cursor=...`を追加し、note ID、titleおよび次cursorを返す。一致箇所の抜粋や本文は返さない。
   - ACL filterは結果数と順位を返す前に適用する。Read権限がないノートを候補数にも含めない。
   - create/update/deleteとrecoveryで検索投影を収束させ、再構築API/CLIを用意する。

3. **MCP認証とtransport**
   - Issue 012で、MCPのOAuth/token方式、transport（Streamable HTTPを第一候補）、client登録およびtoken audienceを確定する。Web session Cookieやroot accountをMCPへ流用しない。
   - `marginalis-mcp` adapterを追加し、認証済みActorをREST検索use caseへ渡す。
- 初期toolは`search_notes`、`get_note`、`list_note_links`、構造化create/update、確認付き物理deleteとする。
  各toolはRESTと同じACL、pagination、エラー意味論を用いる。
   - `marginalis-mcp`はJSON-RPC tool adapterとしてHTTP/OAuth/SQLiteから独立させ、`NoteUseCases`と
     MCP専用Bearer-token authenticatorだけに依存させる。

4. **運用・結合試験**
   - RESTとMCPで同一Actorに同一の可視結果だけが返ることを結合試験で確認する。
   - ACL変更、物理削除、投影再構築、token失効、rate limitを含めて確認する。
   - NixOS VMでREST、MCP有効化、永続化および認証設定を検証する。

## 完了条件

- OIDCでログインした利用者が、Web UIなしでREST APIからノートを作成・取得・更新・検索・削除できる。
- 検索結果はACLを満たすノートだけから構成され、不可視ノートの存在を漏らさない。
- MCPのread/search/link/write/delete toolが同じuse caseとACL判定を利用する。
- RESTとMCPの検索結果、cursor、削除後の可視性を結合試験で検証する。

## 2026-07-23時点の実装状況

- REST CRUD、ETagを使う条件付き更新・物理削除、ACL非漏洩の一覧・FTS5検索・cursorを実装した。
- MCPのread/search/link/write/delete toolは`NoteUseCases`を共有し、access tokenのscope/resource/
  利用者状態をSQLiteで検証する。
- MCP OAuthはPKCE S256、single-use authorization code、token pair、refresh token rotation、Client ID
  Metadata Documentを備える。NixOSでは明示的な`mcp.enable`とmetadata host許可リストが必要である。
- MCPはread/search、構造化create/update、二段階の物理delete、ACL非漏洩のoutgoingリンク一覧を提供する。
  rate limitも実装した。SQLiteを用いるPKCE code、token exchange、refresh rotation、Bearer認証の
  server結合試験と、通常sessionから認可画面、code、token、`/mcp`までを通すHTTP結合試験を実装した。
  実clientを使う相互運用試験は未実装である。
