# 007: AsciiDoc連携の統合試験とリリース検証

位置付け: AsciiDoc処理全体の検証要件を定める。現在のリリース判定は
[Issue 021](021-test-architecture-and-release-gates.md)、E2E自動化は
[Issue 030](030-end-to-end-test-automation-readiness.md)で管理する。

## 概要

AdocWeave連携を含むAsciiDoc処理全体の安全性、決定性および互換性を継続的に検証する。

## 検証範囲

- ノート用プロファイルの正常系、異常系、境界値、回復およびセキュリティのテストデータを整備する。
- Rust実装、WASM Worker、Web APIおよびMCPが同じ解析、診断および投影規則を使用することを検証する。
- 同じ入力と既定のポリシーに対するRust実装とWASMのHTMLを比較する。
- パーサー、属性検証、参照解決、URLポリシー、HTML、位置変換およびFormatterを、
  property testとfuzzingで検証する。
- `format(format(x)) == format(x)`と、整形前後の意味が同じであることを検証する。
- AdocWeave更新時に、パッケージ版、golden HTML、投影、DB再構築およびブラウザー配布物を検証する。

## 完了条件

- テストにメタデータ、`xref:note:`、リンク切れ、ACL、LaTeX、コード、危険なURL、raw HTML、
  深い入れ子および巨大な入力が含まれる。
- 任意のUTF-8入力と不正バイト列が、プロセスの異常終了や機密情報のログ出力を引き起こさない。
- CIが依存固定、Rust test、WASM適合試験、browser smoke test、fuzz/property testおよび
  移行検証を実行する。

## 実施記録

- 単体試験、HTTP統合試験および実環境確認の役割を分けた。実プロバイダー、reverse proxyおよび
  実MCPクライアントを通す手順は`docs/acceptance.md`で管理する。
- REST CRUD・検索と実MCPクライアントのOAuth相互運用は、認証済み利用者またはクライアントの
  選択を要する手動受入として残した。
- backup、非破壊restore候補、投影再構築および`root`監査の手順を実装した。保存先、保持世代および
  実際の`dataDir`切替は運用ポリシーとして判断する。
- `cargo make release-gate`は通常の品質確認に、OpenAPI、GitHub Actions構文、NixOS VMおよび
  release package buildを加える。実行内容とタグ作成手順は`docs/release.md`で管理する。
- 実Kanidm、reverse proxyおよび実MCPクライアントを使う確認は`docs/acceptance.md`と
  [Issue 022](022-v0.1.0-rc.1-release-acceptance.md)へ移管した。

## 関連Issue

- リリース判定: [Issue 021](021-test-architecture-and-release-gates.md)
- E2E自動化: [Issue 030](030-end-to-end-test-automation-readiness.md)
