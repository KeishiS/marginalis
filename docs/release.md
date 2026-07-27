# リリース手順

この文書は、版に依存しない公開手順を定めます。対象版の互換性、移行、schema、archiveは
[変更履歴](../CHANGELOG.md)と[受入確認](acceptance.md)を正とします。破壊的リリースではserviceを
停止し、必要な退避を行った後、対象版の手順どおりに`dataDir`を初期化します。

1. 作業ブランチで `cargo make verify`、`cargo make openapi-check`、`nix flake check --no-build` を実行する。
2. Kanidm 1.10、TLS、サブパスを使う NixOS E2E を実行する。
3. ChatGPT、Claude Code、Codex CLI の MCP OAuth read/write/revoke を受入確認する。
4. 空databaseからのNixOS配備、backup destination、purge timerを確認する。
   現行運用では、最新archiveを本番から隔離した空databaseへ復元する試験も実施する。
5. OpenAPI、MCP、NixOS、受入文書が同じ仕様を説明していることを確認する。
6. Pull Request を作成し、目的・主な差分・検証結果を記載して rebase auto-merge を設定する。
7. `main`へのマージ後、`main`の先端でrelease-gate workflowを手動実行する。`release_tag`には
   作成予定のタグを入力し、成功を確認する。
8. 必須gateが成功した`main`の先端へrelease tagを作成する。タグのpushで再実行される
   release-gateも成功することを確認する。

現行運用では日次backup、30世代保持、四半期の週末復元試験を運用基準とします。復元先は
本番databaseから隔離し、既存databaseを暗黙に上書きしてはなりません。詳細は
[NixOSでの運用](nixos.md)と[受入確認](acceptance.md)を参照してください。
