# 031: PostgreSQL対応の実現性調査

## 状態

未着手。v0.1.0の公開後に調査する。

## 目的

SQLiteを単一ノード用の既定の保存先として維持しながら、PostgreSQLを選択肢として
追加できるかを判断する。このIssueではPostgreSQL対応を実装しない。採用する場合の責務、
データ移行、NixOS運用、バックアップ、復元、検証方法を実装前に決める。

## 背景

現在の`marginalis-sqlite`は、検索用データだけでなく、OIDC利用者、セッション、
root認証情報、MCP OAuthトークン、削除確認、操作履歴、root監査、保守情報を保持する。
`marginalis-server`と`marginalis-service`は`SqliteDatabase`を直接組み立てる。SQLiteの
マイグレーション、FTS5、バックアップファイルの`integrity_check`、`dataDir`内の
SQLiteファイルも前提としている。

PostgreSQLをURL設定だけで有効にすると、検索順位・phrase query、transaction isolation、unique制約、
時刻表現、token hash、操作journalの復旧、backup整合性およびACL非漏洩の契約がSQLiteと分岐する。
ノート本文は引き続きファイルシステムにある。DBをPostgreSQLへ移しても、本文と
検索用データをまたぐ書込み・復旧の不変条件は残る。

## 調査項目

1. **採用判断**: 複数process・高可用性・運用監査・既存PostgreSQL基盤など、PostgreSQLを選ぶ具体的な
   利用条件と、SQLiteを既定に残す理由・非目標を整理する。単一node deploymentでの複雑性と運用負荷も比較する。
2. **保存層のインターフェース**: `SqliteDatabase`への直接依存をapplication portへ戻す。
   利用者、セッション、OAuth、検索用データ、ACL、操作履歴、監査、保守の責務を
   DB製品に依存しない形で定義する。本番通信層が具体的なDB実装へ依存しないことを
   コンパイル時に検証する。
3. **schemaとquery互換性**: SQLite migrationとPostgreSQL migrationのversion管理方針を決める。FTS5の
   phrase semanticsとtitle優先順位をPostgreSQL全文検索で同じAPI結果にできるか、またはbackend共通の
   検索仕様をどう限定するかをテストデータで比較する。
4. **transactionと同時実行**: optimistic revision、操作journal、削除確認、ACL更新、session/token失効および
   projection再構築について、PostgreSQLのisolation level・locking・retry方針を定義する。複数server processを
   許可するか、単一writerを維持するかを明記する。
5. **本文・バックアップ・復元**: ファイルシステムのAsciiDoc本文とPostgreSQLの検索用データを、
   整合した状態でバックアップする方法を設計する。SQLiteのファイルコピー、manifest hash、
   `integrity_check`に相当するPostgreSQLのdumpまたはsnapshot、復元、切戻し、
   検索用データの再構築手順を定義する。
6. **設定とNixOS運用**: backendの選択、接続URLとcredential、TLS、connection pool、health/readiness、
   database provisioning、backup job、secret注入をNixOS moduleとcontainer以外の運用でも扱える形で設計する。
   PostgreSQLのpasswordをNix store、ログ、OpenAPIまたは診断へ出さない。
7. **migration**: existing SQLite v1 deploymentからPostgreSQLへ移すexport/importまたはone-shot migrationを
   検討する。`FORMAT`、SQLite schema revision、PostgreSQL schema revision、正本profileの関係と、失敗時の
   rollback・再実行・検証を定義する。暗黙移行は行わない。
8. **試験項目**: SQLiteとPostgreSQLの両方で、CRUD、検索条件と順位、ACL非漏洩、OIDC/root、
   MCP OAuth、操作の復旧、バックアップと復元、検索用データの再構築、NixOS VMを
   同じ契約試験で検証する。CIのPostgreSQLサービス、テストデータの隔離、並列実行、
   成果物と秘密情報の扱いを決める。

## 完了条件

- SQLite継続、PostgreSQL追加、または不採用の判断と根拠が文書化されている。
- 採用する場合は、backend中立port、schema/migration、検索契約、transaction境界、backup/restore、
  NixOS設定およびtest matrixを含む実装計画がある。
- SQLite v1の既存環境を破壊せず、REST・MCP・OpenAPI仕様と保存形式の互換性方針が明確である。
- PostgreSQLを有効にする実装へ進む判断は、少なくとも一つの実PostgreSQL contract testと、
  migration/rollbackの設計レビューを通過してから行う。
