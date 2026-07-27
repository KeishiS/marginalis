# ロードマップ

## 現在地

`v0.4.0`はAdocWeave 0.10.1、SQLite schema 3、archive v2、共通入力診断、
`get_note_profile`を導入し、2026-07-27に公開しました。実装済みの変更は
[変更履歴](../CHANGELOG.md)を正とします。

現行のノート単位ACLはdatabaseとarchiveに存在しますが、RESTとMCPから管理できません。
共有要件が確認されていないため、`v0.5.0`ではこの中間状態を維持せず、所有者と
`server-admins`だけの認可モデルへ破壊的に単純化します。

着手順と公開単位はこの文書を正とし、個別の作業と完了条件はGitHub Issuesで管理します。
既存の`issues/`は判断履歴として参照し、移行完了後に削除します。

## 設計方針

- 移行コストと後方互換性を制約にせず、複雑さ、安全性、変更容易性を改善する破壊的変更を選択
- 所有者identityとしての`(issuer, subject)`完全一致
- `server-users`による利用開始と`server-admins`による全ノート管理
- REST、Web UI、MCPで共有する認可・revisionユースケース
- 旧schema、旧archive、旧`dataDir`を読み込まない空database初期化
- 小規模運用におけるSQLite、systemd、journald、既存release gateの継続利用

## 優先順

| 段階 | 対象 | 目的 | 次段階へ進む条件 |
| --- | --- | --- | --- |
| 0（完了） | `v0.4.0` | AsciiDoc契約と入力診断の再基準化 | 完了（2026-07-27、`v0.4.0`タグ） |
| 1 | 文書とIssueの管理境界 | 現行仕様、履歴、作業項目の正本を整理 | GitHub Issue #9、#10の完了 |
| 2 | `v0.5.0`所有者モデル | 利用経路のない直接ACLを削除 | schema 4、archive v3、transport横断認可試験の成功 |
| 3 | OAuth境界の評価 | 自前Authorization Serverを維持する必要性の判断 | 実client接続matrixとADRの承認 |
| 4 | 検索評価 | 再発見できない固定例から最小方式を選択 | 評価入力、期待順位、現行不足の再現 |
| 5 | 条件付き機能 | 参照、Resource、Web編集、グラフ | 独立した需要と安全な公開契約の確定 |

段階1は公開を伴わないリポジトリ保守です。段階2を`v0.5.0`とし、段階3以降の版は
評価結果から決めます。

## v0.5.0の認可・保存契約

### 所有者モデル

ノート作成時の`creator_issuer`と`creator_subject`を変更不能な所有者identityとします。
所有者と`server-admins`は一覧、取得、更新、ソフトデリート、復元を実行できます。
その他の利用者には一覧でも個別操作でもノートの存在を開示しません。

MCP scopeは操作種別を制限しますが、所有権を拡張しません。scopeと所有者認可の両方を
満たした場合だけ操作できます。

`NotePermission`、`NoteAclEntry`、`note_acl`、archiveのACL項目を削除します。個人ACLの
互換層や無効な管理APIは残しません。将来共有を導入する場合は、Kanidm group単位の権限を
全transport、archive、受入試験へ一度に導入します。

### 保存形式

SQLite schemaを4、archiveを`marginalis-archive-3`へ更新します。archiveはACL bundleを廃止し、
所有者を含むノートの配列を直接保持します。schema 3以前とarchive v2以前は拒否し、
`dataDir`を完全に削除した空databaseから初期化します。

## OAuth境界の後続評価

ChatGPT、Claude Code、Codex CLIを使った接続試験で、外部Authorization Serverが必要なsubject、
group、audience、短命tokenおよび対象clientの登録方式を提供できるか確認します。

すべての対象clientが接続できる場合は、自前の登録、同意、authorization code、access token、
refresh tokenと関連tableを別のminor releaseで削除します。接続できないclientがある場合は
自前実装を維持し、登録上限枯渇、権限snapshot、token失効を優先して強化します。

ACL削除とOAuth外部化は、異なる根拠を持つため同じ設計判断へ束ねません。

## AdocWeave更新

現行版は0.10.1へ完全一致で固定します。AdocWeaveの新しいminor releaseは、公開後に固定入力で
互換性とMarginalisの入力profileへの効果を評価します。依存更新だけで動作が変わらない場合は
保守release候補とし、新しい診断や設定を有効化する場合は認可変更と混在させません。

## 検索と条件付き候補

- **検索**: 日本語・英語で再発見できない固定例を用意し、FTS5、正規化、trigram、意味検索の順に比較
- **ノート間参照**: ノートID、anchor、所有者認可、URL解決を一つの契約として設計
- **添付Resource**: 保存先、MIME type、容量、認可、backupを定義できるまで無効
- **Web編集とグラフ**: MCPと閲覧UIでは満たせない需要を確認してから独立した公開単位で検討
- **PostgreSQL**: 複数process、高可用性、規模超過の要件が生じるまで保留

## 継続監視

- ChatGPT、Claude Code、Codex CLIのMCP接続とtool resultの解釈
- 所有者と`server-admins`のtransport横断認可
- AdocWeave package版、note profile、OpenAPI、MCP tool schemaの一致
- backup最終成功時刻、保存世代数、四半期復元試験
- database容量、ノート数、revision conflict、検索失敗例

各公開では`cargo make release-gate`と実環境受入を実施します。公開API、MCP tool、NixOS option、
archive、schemaまたはAdocWeave契約を変更する場合は、実装と同じPull Requestで仕様文書、
動作例、受入手順を更新します。
