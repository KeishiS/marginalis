# 011: アーキテクチャ再基線化

状態: 完了（Web UIおよびMCPは初期公開の範囲から除外し、後続issueで扱う）。

`docs/architecture.md`の目標構成へ、公開前に破壊的移行する。

## 作業順序

1. 現行`marginalis-store`と`marginalis-web`の公開型・実行経路を凍結する。
2. domain/application/adapter/server crateへ責務を分離する。
3. version管理されたsqlx migrationと開発DB破棄方針を導入する。
4. `ServerConfig`、secret入力、Clock/Random portを導入し、環境変数読込をserverだけに閉じる。
5. 確定した`ServerConfig`を用いて、010の最小NixOS moduleとhealth checkを並行して実装する。
6. ファイル正本・操作ジャーナル・ノートCRUDを実装する。
7. OIDC、session、ACL、HTML表示を新application use caseへ移す。
8. 旧crateを削除し、RESTを新server境界へ載せる。初期公開ではWeb UIを提供せず、Web UIとMCPは
   後続段階とする。NixOS moduleにはOIDC secret contractと永続化のVM testを追加する。

## 完了条件

- application層がAxum、sqlx、filesystem、openidconnectへ依存しない。
- migration、空DB、復旧、ファイルと投影の一貫性をintegration testで検証する。
- NixOS moduleが`ServerConfig`とsecret contractだけを介して起動する。
- 旧`NotebookStore`中心の実装を削除する。

## 検証

- `cargo make verify`（format、Nix format、check、clippy、全workspace test、flake評価）
- `nix flake check --no-build`
- NixOS VM test（systemd credentialによるOIDC secretとデータ領域の再起動後保持）
