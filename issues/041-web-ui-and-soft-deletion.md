# 041: 閲覧用Web UIとソフトデリート

## 状態

実装完了。[038](038-sqlite-canonical-notes-and-asciidoc-bundles.md)と
[039](039-kanidm-group-authorization-and-mcp-oauth.md)の上に、独立 HTTP router として
認証済みの一覧閲覧、ACLを通すノート取得、CSRF保護した作成・更新・削除、および単体 AsciiDoc
export を追加した。SQLiteの`deleted_at`と日次`purge-deleted` timerも追加済みである。安全なHTML
renderingも追加した。SQLite正本をcontent profileで再検証して固定RenderPolicyでHTML化する。
検索・グラフは将来拡張とし、実環境 E2E は [042](042-v0.3.0-release-acceptance.md)で扱う。
削除前の直接Adminと`server-admins`には、revision
一致を条件とする30日以内の復元 API を提供する。

## 目的

新しい API と同じ認可規則を再利用する閲覧用 Web UI を提供し、誤削除から回復できる 30 日間の
ソフトデリートを導入する。

## 作業内容

1. ログイン利用者が閲覧できるノートだけを一覧、検索、HTML 表示、リンク、グラフ候補へ出す
   閲覧用 Web UI を実装する。HTML は AsciiDoc の安全な RenderPolicy を必ず利用する。
2. `server-admins` には全ノートの閲覧・管理を提供する。通常利用者の ACL 非公開性は API と UI の
   両方で維持する。
3. 削除を `deleted_at` を持つソフトデリートへ変更する。削除済みノートは通常の API、MCP、Web UI、
   検索、参照・グラフ投影から即時に除外する。
4. 元のノート管理者と `server-admins` に 30 日間の復元を提供する。毎日実行する保守処理で期限切れ
   ノート、ACL、投影を物理削除する。

## 完了条件

- Web UI と REST/MCP が同じ可視性と RenderPolicy を用いる。
- 削除済みノートは通常経路から漏洩せず、権限を持つ利用者だけが 30 日間復元できる。
- 日次 purge が期限切れノートを完全に削除し、E2E で確認できる。
