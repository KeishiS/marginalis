# v0.3.0 要件

## 仮定

- 単一 NixOS host、SQLite、同時利用者 10 名、約 1,000 ノートを対象とする。
- Kanidm 1.10 が OIDC provider と group 管理を担う。
- v0.2 のデータと公開 API は互換対象ではなく、空 database から初期化する。

## 確定要件

- SQLite は本文、メタデータ、ACL、削除状態の単一正本である。AsciiDoc はノート単位と archive の
  import/export 形式であり、稼働時の正本ではない。
- `server-users` に属する主体だけが利用できる。`server-admins` は全ノートを読め、管理できる。
  署名検証済み OIDC ID token の `groups` claim をログイン時の権限スナップショットとする。
  group 変更は次回ログインから反映する。
- REST、Web UI、MCP は同じ ACL と revision 規則を使う。REST API は `/api/v2`、MCP は OAuth 2.1
  Authorization Code + PKCE S256 と Dynamic Client Registration を提供する。
- 削除は 30 日のソフトデリートである。本文履歴は保存しない。期限後の物理削除は日次 timer が行う。
- NixOS module は SQLite、OIDC client、MCP、backup destination を設定できる。client secret は systemd
  credential で渡す。
- v3 の公開条件は Kanidm 1.10 E2E、MCP 認可、NixOS 配備の成功である。archive restore の定期試験は
  公開後に追加する。

実装上の不変条件は[アーキテクチャ](architecture.md)、HTTP契約は
[OpenAPI](openapi.json)を参照してください。過去のv0.2要件とv0.3の設計確定過程は、
Git履歴と[再設計判断記録](v0.3.0-design.md)で参照します。
