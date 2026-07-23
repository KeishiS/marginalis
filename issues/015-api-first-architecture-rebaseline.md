# 015: API-firstアーキテクチャ再基線化

状態: 実装中。REST/MCP共通use case、composition root、identity管理、root監査および投影再構築CLIは
完了した。実OIDC/MCP serviceを用いるNixOS VM結合試験を継続する。

RESTによるノートCRUD・検索とMCP toolを同じ業務ロジックへ接続するため、現在のHTTP中心の組立を
破壊的に再構成する。公開HTTP API、SQLite schema、設定形式およびRust crateの後方互換は要求しない。
既存デプロイのAsciiDoc正本およびSQLiteは廃棄可能であり、旧schema・identity・ACLを移行しない。
ただし、dataDirの削除はアプリケーション起動時に自動実行せず、運用者が停止後に明示して行う。

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
   分ける。検索は`SearchNotes` queryに集約し、ACL filter、cursor、順位をその契約へ含める。初期公開の
   検索結果はnote IDとtitleだけとし、本文・一致箇所の抜粋は返さない。
3. **認証済み主体を統一する。** Web Cookie、MCP access tokenおよび将来のCLI tokenは、検証後に
   `Principal`（内部UserId、権限、認証種別、session/token識別子）へ変換する。rootはMCP principalに
   変換しない。
4. **設定と実行をserviceへ集約する。** binaryを`marginalis-service`へ移し、`marginalis-web`を
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

## 初期化方針

- この再基線化では旧dataDirを移行しない。新しいSQLite schema、root credential、OIDC identity、
  ACL、session、監査およびAsciiDoc正本を空の状態から作成する。
- NixOS運用手順は、service停止、対象dataDirのバックアップまたは削除、空directoryの所有者設定、
  configuration適用、root初期化、OIDC login確認の順に固定する。
- applicationおよびNixOS moduleは、通常の起動やrebuildで既存dataDirを削除しない。初期化は運用者が
  明示的に選んだ場合だけ行う。
- 新schemaには旧migrationを引き継がず、空DB作成だけを初期サポート対象にする。旧DBを指定した場合は、
  明確なversion不一致エラーで停止する。

## 完了条件

- HTTPとMCPがSQLite、files、AsciiDoc、OIDCの具体adapterを直接参照しない。
- RESTのCRUD/検索とMCPのread/searchが同じapplication command/queryとACL policyを利用する。
- 空のdataDirからroot初期化、OIDC login、REST CRUD、検索およびMCP read/searchまでを一貫して検証する。
- serverの組立、NixOS設定、tracingおよびmaintenance CLIの責務が一箇所にある。

## 2026-07-23時点の実装状況

- RESTとMCPは`NoteUseCases`を共有し、HTTP handlerからSQLite、file、AsciiDocの具体adapterを参照しない。
- 実行バイナリは`marginalis-service`へ分離し、設定読込、adapter組立、tracing初期化、HTTP listenを集約した。
- request ID、Cookie CSRF、MCP rate limitはHTTP境界にある。identity policyはSQLiteへ永続化した。
- root管理監査はSQLiteへ永続化し、`marginalis rebuild-projections`と`marginalis backup`、対応する
  NixOS oneshot unit、および非破壊的な`marginalis restore`で正本の投影再構築、SQLite・正本の一組backup、
  検証済みdataDir候補の作成を実行できる。VM testはcredential注入、永続directory、service再起動および
  oneshotとの排他を確認する。
- NixOS VM上で実OIDC providerと実MCP clientを用いるend-to-end結合試験は未実装である。
