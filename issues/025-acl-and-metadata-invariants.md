# 025: ACLと保護済みノートmetadataの不変条件

状態: 実装済み。

## 問題

OIDCユーザ無効化は、対象が唯一の有効管理者であるかを確認しない。ACL変更時の最終管理者数も
無効化済みユーザを数えるため、有効管理者ゼロのノートを作れる。

投影保存と投影再構築は、既存ノートでも作成者へadmin ACLを追加する。このため、管理権限を外された
作成者が後続の保存または再構築で復権する。

さらにsource APIは、クライアントが指定した`created-at`および`updated-at`を保存できる。
既存の`ImmutableNoteMetadata`を更新時の解析へ適用していない。

## 実装項目

1. ユーザ無効化をACL不変条件のtransactionへ含める。各ノートで一人以上の**有効な**adminが残る場合だけ
   無効化を許可する。
2. ACL変更時の最終admin判定を有効ユーザだけに限定する。
3. ACL初期化を新規ノート作成だけに限定し、既存ノートの保存および投影再構築がACLを変更しないよう
   projection replaceを分離する。
4. raw AsciiDoc APIを維持する。createでは入力headerの保護属性をserver生成のnote ID、creator ID、日時で
   置換し、updateではnote ID、creator ID、created-atを固定し、updated-atをserver時刻で置換する。
5. ACL剥奪後の保存・再構築、唯一の有効adminの無効化、metadata改竄を回帰試験へ加える。

## 完了条件

- 有効adminがゼロになる無効化・ACL更新は失敗し、既存状態を変更しない。
- 既存ノートの保存・再構築はACLを一切変更しない。
- クライアント入力だけで保護metadataを作成後に変更できない。
