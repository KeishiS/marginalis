# 028: current release契約と保守実装の再整合

状態: RC.1 release blocker（文書契約）。

## 問題

`requirements.md`には、現在の方針と異なる契約が残る。具体的には、監査を一般ユーザのノート操作・
MCP操作まで含める記述、`dataDir/<creator-user-uid>/<note-uid>.adoc`の三段配置、監査閲覧API、
設定可能な各種上限などである。実装とarchitectureは、current releaseではroot監査を365日保持し、
`dataDir/notes/<note-id>.adoc`をdata format v1とする。

また、`parse_note_projection`が解析失敗を空のdiagnostic集合で返すため、利用者とログが失敗理由を区別できない。
AdocWeaveのruntime contract検証はtestだけで、本番起動時には実行されない。projection INSERTも通常保存と
全件再構築で重複している。

## 実装項目

1. requirements、architecture、OpenAPIおよびissueのcurrent/next/future境界を再照合する。root限定監査、
   file layout v1、audit閲覧経路なしをcurrent releaseの意図として明記するか、実装へ戻すかを決定する。
2. AdocWeave runtime contractを起動時にfail closedで検証し、解析失敗を空でない安全なdiagnosticとして返す。
3. projectionの共通挿入処理を一つの内部操作へ集約し、通常保存と再構築の差（ACL初期化を含む）を明示する。
4. 未使用依存と依存version分裂を棚卸しし、不要なものを削除する。同期filesystem I/Oは負荷測定の上で
   `spawn_blocking`等の境界へ移す。

## 完了条件

- 文書だけを読んでもcurrent releaseのデータ形式、監査範囲、設定可能項目が実装と一致する。
- 起動時のAdocWeave契約不一致と解析失敗を、秘密を出さずに検出できる。
- projection更新の共通部分は一箇所で検証される。

