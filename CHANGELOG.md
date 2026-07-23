# Changelog

この文書には利用者に影響する変更だけを記録する。公開 API、データフォーマット、NixOS
モジュールの動作を変えない内部的な再構成は掲載しない。

## 0.1.0-rc.2 — Unreleased

RC.1 で見つかったリリースゲートの不備を修正した候補版である。研究室内で REST API と MCP を
実運用するための基準点を提供する。

### Added

- OIDC ログイン、root 管理、ユーザーへの直接 ACL、監査ログ。
- AsciiDoc 正本を用いる REST ノート CRUD、FTS5 検索、`ETag` による条件付き更新、物理削除。
- OAuth Authorization Code + PKCE で保護された MCP ツール（検索・取得・参照一覧・作成・
  更新・削除）。
- OpenAPI 3.1 契約、NixOS モジュール、バックアップ・復元、投影再構築、root 監査の
  365 日保持。

### Security

- Cookie を伴う変更操作で、CSRF トークン・公開オリジン・Fetch Metadata を検証する。
- root 管理ルーターを通常 API から分離し、プロキシの forwarded クライアント IP ヘッダーを
  信頼しない。

### Fixed

- NixOS のランタイム VM リリーステストへ `sqlite3` CLI を含め、root 資格情報の検証を実行
  できるようにした。

### Known limitations

- Web UI、SMTP、招待、ユーザー再有効化、グループ ACL、専用管理オリジン・mTLS は含まれない。
- 実際の Kanidm と MCP クライアントを使う受入確認は、秘密情報を CI へ置かずに手動で行う。
- RC.2 の `/api/v1` 契約は v0.1.0 の凍結候補であり、リリースブロッカーの修正だけを受け入れる。
