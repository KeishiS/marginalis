# 資料案内

目的に合う資料を次の一覧から選んでください。

## 利用者向け

- [用語集](glossary.md)
- [OpenAPI 3.1](openapi.json)
- [REST API](rest-api.md)
- [AsciiDoc編集画面](web-ui-editor.md)
- [Web UIの画面設計](web-ui-design.md)
- [MCPとOAuth](mcp.md)
- [MCP toolのJSON Schema](mcp-tools.json)

## 運用者向け

- [NixOSでの運用](nixos.md)
- [ログと障害診断](observability.md)
- [リリース手順](release.md)
- [受入基準と版別結果](acceptance.md)

## 設計・開発向け

- [GitHubを使う開発手順](development.md)
- [文書管理方針](documentation.md)
- [リポジトリ文書のAsciiDoc化評価](repository-asciidoc-evaluation.md)
- [カバレッジ](coverage.md)
- [ログと障害診断](observability.md)
- [セキュリティ](security.md)
- [要件定義](requirements.md)
- [要件と検証の対応表](traceability.md)
- [アーキテクチャ](architecture.md)
- [AdocWeave 0.27移行判断](adocweave-v0.27-migration.md)
- [Auth0をMCPのAuthorization Serverに採用](adr/0001-auth0をmcpのauthorization-serverに採用.md)（廃止。ADR 0007で置き換え）
- [SQLiteを正本として維持](adr/0002-sqliteを正本として維持する.md)
- [CodeMirrorをAsciiDoc編集基盤に採用](adr/0003-codemirrorをasciidoc編集基盤に採用.md)
- [書誌ライブラリーを利用者ごとに管理する](adr/0004-書誌ライブラリーを利用者ごとに管理する.md)
- [認証基盤の停止時にWebとMCPで起動可否を分ける](adr/0005-認証基盤の停止時にwebとmcpで起動可否を分ける.md)
- [引用はノート作成者の書誌ライブラリーで解決する](adr/0006-引用はノート作成者の書誌ライブラリーで解決する.md)
- [MCPのAuthorization Serverを内蔵する](adr/0007-mcpのauthorization-serverを内蔵する.md)
- [MCP Authorization Serverの中核を製品から分離する](adr/0008-mcp-authorization-serverの中核を製品から分離する.md)
- [MCP Authorization Serverの保守](mcp-authorization-server-maintenance.md)
- [MCPのscope上限を利用者設定として管理する](adr/0009-mcpのscope上限を利用者設定として管理する.md)
- [ブラウザーとMCPプロトコルの回帰テスト](protocol-regression.md)

## 過去の記録と今後の計画

- [変更履歴](../CHANGELOG.md)
- [ロードマップ](roadmap.md)
- [MCP向けAuthorization Serverの評価手順](mcp-authorization-server-evaluation.md)
- [GitHub Issues](https://github.com/KeishiS/marginalis/issues)
- [旧ローカルIssue移行対応表](issue-migration.md)
- [要件ヒアリング記録](interviews.md)
- [AdocWeave 0.11移行判断](adocweave-v0.11-migration.md)
- [AdocWeave 0.17移行判断](adocweave-v0.17-migration.md)
- [AdocWeave 0.19移行判断](adocweave-v0.19-migration.md)
- [AdocWeave 0.20移行判断](adocweave-v0.20-migration.md)
- [AdocWeave 0.22移行判断](adocweave-v0.22-migration.md)
- [AdocWeave 0.23移行判断](adocweave-v0.23-migration.md)
- [v0.3.0再設計判断記録](v0.3.0-design.md)
- [v0.3.0運用記録](v0.3.0-operations.md)
- [連環（Renkan）— 個人研究の記憶を横断する探索基盤](research-search-vision.md)

## 文書の読み分け

現行の動作について資料間に差がある場合は、REST APIには`openapi.json`、システム要件には
`requirements.md`、設計には`architecture.md`、NixOSでの運用には`nixos.md`を優先します。

バージョン番号が付いた資料、ヒアリング記録、将来構想、完了済みIssueは、作成時点の記録です。
現在の仕様を確認する目的では使用しません。
