# 044: ソース配置と文書境界の整理

## 状態

着手済み。単一利用だったMCP wire crateは`marginalis-web::mcp`へ統合した。残る大規模な
production fileと規範文書を、機能拡充前の横断的な保守性改善として、振る舞いを変えず
責務単位のmoduleへ分割する。

## 背景

v0.3の破壊的再設計で旧経路と互換adapterは除去できた。一方、現行実装は
`marginalis-web/src/http.rs`、`marginalis-server/src/lib.rs`、
`marginalis-sqlite/src/lib.rs`へ複数の責務が集まり、変更時に読む範囲と競合範囲が広い。
逆に、利用者が一つしかない小さなMCP wire crateは独立境界にする効果がなく、HTTP
transportへ統合した。

crateは依存方向や再利用単位を隔離するときだけ増やし、単なるファイル整理には
crate内moduleを使う。

## 実装方針

1. `marginalis-web`はrouterを一か所で一覧できる状態を保ち、`auth`、`oauth`、`mcp`、
   `notes`、`ui`、`security`へhandlerとrequest/response型を分ける。
2. `marginalis-server`は`config`、`notes`、`session`、`oidc`、`mcp_oauth`、実行環境の
   `clock/random`へ分ける。外部crateへ公開する型は`lib.rs`から明示的に再exportする。
3. `marginalis-sqlite`は接続・schema、note、session/OIDC、MCP OAuth、archiveに分ける。
   transactionをまたぐprivate helperは所有するmoduleを一つに決める。
4. testは対象moduleの近くへ置き、HTTP/OIDC/MCPを通す結合試験だけを
   `marginalis-integration-tests`へ残す。
5. `docs/architecture.md`を現行責務の短い入口にする。版番号付き設計書と運用書は
   v0.3公開後に履歴へ移し、現行の規範文書を`requirements.md`、`architecture.md`、
   `nixos.md`、`mcp.md`、`rest-api.md`へ一本化する。
6. 移動と意味変更を同じcommitに混在させない。各段階でdependency boundary、unit test、
   integration testを実行する。

## 完了条件

- production sourceの単一ファイルが、HTTP、OAuth、永続化など複数の変更理由を持たない。
- router、composition root、公開re-exportを見れば、実行経路と依存方向を追跡できる。
- wire型だけの単一利用crateや、空の互換用directoryが残らない。
- 現行仕様の同じ規則を複数文書で重複して定義せず、規範文書と履歴文書を判別できる。
- `cargo make verify`と該当するNixOS VM試験が成功し、公開HTTP/OpenAPI契約に差分がない。
