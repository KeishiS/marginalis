# 006: ブラウザ編集プレビュー

## 目的

AdocWeave WASMとWeb Workerを用い、入力中のノートへ非ブロッキングな診断とプレビューを提供する。

## 範囲

- AdocWeave browser packageをアプリの静的資産として固定・配布する。
- Workerへ単調増加versionとgenerationを渡し、古い解析結果を画面へ反映しない。
- cross-origin isolationが利用できる環境では協調キャンセル、利用できない環境ではWorker再生成を行う。
- 編集中はpermissive modeで解析する。
- 現在の利用者に対する参照解決結果だけをサーバから受け取り、`RenderInputs`としてWorkerへ渡す。
- Worker出力を無条件に`innerHTML`へ渡さず、HTML契約、サニタイズおよび表示sandboxを適用する。

## 範囲外

- ブラウザからの直接DB照会、ACL判定またはノートファイル操作。
- 差分パーサーの導入。

## 完了条件

- 日本語、絵文字、CRLFを含む編集で、診断位置とプレビューがサーバ側と一致する。
- 高頻度更新、取消、Worker異常終了および入力上限超過でUIスレッドを停止させない。
- WASMとnativeのAST、診断、HTML、projectionが同じfixtureで一致する。

## 依存関係

- 001
- 002
- 003
- 004
