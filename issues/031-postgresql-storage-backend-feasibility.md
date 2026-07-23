# 031: PostgreSQL storage backendの実現性調査

状態: RC.2 リリース後に調査。

## 目的

SQLiteを既定の単一node storageとして維持しつつ、PostgreSQLを選択可能なstorage backendとして
提供できるかを判断する。これはSQLiteを直ちに置換する実装Issueではない。採用する場合の責務境界、
データ移行、NixOS運用、backup/restoreおよび検証戦略を、実装開始前に決定する。

## 現状と問題

現在の`marginalis-sqlite`は、ノート投影だけでなく、OIDC identity、session、root credential、
MCP OAuth token、削除確認、操作journal、root監査およびmaintenanceを保持する。`marginalis-server`と
`marginalis-service`は`SqliteDatabase`を直接組み立て、SQLiteのmigration、FTS5、backup fileの
`integrity_check`およびdataDir内SQLite fileを前提とする。

PostgreSQLをURL設定だけで有効にすると、検索順位・phrase query、transaction isolation、unique制約、
時刻表現、token hash、操作journalの復旧、backup整合性およびACL非漏洩の契約がSQLiteと分岐する。
正本AsciiDocは引き続きfile systemにあるため、DBをPostgreSQLへ移しても正本と投影をまたぐ書込み・
復旧の不変条件は残る。

## 調査・設計項目

1. **採用判断**: 複数process・高可用性・運用監査・既存PostgreSQL基盤など、PostgreSQLを選ぶ具体的な
   利用条件と、SQLiteを既定に残す理由・非目標を整理する。単一node deploymentでの複雑性と運用負荷も比較する。
2. **port境界**: `SqliteDatabase`への直接依存をapplication portへ戻し、identity、session、OAuth、
   note projection、ACL、journal、監査およびmaintenanceのbackend中立な責務を定義する。transportの
   production dependency graphに具体DB adapterを入れないことをcompile-timeで検証する。
3. **schemaとquery互換性**: SQLite migrationとPostgreSQL migrationのversion管理方針を決める。FTS5の
   phrase semanticsとtitle優先順位をPostgreSQL全文検索で同じAPI結果にできるか、またはbackend共通の
   検索契約をどう限定するかをfixtureで比較する。
4. **transactionと同時実行**: optimistic revision、操作journal、削除確認、ACL更新、session/token失効および
   projection再構築について、PostgreSQLのisolation level・locking・retry方針を定義する。複数server processを
   許可するか、単一writerを維持するかを明記する。
5. **正本・backup・restore**: file systemのAsciiDoc正本とPostgreSQL投影の整合したbackup境界を設計する。
   SQLiteのfile copy、manifest hash、`integrity_check`に相当するPostgreSQL dump/snapshot、restore staging、
   rollbackおよびprojection rebuildの手順を定義する。
6. **設定とNixOS運用**: backendの選択、接続URLとcredential、TLS、connection pool、health/readiness、
   database provisioning、backup job、secret注入をNixOS moduleとcontainer以外の運用でも扱える形で設計する。
   PostgreSQLのpasswordをNix store、ログ、OpenAPIまたは診断へ出さない。
7. **migration**: existing SQLite v1 deploymentからPostgreSQLへ移すexport/importまたはone-shot migrationを
   検討する。`FORMAT`、SQLite schema revision、PostgreSQL schema revision、正本profileの関係と、失敗時の
   rollback・再実行・検証を定義する。暗黙移行は行わない。
8. **test matrix**: SQLiteとPostgreSQLの両backendで、CRUD、検索filter/ranking、ACL非漏洩、OIDC/root、
   MCP OAuth、journal recovery、backup/restore、projection rebuildおよびNixOS VMを同じcontract suiteで
   検証する。CIのPostgreSQL service、fixture隔離、parallel test、artifact/secret policyを決める。

## 成果物と完了条件

- SQLite継続、PostgreSQL追加、または不採用の判断と根拠が文書化されている。
- 採用する場合は、backend中立port、schema/migration、検索契約、transaction境界、backup/restore、
  NixOS設定およびtest matrixを含む実装計画がある。
- SQLite v1 deploymentを破壊せず、既存REST/MCP/OpenAPI contractとdata formatの互換性方針が明確である。
- PostgreSQLを有効にする実装へ進む判断は、少なくとも一つの実PostgreSQL contract testと、
  migration/rollbackの設計レビューを通過してから行う。
