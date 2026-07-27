# 文書案内

このディレクトリには、Marginalis の現行仕様、利用手順、運用手順を収録しています。現行の
製品要件は[要件定義](requirements.md)、実装上の不変条件は
[アーキテクチャ](architecture.md)を正とします。REST、MCP、NixOSの詳細は、それぞれの
主題別文書を正とします。過去のリリース内容は[変更履歴](../CHANGELOG.md)、将来の作業は
[ロードマップ](roadmap.md)と[Issue 一覧](../issues/README.md)を正とします。

## 利用者向け

- [OpenAPI 3.1](openapi.json): `/api/v2` の機械可読な仕様
- [MCP と OAuth](mcp.md): MCP ツール、OAuth 認可、クライアント登録、認可の取消
- [browser・MCP protocol回帰試験](protocol-regression.md): 自動回帰、client fixture、失敗証跡

## 運用者向け

- [NixOS での運用](nixos.md): v0.3 設定の要点と日常操作
- [リリース手順](release.md): 自動検証、実環境での確認、タグ付け、公開

## 設計・開発向け

- [GitHubを使う開発手順](development.md): Nix開発環境、`gh`、ブランチ、Pull Request、マージ
- [カバレッジ](coverage.md): 本番到達性検査、coverage reportの生成と読み方
- [セキュリティ](security.md): 認証境界、依存脆弱性監査、例外の根拠
- [要件定義](requirements.md): v0.3 の確定要件
- [アーキテクチャ](architecture.md): クレートの責務、依存関係、データ整合性
- [v0.3.0 再設計判断記録](v0.3.0-design.md): v0.3.0 の設計を確定した時点の非規範snapshot
- [v0.3.0 運用snapshot](v0.3.0-operations.md): v0.3.0 公開候補時点の非規範snapshot
- [連環（Renkan）— 個人研究の記憶を横断する探索基盤](research-search-vision.md): 多様な研究
  データを横断する探索、個人向け学習、研究記録への還流を論じ、Marginalis のあいまい検索を
  第一歩に置く非規範のポジションペーパー
- [ロードマップ](roadmap.md): v0.3.1の運用堅牢化、v0.4.0のMCP執筆支援、条件付き機能の判断時期
- [要件ヒアリング記録](interviews.md): 要件を決めた時点の履歴

## 文書の読み分け

現行の動作について文書間に差がある場合は、機械可読な REST 仕様には `openapi.json`、製品要件
には`requirements.md`、実装上の不変条件には`architecture.md`、運用には`nixos.md`を
優先します。版番号付きsnapshot、ヒアリング記録、将来構想、完了済みIssueは判断時点の記録であり、
現行仕様ではありません。

識別子、HTTP ヘッダー、設定名、コマンド、コード上の型名は原綴りで表記します。それ以外の
一般用語は日本語で説明します。
