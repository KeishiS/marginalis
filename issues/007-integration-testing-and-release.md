# 007: 結合試験とリリース検証

## 目的

AdocWeave拡張を含むAsciiDoc処理全体の安全性、決定性および互換性を継続的に検証する。

## 範囲

- ノート用プロファイルの正常系、異常系、境界値、回復およびセキュリティfixtureを整備する。
- native Rust、WASM Worker、Web APIおよびMCPが同じ解析・診断・投影規則を使うことを検証する。
- 初期段階では、同じ入力と既定policyに対するnative HTMLとWASM HTMLの一致をRustテストで
  固定する。Resolver URL policyおよび表示上書きは、対応する上流APIが提供された後に同じ
  fixtureへ追加する。
- パーサー、属性検証、参照解決、URL policy、HTML、位置変換およびFormatterのproperty testとfuzzingを行う。
- `format(format(x)) == format(x)`と、整形前後の意味同値性を検証する。
- 上流AdocWeave更新時に、契約version、golden HTML、projection、DB再構築およびブラウザ資産を検証する。

## 完了条件

- テストに、メタデータ、`xref:note:`、リンク切れ、ACL、LaTeX、コード、
  危険URL、raw HTML、深い入れ子および巨大入力が含まれる。
- 任意UTF-8入力と不正バイト列が、プロセス異常終了や機密情報のログ出力を引き起こさない。
- CIが依存固定、Rust test、WASM適合試験、browser smoke test、fuzz/property testおよび
  migration検証を実行する。

## 依存関係

- 001
- 002
- 003
- 004
- 005
- 006

## 2026-07-23時点の実環境受入確認

- unit・HTTP結合試験と実環境確認を混同しない。実provider・reverse proxy・実MCP clientを通す手順は
  `docs/acceptance.md`で管理する。
- REST CRUD/検索と実MCP clientのOAuth・tool相互運用は、認証済み利用者またはclientの選択を必要とする
  手動受入確認として残す。
- backup、非破壊restore候補、投影再構築、root auditの手順は実装済みであり、保存先・保持世代・実際の
  dataDir切替は運用policyとして明示的に判断する。
