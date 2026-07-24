# 006: ブラウザー編集プレビュー

状態: 未着手。Web UIを公開する段階で実装する。

## 概要

AdocWeave WASMとWeb Workerを用いて、編集中のノートへ画面を停止させない診断と
プレビューを提供する。

## 対象範囲

- AdocWeaveのbrowser packageを静的配布物として固定する。
- Workerへ単調増加する`version`と`generation`を渡し、古い解析結果を画面へ反映しない。
- cross-origin isolationを利用できる環境では協調キャンセルを行い、利用できない環境では
  Workerを再生成する。
- 編集中は`permissive`モードで解析する。
- 現在の利用者に対する参照解決結果だけをサーバーから受け取り、`RenderInputs`としてWorkerへ渡す。
- Workerの出力を無条件に`innerHTML`へ渡さず、HTML契約、サニタイズおよび表示sandboxを適用する。

## 対象外

- ブラウザーからの直接DB照会、ACL判定およびノートファイル操作。
- 差分パーサーの導入。

## 完了条件

- 日本語、絵文字およびCRLFを含む編集で、診断位置とプレビューがサーバー側と一致する。
- 高頻度の更新、取消し、Workerの異常終了および入力上限超過でUIスレッドを停止させない。
- WASMとRust実装のAST、診断、HTMLおよび投影が、同じテストデータで一致する。

## 関連Issue

- [Issue 001](001-adocweave-dependency-and-contract.md)
- [Issue 002](002-note-profile-and-metadata.md)
- [Issue 003](003-note-references-and-resolver.md)
- [Issue 004](004-safe-rendering-and-presentation.md)
- AdocWeave v0.6.1への移行: [Issue 029](029-adocweave-v0.6.1-migration.md)
