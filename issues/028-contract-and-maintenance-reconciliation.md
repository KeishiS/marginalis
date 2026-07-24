# 028: v0.1.0仕様と保守処理の整合

## 状態

2026-07-23に完了した。

## 背景

`requirements.md` には、現在の方針と異なる仕様が残る。具体的には、監査を一般ユーザーのノート操作・
MCP操作まで含める記述、`dataDir/<creator-user-uid>/<note-uid>.adoc`の三段配置、監査閲覧API、
設定可能な各種上限などである。実装とarchitectureは、current releaseではroot監査を365日保持し、
`dataDir/notes/<note-id>.adoc`を保存形式v1とする。

また、`parse_note_projection`は解析失敗時に空の診断一覧を返すため、利用者とログが失敗理由を
区別できない。AdocWeaveの実行時の版検証はテストでしか行っていない。検索用データを保存する処理も、
通常保存と全件再構築で重複している。

## 作業内容

1. requirements、architecture、OpenAPIおよびissueのcurrent/next/future境界を再照合する。root限定監査、
   file layout v1、audit閲覧経路なしをcurrent releaseの意図として明記するか、実装へ戻すかを決定する。
2. AdocWeaveの版を起動時に検証する。不一致は安全側に停止し、解析失敗時は安全な診断を返す。
3. 検索用データの共通保存処理を一つにまとめ、通常保存と再構築の差（ACL初期化を含む）を明示する。
4. 未使用依存と依存version分裂を棚卸しし、不要なものを削除する。同期filesystem I/Oは負荷測定の上で
   `spawn_blocking`等の境界へ移す。

## 完了条件

- 文書だけを読んでもcurrent releaseのデータ形式、監査範囲、設定可能項目が実装と一致する。
- 起動時の AdocWeave バージョン不一致と解析失敗を、秘密を出さずに検出できる。
- 検索用データを更新する共通処理は一箇所で検証される。

## 実施結果

- `requirements.md`を現在・次期・将来の区分、保存形式v1、root監査、バックアップ・復元、
  実装済みの検索条件に合わせた。未導入の数値上限は要件から除外した。
- `marginalis-service` は起動前に AdocWeave の実行時バージョンを検証し、解析失敗は安全な空でない
  diagnosticとして扱う。
- SQLiteの通常保存と検索用データの再構築は、共通の行挿入処理を利用する。
