# Changelog

この文書は利用者に影響する変更だけを記録する。開発中の内部再構成は、公開 API、data format、NixOS moduleの
動作を変えない限り掲載しない。

## 0.1.0-rc.1 — Unreleased

最初の候補版である。研究室内で REST API と MCP を実運用するための基準点を提供する。

### Added

- OIDC login、root 管理、直接ユーザー ACL、監査ログ。
- AsciiDoc 正本を用いる REST ノート CRUD、FTS5 検索、ETag による条件付き更新と物理削除。
- OAuth Authorization Code + PKCE による MCP の read/search/link/write/delete tool。
- OpenAPI 3.1 contract、NixOS module、backup/restore、projection rebuild、root監査の365日保持。

### Security

- Cookie を伴う変更操作で CSRF、公開 origin、Fetch Metadata を検証する。
- root 管理 router を通常 API から分離し、proxy の forwarded client-IP header を信頼しない。

### Known limitations

- Web UI、SMTP、招待、ユーザー再有効化、グループ ACL、専用管理 origin/mTLS は含まれない。
- 実 Kanidm と実 MCP client を用いる受入確認は、秘密情報を CI に置かず手動で行う。
- RC.1 の `/api/v1` contract は v0.1.0 の freeze 候補であり、release blocker の修正だけを受け入れる。
