# 011: アーキテクチャの再設計

状態: 完了（Web UIおよびMCPは初期公開の範囲から除外し、後続Issueで扱う）。

`docs/architecture.md`の目標構成へ、公開前に破壊的移行する。

## 実施内容

1. 旧`marginalis-store`と`marginalis-web`の公開型・実行経路を固定した。
2. domain/application/adapter/serverの各クレートへ責務を分離した。
3. バージョン管理されたsqlx migrationと開発DBの破棄方針を導入した。
4. `ServerConfig`、secret入力、Clock/Random portを導入し、環境変数の読込みをserverへ集約した。
5. 確定した`ServerConfig`を用いて、Issue 010の最小NixOSモジュールとhealth checkを実装した。
6. ファイル原本、操作ジャーナルおよびノートCRUDを実装した。
7. OIDC、session、ACLおよびHTML表示を新しいapplication use caseへ移した。
8. 旧crateを削除し、RESTを新しいserver境界へ載せた。Web UIとMCPは後続Issueへ移管した。

## 完了条件

- application層がAxum、sqlx、filesystem、openidconnectへ依存しない。
- migration、空DB、復旧、ファイルと投影の一貫性をintegration testで検証する。
- NixOS module が `ServerConfig` と秘密情報の受け渡し仕様だけを介して起動する。
- 旧`NotebookStore`中心の実装を削除する。

## 検証

- `cargo make verify`（format、Nix format、check、clippy、全workspace test、flake評価）
- `nix flake check --no-build`
- NixOS VM test（systemd credentialによるOIDC secretとデータ領域の再起動後保持）
