# 015: API-firstアーキテクチャ再基線化

状態: 優先。Issue 014より先に実施する。

RESTによるノートCRUD・検索とMCP toolを同じ業務ロジックへ接続するため、現在のHTTP中心の組立を
破壊的に再構成する。公開HTTP API、SQLite schema、設定形式およびRust crateの後方互換は要求しない。
ただし、運用中の正本ファイルおよびSQLiteを自動削除してはならず、移行または明示的な再初期化の
手順を用意する。

## 現状の問題

- `marginalis-web`がAxum handlerからSQLite、ファイル、AsciiDoc parser、OIDC adapterおよび
  `NoteWriteService`を直接呼ぶ。RESTとMCPで同じ処理を再利用できない。
- binaryの組立・設定読込・tracing初期化が`marginalis-web`にあり、`marginalis-server`は設定と
  Clock/Randomだけを持つ。crate名と責務が逆転している。
- ノート更新はuse case化されているが、閲覧、一覧、検索、認証、session、root管理はtransport層から
  adapterへ直接到達する。
- OIDC登録ポリシーは`RegistrationPolicy::default()`へ固定され、設定・永続化・管理APIの境界がない。
- HTTPのエラー変換、CSRF、監査、rate limitおよびrequest IDは個別handlerに分散する。
- SQLiteは検索用read modelを持たず、検索・MCPのACL非漏洩を一つの契約として検証できない。

## 目標構成

```text
HTTP REST        MCP transport        CLI / maintenance
     │                 │                     │
     └─────────────── transport adapters ────┘
                           │
                   marginalis-application
             commands / queries / policy / ports
                           │
      ┌──────────────┬─────┴─────┬──────────────┐
      SQLite          files       AsciiDoc       OIDC
      adapter         adapter     adapter        adapter
                           │
                    marginalis-server
          composition root / config / tracing / serve
```

`marginalis-application`は、transport・sqlx・filesystem・AdocWeave・OIDC libraryへ依存しない。
各transportは型付きcommand/queryだけを呼び、adapterの具体型およびDB connection poolを保持しない。

## 破壊的な設計決定

1. **application facadeを導入する。** `NoteCommands`、`NoteQueries`、`IdentityCommands`、
   `SessionCommands`および`AdministrationCommands`を、request/response型とdomain errorを持つ
   application APIとして定義する。HTTP/MCPはこのAPIだけに依存する。
2. **読み取りと書き込みを分離する。** source正本を更新するcommandと、SQLite投影だけを返すqueryを
   分ける。検索は`SearchNotes` queryに集約し、ACL filter、cursor、snippet、順位をその契約へ含める。
3. **認証済み主体を統一する。** Web Cookie、MCP access tokenおよび将来のCLI tokenは、検証後に
   `Principal`（内部UserId、権限、認証種別、session/token識別子）へ変換する。rootはMCP principalに
   変換しない。
4. **設定と実行をserverへ集約する。** binaryを`marginalis-server`へ移し、`marginalis-web`を
   HTTP transport adapterとして縮小する。`ServerConfig`へ登録ポリシー、session lifetime、API/MCPの
   listener・公開URLを追加し、NixOS moduleはこの設定に対応する。
5. **SQLite schemaをread model中心に再設計する。** users/ACL/session/監査と、notes/anchors/references/
   検索文書を明確に分ける。検索文書は正本revisionとprojection versionを持ち、再構築可能とする。
6. **transport横断の境界処理を一元化する。** error mapping、request ID、tracing、認証失敗のredaction、
   CSRF（HTTPだけ）、rate limit、監査をmiddlewareまたはapplication commandへ集める。

## 実装順序

1. application request/response、domain error、port traitを定義し、既存adapterの具体型をHTTP stateから
   追い出す。command/queryのin-memory fakeでユースケースを単体試験できる状態にする。
2. SQLite/files/AsciiDoc adapterを新portへ実装し、ノートcreate/read/update/deleteを新facadeへ移す。
   operation journalとrecoveryの責務はnote commandに閉じる。
3. OIDC、root login、session、登録ポリシーおよびユーザー状態をidentity/session facadeへ移す。
   disabled userのsession・token失効と、監査のtransaction境界を定義する。
4. HTTP adapterを新facadeだけで書き直す。現行APIは維持せず、Issue 014のREST resource設計へ置換する。
5. 検索read modelと`SearchNotes` queryを導入してから、MCP adapterを接続する。
6. server binary、NixOS module、VM testおよび運用CLIを新構成へ移す。旧crate・旧route・旧schemaを
   明示的に削除する。

## データ移行方針

- AsciiDoc sourceは正本として保持し、新projectionはそこから再構築する。
- SQLite内のidentity、root credential、ACL、設定および監査は再構築できないため、明示的なone-shot
  importerまたは新規DBへの管理者承認手順を提供する。自動破棄しない。
- 開発環境では旧SQLiteを破棄して新schemaを作成してよい。本番ではbackup作成、dry-run、完了確認を
  行う移行commandを必須とする。

## 完了条件

- HTTPとMCPがSQLite、files、AsciiDoc、OIDCの具体adapterを直接参照しない。
- RESTのCRUD/検索とMCPのread/searchが同じapplication command/queryとACL policyを利用する。
- 既存sourceを再構築した新projectionと、identity/ACL等の移行手順が検証されている。
- serverの組立、NixOS設定、tracingおよびmaintenance CLIの責務が一箇所にある。
