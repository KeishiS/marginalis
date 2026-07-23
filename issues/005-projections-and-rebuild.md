# 005: 検索・グラフ投影と再構築

状態: 実装中。同期投影、物理削除追従、起動時recoveryおよび正本全件からの原子的な再構築は実装済み。
契約version別の部分無効化と統合試験は後続である。

## 目的

一つのAdocWeave解析結果から、検索、グラフ、HTML表示に必要なアプリ固有projectionを構築する。

## 範囲

- 文書タイトル、タグ、本文、コード、LaTeXソース、アンカーおよび参照を抽出する。
- AdocWeaveの汎用`DocumentProjection`を再解析せずに利用し、数式とノート属性だけを
  ホスト側で補完する。
- `sqlx`を通じて、ノートごとの検索projection、参照位置、グラフ辺、逆参照をSQLiteへ保存する。
- AsciiDocファイルだけからノード情報と有向グラフを再構成する処理を実装する。
- AdocWeave契約version、構文設定、URL policy、Resolver結果、ACLおよび対象文書versionに応じて、
  必要なcache／projectionだけを無効化する。
- 再構築に成功した場合だけ、トランザクションで既存projectionを置換する。

## 範囲外

- ベクトル検索、意味検索および外部リンク先の代理取得。

## 完了条件

- 本文、タグ、コード、LaTeXソースおよび参照元・参照先を検索できる。
- 元ファイルの更新、削除、外部変更後の明示的再構築で古いprojectionが残らない。
- 解析・Resolver・DB更新のいずれかが失敗しても、最後に成功したprojectionを破壊しない。
- 契約version別の再構築範囲を統合試験で検証する。

## 依存関係

- 002
- 003
- 004

## 2026-07-23時点の実装状況

- `marginalis rebuild-projections`は`notes/`の全UUIDv7 `.adoc`正本を列挙し、UTF-8、ノートprofile、
  ファイル名と`note-id`の一致を全件確認してからSQLite投影を置換する。
- NixOS moduleは`marginalis-rebuild-projections.service`を提供する。このoneshot unitはHTTP serverと
  競合し、同じsystemd credentialとsandbox条件で実行される。
- 再構築では検索、anchor、位置付きxrefを置換する。既存ノートのACLを維持し、正本から消えたノートだけを
  ACLごと削除する。解析またはDB更新が失敗した場合、transactionにより最後の成功したprojectionを保持する。
