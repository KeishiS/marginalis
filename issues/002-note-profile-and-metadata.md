# 002: ノート用AsciiDocプロファイルと属性検証

位置付け: ノートの保存時および編集中に適用する恒久要件を定める。AdocWeave v0.6.1の
公開APIへの適合は[Issue 029](029-adocweave-v0.6.1-migration.md)で扱う。

## 概要

本アプリケーションが保存するノートの文書ヘッダーと本文を、AdocWeaveによる解析後に
一貫した規則で検証する。

## 対象範囲

- `note-id`、`creator-id`、`created-at`、`updated-at`および`tags`を必須属性として検証する。
- UUIDv7、ミリ秒精度に固定したUTCのRFC 3339日時、タグの正規化、重複除去および上限を検証する。
- `note-id`、`creator-id`および`created-at`を、作成後に変更できない保護属性として扱う。
- 明示アンカーと見出しIDを抽出し、重複するアンカーを診断する。
- 保存時の`strict`モードと、編集中の`permissive`モードで用いる診断の重大度を定める。
- 属性名と値の正確なUTF-8バイト範囲を用いて、位置が安定した診断を返す。

## 対象外

- ACL、現在の管理者およびリビジョンをAsciiDocへ保存すること。
- ノート参照の実在確認。

## 完了条件

- 正常なノートヘッダーと、必須属性の欠落、形式不正および変更禁止違反をテストデータで検証する。
- Formatterが属性の原文を破壊せず、正規化した値だけを投影へ使用する。
- サーバーの保存時検証とブラウザーの編集中診断が、同じプロファイル規則を使用する。

## 関連Issue

- 依存固定: [Issue 001](001-adocweave-dependency-and-contract.md)
- 保護属性の実装: [Issue 025](025-acl-and-metadata-invariants.md)
- AdocWeave v0.6.1への移行: [Issue 029](029-adocweave-v0.6.1-migration.md)
