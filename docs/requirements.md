# 現行要件

## 仮定

- 単一NixOS host、SQLite、同時利用者10名、約1,000ノート
- Kanidm 1.10によるOIDC本人確認とgroup管理
- 旧`dataDir`を完全に削除した空databaseからの初期化

## 確定要件

- SQLiteは本文、メタデータ、所有者、削除状態の単一正本です。AsciiDocはノート単位のexport、
  JSON archiveは全体のimport/export形式であり、稼働時の正本ではありません。
- ノート所有者は作成時の`(issuer, subject)`であり変更できません。所有者と`server-admins`だけが
  ノートを閲覧・管理できます。その他の利用者には存在を開示しません。
- `server-users`に属する主体だけが利用できます。署名検証済みOIDC ID tokenの`groups` claimを
  login時の権限snapshotとし、group変更は次回loginから反映します。
- Web sessionは最終利用から24時間、loginから7日で失効します。期限切れまたは失効済みの認証状態は
  日次保守で削除します。
- REST、Web UI、MCPは同じ所有者認可とrevision規則を使います。REST APIは`/api/v2`、MCPは
  OAuth 2.1 Authorization Code + PKCE S256とDynamic Client Registrationを提供します。
- MCP scopeは操作種別を制限しますが、ノート所有権を拡張しません。
- 削除は30日のソフトデリートです。本文履歴は保存せず、期限後に日次timerで物理削除します。
- 本文はUTF-8で512 KiB以下とし、上限超過時はAsciiDoc解析を開始せず拒否します。
- NixOS moduleはSQLite、OIDC client、MCP、backup destinationを設定でき、client secretを
  systemd credentialで渡します。
- SQLite schema 4と`marginalis-archive-3`だけを受理します。旧schemaと旧archiveの移行は
  提供しません。

実装上の不変条件は[アーキテクチャ](architecture.md)、HTTP仕様は
[OpenAPI](openapi.json)を参照してください。v0.3.0時点の要件と設計判断は
[再設計判断記録](v0.3.0-design.md)を履歴として参照します。
