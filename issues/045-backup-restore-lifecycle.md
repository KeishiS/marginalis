# 045: backup・復元ライフサイクル

## 状態

実装完了。archive検証、隔離復元、成功世代管理、NixOS timer、障害経路を自動試験で保護した。

## 目的

`marginalis-backup.service`が作成したarchiveを、運用中のdatabaseへ影響を与えずに検証し、
障害時に空のSQLite databaseへ復元できることを継続的に証明する。backupの作成成功だけを
データ保全の根拠にしない。

## 決定事項

- `backupDirectory`を設定した場合の日次backupと30世代保持を既定の設計基準とする。
- 四半期ごとの週末に、最新archiveを一時的な空databaseへ復元する試験を行う。
- backupは稼働中の単一SQLite read transactionから取得し、通常のHTTP serviceを停止しない。
- 復元試験は本番database、Web session、MCP authorization、Kanidmを変更しない。
- 保存媒体のsnapshot、off-site複製、暗号化はNixOS host側の責務とし、保存先は引き続き
  NixOS設定で指定する。

## 作業内容

1. archiveのformat、schema、全ノート、ACL、削除状態、revisionを検証するcommandを追加する。
2. 空の一時databaseへimportし、再exportした論理内容が元archiveと一致する復元試験を実装する。
3. NixOS VM試験で、ノート作成、backup、元databaseから隔離した復元、可視性、ソフトデリート状態を
   一気通貫で確認する。
4. `backupDirectory`設定時のtimer、保存世代数、安全な世代削除をNixOS moduleへ追加する。
   作成途中または検証失敗した世代を成功世代として数えない。
5. 四半期復元試験を手動または明示的に有効化したtimerから実行できるようにする。
6. 復元、切戻し、必要disk容量、失敗時の確認箇所を`docs/nixos.md`へ記載する。

## 安全条件

- import先が空でない場合は失敗し、既存databaseを暗黙に上書きしない。
- 世代削除は正規化済みの`backupDirectory`直下でMarginalisが作成した成功世代だけを対象にする。
- 最新の成功世代を削除せず、backup作成または検証に失敗した場合も既存世代を保持する。
- archive、log、CI artifactへCookie、token、client secret、ノート本文を不用意に出力しない。

## 完了条件

- NixOS VMでbackupから空databaseへの復元と論理内容の一致を自動確認できる。
- 日次backup、30世代保持、四半期復元という既定方針とNixOS optionの関係が文書化されている。
- 本番databaseを上書きしない復元runbookが存在する。
- backup失敗、archive破損、空でないimport先、世代削除失敗を安全側で処理する試験がある。
