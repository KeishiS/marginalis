# 044: ソース配置と文書境界の整理

## 状態

着手済み。単一利用だったMCP wire crateは`marginalis-web::mcp`へ統合し、
`marginalis-server`は公開facade、設定、ノート、session、OIDC、MCP OAuth、実行環境へ
分割した。`marginalis-web`は共有状態、browser認証、security policy、REST note、閲覧UI、
MCP transportへ分割した。SQLiteのschema、Web session、OIDC login attemptも独立moduleへ
移した。規範文書の正本も主題別文書へ一本化した。残るSQLite note/archive/MCP、Webの
MCP OAuth handler、moduleごとのtest配置を整理する。

## 背景

v0.3の破壊的再設計で旧経路と互換adapterは除去できた。一方、現行実装は
`marginalis-web/src/http.rs`、`marginalis-server/src/lib.rs`、
`marginalis-sqlite/src/lib.rs`へ複数の責務が集まり、変更時に読む範囲と競合範囲が広い。
逆に、利用者が一つしかない小さなMCP wire crateは独立境界にする効果がなく、HTTP
transportへ統合した。

crateは依存方向や再利用単位を隔離するときだけ増やし、単なるファイル整理には
crate内moduleを使う。

## 横断レビュー

2026-07-25時点で、次を優先して解消する。

1. `marginalis-web/src/http.rs`はrouter、OIDC、MCP OAuth、MCP JSON-RPC、REST、HTML UI、
   Cookie/CSRFを2,000行以上の一ファイルに持つ。公開route一覧をcomposition rootへ残し、
   handlerとwire型を変更理由ごとに分割する。
2. `marginalis-sqlite/src/lib.rs`はschema、MCP OAuth、note/ACL、archiveと大半のtestを
   1,500行以上の一ファイルに持つ。transaction境界を崩さず、永続化対象ごとに分割する。
   AsciiDoc archiveの意味検証はSQLite adapterの責務ではないため、application/server側へ
   移し、SQLiteは検証済みarchiveの原子的な格納に限定する。
3. `marginalis-server`のproduction codeは責務別になったが、`mcp_oauth.rs`に他moduleのtestが
   残る。各testを対象moduleへ移し、HTTPを通す試験だけをintegration crateへ残す。
4. `marginalis-service/src/main.rs`はcomposition rootとmaintenance CLIを兼ねる。現状の規模では
   crate分割は不要だが、引数解釈を`cli`、HTTP組立を`serve`、保守処理を`maintenance`へ分ける。
5. `docs/v0.3.0-design.md`、`docs/v0.3.0-operations.md`と主題別文書が同じ規則を重複している。
   現行の正本は`requirements.md`、`architecture.md`、`nixos.md`、`mcp.md`、`rest-api.md`とし、
   版付き文書は公開時点の非規範snapshotとしてだけ残す。

`domain`、`application`、OIDC adapter、SQLite adapter、HTTP adapter、composition rootという
crate境界と依存方向は維持する。flatなIssue配置も番号による検索とリンク安定性を優先して維持し、
完了・現行・将来の区別は`issues/README.md`で行う。ディレクトリ階層を増やすこと自体を目的にしない。

## 実装方針

1. `marginalis-web`はrouterを一か所で一覧できる状態を保ち、`auth`、`oauth`、`mcp`、
   `notes`、`ui`、`security`へhandlerとrequest/response型を分ける。
2. `marginalis-server`は`config`、`notes`、`session`、`oidc`、`mcp_oauth`、実行環境の
   `clock/random`へ分ける。外部crateへ公開する型は`lib.rs`から明示的に再exportする。
3. `marginalis-sqlite`は接続・schema、note、session/OIDC、MCP OAuth、archiveに分ける。
   transactionをまたぐprivate helperは所有するmoduleを一つに決める。
4. testは対象moduleの近くへ置き、HTTP/OIDC/MCPを通す結合試験だけを
   `marginalis-integration-tests`へ残す。
5. `docs/architecture.md`を現行責務の短い入口にする。現行の規範文書を
   `requirements.md`、`architecture.md`、`nixos.md`、`mcp.md`、`rest-api.md`へ一本化し、
   版番号付き設計書と運用書は非規範snapshotとして扱う。
6. 移動と意味変更を同じcommitに混在させない。各段階でdependency boundary、unit test、
   integration testを実行する。

## 完了条件

- production sourceの単一ファイルが、HTTP、OAuth、永続化など複数の変更理由を持たない。
- router、composition root、公開re-exportを見れば、実行経路と依存方向を追跡できる。
- wire型だけの単一利用crateや、空の互換用directoryが残らない。
- 現行仕様の同じ規則を複数文書で重複して定義せず、規範文書と履歴文書を判別できる。
- SQLite adapterからAsciiDoc parserへの依存がなく、archiveの意味検証と格納責務が分かれる。
- `cargo make verify`と該当するNixOS VM試験が成功し、公開HTTP/OpenAPI契約に差分がない。
