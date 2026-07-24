# 017: 依存境界を強制するアーキテクチャ再設計 v2

状態: 完了。

## 目的

HTTPとMCPの入出力層がアプリケーション層のポートだけに依存する構成へ移行する。
SQLite、ファイルシステム、AsciiDoc、OIDC、HTTPクライアントの具体的なアダプターを、
本番用の依存関係から除外する。

## 移行前の問題

- `marginalis-web`はテスト補助機能のために、`marginalis-sqlite`、`marginalis-files`、
  `marginalis-server`等へ本番環境でも依存していた。設計上の依存規則をCargoが保証できない。
- `marginalis-server`が、設定の読込、アダプターの組立、ノート、認証、MCP OAuthを同時に担う。
- バックアップと再構築が、OIDCの秘密情報を含む`ServerConfig`に依存する。
  ストレージの保守だけを単独で実行できない。

## 目標構成

```text
ドメイン ── アプリケーション ── Web / MCP
              │
              ├── SQLite / ファイル / AsciiDoc / OIDC アダプター
              │
              └── 実行環境（設定・組立・起動）

保守コマンド ── ストレージ設定 + 各ストレージアダプター
結合試験 ── 実行環境 + 具体的なアダプター
```

クレート名よりも、入出力層から具体的なアダプターへの依存を禁止することを優先する。

## 実施内容

1. アプリケーションAPIを、ノート、利用者とセッション、管理、MCP OAuthの機能別に分割した。
2. `marginalis-web`と`marginalis-mcp`の依存先を、アプリケーション層、ドメイン層、
   公開契約に限定した。
3. 具体的なアダプターを使うHTTP・MCP試験を`marginalis-integration-tests`へ移した。
4. `StorageConfig`、`HttpConfig`、`OidcConfig`、秘密情報の設定を分離した。
5. バックアップ、再構築、復元、監査ログの整理を、OIDC設定なしで動く保守処理へ移した。
6. 禁止した依存関係をCIで検査する`cargo make dependency-boundaries`を追加した。

## 完了条件

- WebとMCPの本番用依存関係に、sqlx、ファイルシステム、AdocWeave、openidconnect、
  reqwestが含まれない。
- 保守操作はOIDCのissuerとクライアントシークレットがなくてもストレージを検証・操作できる。
- 実行環境を組み立てるクレート以外は、環境変数を直接読まない。
- 結合試験で使う具体的なアダプターは、`[dev-dependencies]`または専用クレートに隔離する。

## 実施結果

- `marginalis-web`の本番用依存関係を、アプリケーション層、ドメイン層、MCP契約、
  HTTP通信に限定した。SQLite、ファイル、AsciiDoc、OIDC、サーバーの各アダプターは
  テスト専用の依存関係へ移した。
- OIDCの具体型は`marginalis-service`だけが組み立てる。Web側は
  `OidcAuthenticationUseCases`、`WebSessionUseCases`、`UserAdministrationUseCases`だけを受け取る。
- `StorageConfig`、`HttpConfig`、`OidcConfig`と`SecretConfig`を分離し、backup/rebuild/audit-pruneはOIDC issuer・
  client secret・HTTP設定を読まない。
- `cargo make dependency-boundaries`を必須検証へ加えた。WebとMCPの通常の依存グラフに
  禁止したアダプターが混入すると失敗する。
