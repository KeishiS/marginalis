# リリース手順

`v0.4.0`では`/api/v2`、SQLite schema version 3、`marginalis-archive-2`が公開仕様です。
旧database、旧archive、`/api/v1`、ファイル正本、root APIには後方互換性を提供しません。

1. 作業ブランチで `cargo make verify`、`cargo make openapi-check`、`nix flake check --no-build` を実行する。
2. Kanidm 1.10、TLS、サブパスを使う NixOS E2E を実行する。
3. ChatGPT、Claude Code、Codex CLI の MCP OAuth read/write/revoke を受入確認する。
4. 空databaseからのNixOS配備、backup destination、purge timerを確認する。
   v0.3.1以降は、最新archiveを本番から隔離した空databaseへ復元する試験も実施する。
5. OpenAPI、MCP、NixOS、受入文書が同じ仕様を説明していることを確認する。
6. Pull Request を作成し、目的・主な差分・検証結果を記載して rebase auto-merge を設定する。
7. `main` へのマージと必須 gate 成功後に、対象 commit へ release tag を作成する。

`v0.3.1`では日次backup、30世代保持、四半期の週末復元試験を運用基準とする。復元先は
本番databaseから隔離し、既存databaseを暗黙に上書きしてはならない。詳細は
[Issue 045](../issues/045-backup-restore-lifecycle.md)と
[v0.4.0受入確認](acceptance.md)を参照する。
