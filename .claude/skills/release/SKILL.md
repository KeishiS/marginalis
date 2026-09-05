---
name: release
description: Marginalisの版更新、公開候補の確認、安定版公開、失敗した公開処理の再開に使用します。
---

# Marginalisのリリース

最初に、リポジトリルートの`docs/developer-guide/release.adoc`を全文読みます。
同文書を手順の正本とし、受入項目は`docs/developer-guide/acceptance.adoc`で確認します。
版番号、契約、asset集合、コマンドの詳細はここへ複製しません。

## 作業範囲

ユーザーが依頼した範囲と既に得た承認に従います。版更新だけの依頼から公開を推定しません。
公開まで依頼されている場合、Release Notesの追加承認を一律に求めず、PR上で内容と検証を
確認できる状態にして進めます。実配備先での人手受入は、自動検証の成功から実施済みと推定せず、
未実施の項目と公開可否の判断をPRへ記録します。

## 判断と確認

1. 対象リポジトリ、作業ツリー、最新の公開版、`main`の差分とCI、関連する未完了PRを確認します。
   無関係なopen PRがあるだけで停止せず、公開対象と競合する変更がないか判断します。
2. 版更新とRelease NotesをPRへまとめます。archiveの保存契約が変わる場合は、
   リリース手順が指定する直前契約と「保存契約の履歴」を更新します。
   MCPの執筆支援情報の版とarchiveの保存契約の版を混同しません。
3. 所定の検証後、squash方式のauto-mergeを設定します。PRのチェック成功とマージ完了を
   別々に確認し、マージコミットの完全SHAを記録します。
4. そのSHAの`main` CIと`release-candidate`の成功を確認します。PRでは候補が省略されるため、
   PRの成功だけでは公開へ進みません。公開直前にリモートの`main`先端を再取得し、
   候補SHAと一致することを確認します。公開完了までは他のPRをマージしません。
5. 対象リポジトリを`--repo KeishiS/marginalis`で明示し、公開workflowへ完全SHAを渡します。
   開始したrun IDを記録して監視します。タグとReleaseはworkflowだけが作成します。
6. workflow、安定版の公開状態、宣言されたasset集合、タグが指すコミット、両CPU向けcacheの
   結果を確認します。attestationの存在確認と署名の検証は区別して報告します。

## 待機と失敗時の再開

`gh pr checks <PR> --repo KeishiS/marginalis --watch`と
`gh run watch <run ID> --repo KeishiS/marginalis --exit-status`で監視します。
チェック終了後はPRの`state`と`mergeCommit`も確認します。実行環境の継続可能なセッションを使い、
待機中も進捗を伝えます。

失敗した場合は、ログとタグ・Releaseの実在を確認してからリリース手順の再開条件に従います。
原因を確認しない自動再試行や、タグの付け替え・削除は行いません。
完了後は未コミット変更を確認してローカル`main`を追従させ、利用者のデータや他の作業を保全します。
