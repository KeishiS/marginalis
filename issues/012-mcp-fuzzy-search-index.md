# 012: MCP曖昧検索用の中間表現インデックス調査

状態: 初期実装済み。検索拡張と運用結合試験はIssue 014で継続する。

ノートの作成・更新・物理削除を契機として検索用の中間表現（IR）を更新し、MCP経由で権限を守った
曖昧検索を提供するための設計を調査する。初期REST APIおよび正本ファイルの意味論は変更しない。

## 前提

- AsciiDoc sourceが正本であり、SQLiteは再構築可能な投影である。
- 現在の`NoteWriteService`はoperation journalを介して、正本とSQLite投影を更新・復旧する。
- ノートが物理削除された場合、検索インデックスからも同じノートを除外する。
- Read権限がないノートの存在、題名、断片、順位その他の情報を検索結果から推測できてはならない。
- MCPはWeb UIとは独立した後続の利用境界である。

## 調査事項

1. **中間表現の定義**

   検索対象を、ノート単位・見出し単位・段落単位のどれにするか比較する。少なくとも`note_id`、
   source revision、title、本文抽出値、anchor、位置範囲、および再構築に必要なversionを検討する。
   AsciiDoc AST/projectionから安全に抽出できる本文と、xref・数式・source blockを検索語に含める規則も
   定める。

2. **曖昧検索方式**

   SQLite FTS5（tokenizer、prefix、BM25、snippet）、trigram索引、外部のベクトル検索、および
   keywordとsemantic retrievalの組合せを比較する。日本語・英語混在、typo、短いクエリ、ランキングの
   再現性、運用時の依存追加、バックアップとNixOS配布への影響を評価する。

3. **更新・復旧プロトコル**

   `NoteWriteService`の成功後に同期更新する方法、operation journalをoutboxとして非同期更新する方法、
   および起動時の全量再構築を比較する。作成・更新・削除の冪等性、source revisionによる古い更新の
   排除、失敗時の再試行、検索投影のlagの公開方法を決める。

4. **ACLを含む検索実行**

   検索前フィルタ、候補取得後フィルタ、ACLを検索投影へ複製する方法を比較する。root、直接ACL変更、
   削除直後、競合する更新について、情報漏洩なく結果数・snippet・scoreを返す境界を定義する。

5. **MCP API境界**

   tool名、入力（query、limit、検索対象、cursor）、出力（note ID、title、anchor、短い安全なsnippet、
   score）、認証済みActorの伝達、エラー表現、ページング、rate limit、監査ログの要否を設計する。
   MCP transportとOAuth/token方式はWeb session cookieに依存させない。

6. **運用・保守**

   schema migration、インデックスversion、全量再構築コマンド、整合性検査、バックアップ／復元、
   観測可能性、データ量と性能の受入基準を決める。

## 調査成果物

- 推奨するIR schemaと検索方式、および不採用案との比較表。
- 作成・更新・削除から検索更新・復旧に至る状態遷移。
- ACL非漏洩を含むMCP tool contract。
- migration／rebuild／障害復旧の運用手順。
- 実装を分割する後続issueと、各issueの検証計画。

## 2026-07-23時点の判断

### 検索投影

- 初期検索はSQLite FTS5を採用する。AdocWeaveの`searchable_text` projectionを本文の基礎とし、
  profileで許可したsource blockとLaTeX数式を含める。生のAsciiDoc属性・マクロ記法は索引しない。
- SQLite queryは`note_search`の候補に対し、同じSQL statementで`note_acl`の`EXISTS`を適用する。
  不可視ノートを候補数、順位、cursorまたは本文断片へ含めない。
- 初期のREST/MCP検索結果はnote IDとtitleだけとする。本文、snippet、scoreは返さない。検索結果の
  cursorはACL filter後の結果集合だけを進める。
- FTS5は同期投影とする。`NoteWriteService`が正本を書き込み、operation journalで投影更新を復旧する。
  ベクトル検索、外部検索サービス、非同期outboxは初期公開の範囲外とする。

### MCP認証・transport

- Streamable HTTPの単一`/mcp` endpointを採用する。POSTはJSON-RPC requestを受け、GETは通知streamが
  必要になるまで`405`を返す。初期toolは`search_notes`、`get_note`、`list_note_links`、構造化
  create/updateおよび確認付き物理deleteとする。
- MCP resource serverはRFC 9728 Protected Resource Metadataを提供する。未認証の`/mcp` requestは
  `401`と`WWW-Authenticate` headerでmetadata URLを示す。
- OAuth Authorization ServerはMarginalisが担当し、外部Kanidmは利用者の本人認証だけに使う。access/
  refresh tokenはopaque random valueを発行し、DBにはhashだけを置く。Web session Cookieとroot sessionは
  MCPで受理しない。
- Authorization Code + PKCE S256、resource indicator、Client ID Metadata Documents、HTTPSまたは
  loopback redirect URI完全一致検証を初期境界とする。Dynamic Client RegistrationとDevice Flowは後続に
  追加可能なportとして分離する。
- MCP endpointでは、存在する`Origin`をBase URLのoriginと照合して拒否する。各Bearer tokenはMCP
  canonical resource URIとscopeを照合し、root actorへは発行・認証しない。

この判断はMCP Authorization SpecificationおよびStreamable HTTP transportの2025-11-25版に基づく。

## 完了条件

- 正本、検索投影、MCPの責務境界が明文化されている。
- ACLを持たないActorに情報を漏らさない検索手順が選定されている。
- 作成・更新・削除・復旧の各経路で検索投影を収束させる方式が選定されている。
- 採用する検索技術とNixOS運用への影響が根拠とともに記録されている。

## 実装済み範囲

- `NoteWriteService`の同期投影としてSQLite FTS5を更新し、物理削除と起動時recoveryにも追従させた。
- RESTとMCPの`search_notes`・`get_note`・`list_note_links`および書込みtoolは同じ`NoteUseCases`と
  ACL非漏洩queryを利用する。
- MCPはStreamable HTTP、Protected Resource Metadata、OAuth Authorization Code + PKCE S256、opaque
  access/refresh token、resource audience照合、refresh token rotationを実装した。
- NixOS moduleはMCPを明示opt-inにし、Client ID Metadata Documentの取得hostを許可リストに限定する。

実運用のMCP clientとのOAuth結合試験、全文snippet・semantic search、server-to-client notification
streamは初期実装に含まれず、後続作業とする。MCP tool呼出しの利用者単位rate limitは実装済みである。
