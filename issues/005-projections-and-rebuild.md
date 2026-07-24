# 005: 検索・グラフ投影と再構築

状態: 進行中。同期投影、物理削除への追従、起動時の復旧、およびすべての原本からの
原子的な再構築は実装済みである。AdocWeave v0.6.1に伴う識別子とデータ形式の移行は
[Issue 029](029-adocweave-v0.6.1-migration.md)で扱う。

## 概要

一つのAdocWeave解析結果から、検索、グラフおよびHTML表示に必要なアプリケーション固有の
投影を構築する。

## 対象範囲

- 文書タイトル、タグ、本文、コード、LaTeXソース、アンカーおよび参照を抽出する。
- AdocWeaveの汎用`DocumentProjection`を再解析せずに利用し、数式とノート属性だけを
  ホスト側で補う。
- `sqlx`を通じて、ノートごとの検索投影、参照位置、グラフ辺および逆参照をSQLiteへ保存する。
- AsciiDocファイルだけからノード情報と有向グラフを再構成する。
- AdocWeaveのパッケージ版、構文設定、URLポリシー、Resolver結果、ACLおよび対象文書の
  リビジョンに応じて、必要なキャッシュと投影を無効化する。
- 再構築に成功した場合だけ、トランザクションで既存の投影を置換する。

## 対象外

- ベクトル検索、意味検索および外部リンク先の代理取得。

## 完了条件

- 本文、タグ、コード、LaTeXソースおよび参照元・参照先を検索できる。
- 原本の更新、削除または外部変更後に再構築しても、古い投影が残らない。
- 解析、ResolverまたはDB更新が失敗しても、最後に成功した投影を破壊しない。
- AdocWeaveのパッケージ版変更に伴う再構築範囲を統合試験で検証する。

## 実施記録

- `marginalis rebuild-projections`は、`notes/`にあるすべてのUUIDv7 `.adoc`原本を列挙し、
  UTF-8、ノートプロファイル、ファイル名と`note-id`の一致を全件確認してからSQLite投影を置換する。
- NixOSモジュールは`marginalis-rebuild-projections.service`を提供する。このoneshot unitは
  HTTPサーバーと排他的に動作し、同じsystemd credentialとsandbox条件を使用する。
- 再構築では検索、アンカーおよび位置付きxrefを置換する。既存ノートのACLを維持し、原本から
  消えたノートだけをACLとともに削除する。解析またはDB更新が失敗した場合は、トランザクションにより
  最後に成功した投影を保持する。

## 関連Issue

- [Issue 002](002-note-profile-and-metadata.md)
- [Issue 003](003-note-references-and-resolver.md)
- [Issue 004](004-safe-rendering-and-presentation.md)
- AdocWeave v0.6.1への移行: [Issue 029](029-adocweave-v0.6.1-migration.md)
