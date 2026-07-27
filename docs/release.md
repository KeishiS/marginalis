# リリース手順

`v0.5.0`では`/api/v2`、SQLite schema version 4、`marginalis-archive-3`が公開仕様です。
旧database、旧archive、`/api/v1`、ファイル正本、root APIには後方互換性を提供しません。
更新時はserviceを停止し、切戻しが必要なら旧版専用として旧`dataDir`を別領域へ退避します。その後、
配備先の旧`dataDir`全体を削除し、空の`dataDir`から再初期化します。退避したdatabaseは現行版へ
importしません。

1. 作業ブランチで `cargo make verify`、`cargo make openapi-check`、`nix flake check --no-build` を実行する。
2. Kanidm 1.10、TLS、サブパスを使う NixOS E2E を実行する。
3. ChatGPT、Claude Code、Codex CLI の MCP OAuth read/write/revoke を受入確認する。
4. 空databaseからのNixOS配備、backup destination、purge timerを確認する。
   現行運用では、最新archiveを本番から隔離した空databaseへ復元する試験も実施する。
5. OpenAPI、MCP、NixOS、受入文書が同じ仕様を説明していることを確認する。
6. Pull Request を作成し、目的・主な差分・検証結果を記載して rebase auto-merge を設定する。
7. `main`へのマージ後、対象commitでrelease-gate workflowを実行し、成功を確認する。
8. 必須gateが成功した対象commitへrelease tagを作成する。

現行運用では日次backup、30世代保持、四半期の週末復元試験を運用基準とする。復元先は
本番databaseから隔離し、既存databaseを暗黙に上書きしてはならない。詳細は
[Issue 045](../issues/045-backup-restore-lifecycle.md)と
[v0.5.0受入確認](acceptance.md)を参照する。
