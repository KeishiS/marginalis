# v0.3.0 受入確認

この受入は空の SQLite database から行います。v0.2 の DB、`dataDir`、ファイル正本、root credential は
入力にしません。archive import と定期復元試験は公開後の運用改善であり、本 release gate には含めません。

## 必須確認

1. NixOS module で service を配備し、`GET /api/v2/health` が `200` を返す。
2. TLS とサブパスで OIDC login を行い、`server-users` 所属者だけが session を取得できる。
3. `server-admins` の利用者が他人のノートを閲覧・管理でき、通常利用者には ACL が適用される。
4. Kanidm で group を変更し、最大 5 分後の要求で失効または管理権限の変更が反映される。Kanidm を
   確認できない状態では、freshness が期限切れた要求が拒否される。
5. Web UI と `/api/v2` からノートを作成、更新、削除、復元し、revision conflict と CSRF 拒否を確認する。
6. ChatGPT、Claude Code、Codex CLI の各 MCP client で Dynamic Client Registration、OIDC login、
   Authorization Code + PKCE、read/write、認可取消を確認する。
7. `marginalis-backup.service` が指定先に archive を作ること、日次 purge timer が有効なことを確認する。

実施結果は release issue に、環境、client の版、Kanidm 1.10 の版、base URL（機密情報を除く）、
各項目の結果として記録します。
