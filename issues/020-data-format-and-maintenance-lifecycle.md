# 020: data format v1とmaintenance lifecycle

状態: 完了。

## 目的

AsciiDoc正本、SQLite投影・identity・session・audit、backupおよびrestoreの関係を、一つの明示的なdata format
契約として固定する。現在は「旧schemaを拒否する」という文書と、SQLite migrationを継続する実装が併存する。

## 実装項目

1. `dataDir` format version、AsciiDoc profile/AdocWeave contract version、SQLite schema versionの関係を定義する。
2. format v1以前を明確に拒否するか、migrationを提供するかを一方に統一する。
3. backup generationにmanifest、format version、作成時刻、SQLite integrity、正本一覧とhashを記録する。
4. restoreを`verify → stage → switch → rebuild → health check`の状態遷移として明文化する。
5. audit retention（365日）を起動時副作用ではなく、定期maintenance operationとして実行する。
6. backup generationの衝突、部分失敗、保持世代、off-site同期を運用policyとして扱う。

## 完了条件

- 起動時に未知または不整合なdata formatを明確に拒否する。
- backup manifestだけで、復元候補の完全性と互換性を事前判断できる。
- audit retentionがserver再起動の有無に依存しない。
- NixOS moduleのbackup/rebuild/restore手順とCLIの状態遷移が一致する。

## 要判断事項

- 既存deploymentを破棄できる前提で、format v1以前はmigrationせず拒否する。
- backupの保持世代、保存先、off-site複製、暗号化をどの運用手段で担うか。

## 実施結果

- 空のdata directoryだけを`FORMAT` marker付きdata format v1として初期化し、markerのない非空directoryと
  未知versionを起動・maintenance・restore入力で拒否する。
- backupには`FORMAT`、`MANIFEST`、`COMPLETE`を加え、作成時刻、SQLiteと全正本のSHA-256を記録・照合する。
- restoreは`format確認 → manifest照合 → SQLite integrity → AsciiDoc検証 → 新directoryへのstage`で実行する。
- root監査の365日保持は起動時副作用から`prune-audit` maintenance commandと日次NixOS timerへ移した。
- backup保存先、世代数、off-site複製および暗号化は、data formatの互換性契約ではなく配備ごとの運用policyとする。
