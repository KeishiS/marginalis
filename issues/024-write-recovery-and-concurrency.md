# 024: 正本更新の復旧状態機械と並行書込み制御

状態: RC.1 release blocker。

## 問題

操作ジャーナルの復旧は`source_applied`以外を単に残す。したがって、正本変更前に停止した
`prepared`操作の取消し、一時ファイルの除去、復旧不能操作の隔離・root通知を行わない。rename直後の
停止で「正本新版・ジャーナルprepared」のような状態も識別できない。

また、更新競合のrevision照合は正本を読む時点だけで、`NoteSourceStore::replace`および`delete`は
期待revisionを受け取らない。同じrevisionからの更新や更新と削除が並行すると、lost updateまたは
正本・投影乖離を起こしうる。

## 実装項目

1. journalの状態、temp file、正本revisionおよび投影revisionを対応付けた、再試行可能な明示的状態機械を
   定義する。prepared操作を安全に取消し、temp fileを除去する復旧portを追加する。
2. 正本変更後の復旧可能・不能を判定し、復旧不能なノートは書込み停止する。自動復旧と隔離はroot監査へ
   記録し、運用者が解消できる情報を提供する。
3. 正本の条件付き置換・削除をportへ導入し、ノート単位の直列化または同等の排他により、比較からrename/
   deleteまでを一つの競合判定にする。
4. create/update/delete、クラッシュ注入、並行update/update、並行update/deleteを実filesystemとSQLiteで
   テストする。

## 完了条件

- 再起動後、未完了操作は取消し・完了・隔離のいずれかに必ず遷移し、temp fileを残さない。
- 成功応答時と復旧完了時の正本・投影・ACLは同じrevisionを表す。
- 同一expected revisionから複数書込みを試みた場合、高々一つだけが成功する。

