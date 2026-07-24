# 038: SQLite正本とAsciiDoc import/export

## 状態

未着手。[037](037-v0.3.0-architecture-rebaseline.md)で決定した単一正本へ置き換える。

## 目的

ノート本文、メタデータ、ACL、投影を SQLite の一トランザクションで更新し、ファイルと
データベースをまたぐ操作ジャーナル、投影再構築、停止中の複合バックアップを不要にする。

## 作業内容

1. ノート本文、タイトル、タグ、作成者、作成・更新日時、revision、削除日時を保存する新しい
   SQLite schema を定義する。ACL、検索、参照、アンカーは同じ database に置く。
2. `create`、`update`、ACL 更新、ソフトデリート、復元、物理削除で、正本と全投影を一つの
   transaction として更新する。revision は DB 正本に基づく楽観的ロックにする。
3. AsciiDoc の header は export 時に生成し、import 時にはサーバー保護属性を検証・上書きする。
   稼働中にノートファイルを読み書きしない。
4. ノート単位 `.adoc` export と、全ノート、ACL、削除状態、format marker、manifest を含む
   archive export/import を実装する。archive は検証に失敗した場合に import しない。
5. `marginalis-files`、操作ジャーナル、ファイル正本を前提とする保守コマンドと設定を削除する。

## 対象外

- `v0.2.x` 保存データの自動移行。
- 本文履歴、Git 同期、稼働中のファイル監視。
- PostgreSQL 実装。

## 完了条件

- ノートの通常操作が SQLite の単一 transaction で完了し、ファイル操作ジャーナルを持たない。
- AsciiDoc の単体 export と archive export/import が、本文、ACL、削除状態を失わず往復する。
- 破損・不正な archive を import しても既存 database が変化しない。
- 新しい schema と export 形式が文書化され、単体・結合試験で固定される。
