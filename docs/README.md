# 文書案内

このディレクトリには、Marginalis の現行仕様、利用手順、運用手順を収録しています。v0.3.0 では
SQLite 正本、Kanidm group 認可、`/api/v2` を現行契約とします。過去の
リリース内容は [変更履歴](../CHANGELOG.md)、将来の作業は[ロードマップ](roadmap.md)と
[Issue 一覧](../issues/README.md)を正とします。

## 利用者向け

- [v0.3.0 運用契約](v0.3.0-operations.md): Kanidm、NixOS、MCP の現行運用手順
- [OpenAPI 3.1](openapi-v3.json): `/api/v2` の機械可読な仕様
- [MCP と OAuth](mcp.md): MCP ツール、OAuth 認可、クライアント登録、認可の取消

## 運用者向け

- [NixOS での運用](nixos.md): v0.2 の履歴。v0.3 は運用契約を参照
- [リリース手順](release.md): 自動検証、実環境での確認、タグ付け、公開

## 設計・開発向け

- [GitHubを使う開発手順](development.md): Nix開発環境、`gh`、ブランチ、Pull Request、マージ
- [要件定義](requirements.md): v0.2 の履歴。v0.3 の規範は再設計仕様を参照
- [アーキテクチャ](architecture.md): クレートの責務、依存関係、データ整合性
- [v0.3.0 再設計仕様](v0.3.0-design.md): SQLite 正本、Kanidm group 認可、新 API の確定設計
- [研究データ横断検索ハブの将来構想](research-search-vision.md): 複数データソースの横断検索、
  pgvector、個人向け学習、研究メモへの還流に関する非規範の長期構想
- [ロードマップ](roadmap.md): 未完了作業の順序と判断時期
- [要件ヒアリング記録](interviews.md): 要件を決めた時点の履歴

## 文書の読み分け

現行の動作について文書間に差がある場合は、機械可読な REST 仕様には `openapi-v3.json`、製品要件
と具体的な実装判断には `v0.3.0-design.md` を優先します。ヒアリング記録、
将来構想、完了済み Issue は判断当時の記録であり、現行仕様ではありません。

識別子、HTTP ヘッダー、設定名、コマンド、コード上の型名は原綴りで表記します。それ以外の
一般用語は日本語で説明します。
