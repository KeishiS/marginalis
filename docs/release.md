# リリース手順

v0.3 では `/api/v2` と SQLite canonical archive が公開契約です。v0.2 の `/api/v1`、ファイル正本、
root API には後方互換性を提供しません。

1. 作業ブランチで `cargo make verify`、`cargo make openapi-check`、`nix flake check --no-build` を実行する。
2. Kanidm 1.10、TLS、サブパスを使う NixOS E2E を実行する。
3. ChatGPT、Claude Code、Codex CLI の MCP OAuth read/write/revoke を受入確認する。
4. 空 database からの NixOS 配備、backup destination、purge timer を確認する。
5. OpenAPI、MCP、NixOS、受入文書が同じ契約を説明していることを確認する。
6. Pull Request を作成し、目的・主な差分・検証結果を記載して rebase auto-merge を設定する。
7. `main` へのマージと必須 gate 成功後に、対象 commit へ release tag を作成する。

復元試験、archive 保存世代、保存先の継続検証は v0.3 公開後に release gate へ追加する改善項目です。
