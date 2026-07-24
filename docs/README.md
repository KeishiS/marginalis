# 文書案内

このディレクトリには、Marginalis の現行仕様、利用手順、運用手順を収録しています。過去の
リリース内容は [変更履歴](../CHANGELOG.md)、将来の作業は[ロードマップ](roadmap.md)と
[Issue 一覧](../issues/README.md)を正とします。

## 利用者向け

- [REST API リファレンス](rest-api.md): Cookie 認証、CSRF、ノート、検索、アクセス制御、
  `root` 管理
- [OpenAPI 3.1](openapi.json): `/api/v1` の機械可読な仕様
- [MCP と OAuth](mcp.md): MCP ツール、OAuth 認可、クライアント登録、認可の取消

## 運用者向け

- [NixOS での運用](nixos.md): 配備、シークレット、リバースプロキシ、v0.2.0 系列への破壊的
  初期化、バックアップと復元
- [実環境での受入確認](acceptance.md): REST、MCP、バックアップ、復元、監査の確認
- [リリース手順](release.md): 自動検証、実環境での確認、タグ付け、公開

## 設計・開発向け

- [要件定義](requirements.md): 現行の規範要件と、将来も維持する設計要件
- [アーキテクチャ](architecture.md): クレートの責務、依存関係、データ整合性
- [ロードマップ](roadmap.md): 未完了作業の順序と判断時期
- [要件ヒアリング記録](interviews.md): 要件を決めた時点の履歴

## 文書の読み分け

現行の動作について文書間に差がある場合は、機械可読な REST 仕様には `openapi.json`、製品要件
には `requirements.md`、具体的な実装判断には `architecture.md` を優先します。ヒアリング記録と
完了済み Issue は判断当時の履歴であり、現行仕様ではありません。

識別子、HTTP ヘッダー、設定名、コマンド、コード上の型名は原綴りで表記します。それ以外の
一般用語は日本語で説明します。
