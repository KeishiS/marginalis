# ロードマップ

## 目的と対象

この文書は、v0.7.0公開後の方向、着手順、公開単位を示します。個別作業と完了条件は
[GitHub Issues](https://github.com/KeishiS/marginalis/issues)で管理し、公開済みの利用者影響は
[変更履歴](../CHANGELOG.md)を参照します。

## 現在地

`v0.7.0`は2026-07-28に公開しました。React・TypeScriptフロントエンド、identity単位ACL、
ノート参照を追加し、SQLite schema 8、note profile 2、`marginalis-archive-6`へ更新しました。

公開後の再設計では、domainとapplicationの境界、ACLとSQLite transaction、公開API契約、
Web UIをこの順に整理しています。REST APIは`/api/v3`へ更新し、OpenAPI、TypeScriptクライアント、
MCPツール定義を`marginalis-contract`から生成します。一覧、閲覧、編集、共有設定は一つの
Reactアプリケーションが担当します。

リポジトリ内のIssue管理はGitHub Issuesへ移行しました。v0.5.0以前のローカルIssueは
[移行対応表](issue-migration.md)から参照できます。

## 設計方針

- 移行コストと後方互換性を制約にせず、複雑さ、安全性、変更容易性を改善する変更の選択
- 一つの公開単位につき一つの設計根拠
- 未使用の互換層と中間モデルの削除
- schemaとarchiveの更新を、実際の保存契約変更時だけ実施
- OAuth、検索、参照、Resource、Web編集の独立した評価
- 小規模運用におけるSQLite、systemd、journald、既存release gateの継続利用

## 優先順

現在の再設計は次の順序で進めます。

| 順序 | Issue | 対象 | 状態 |
| --- | --- | --- | --- |
| 1 | [#54](https://github.com/KeishiS/marginalis/issues/54) | domain型とapplication境界 | 実装・検証済み |
| 2 | [#55](https://github.com/KeishiS/marginalis/issues/55) | ACLとSQLite transaction境界 | 実装・検証済み |
| 3 | [#52](https://github.com/KeishiS/marginalis/issues/52) | 公開契約とTypeScript Web UI | 実装・検証済み |
| 4 | [#53](https://github.com/KeishiS/marginalis/issues/53) | 要件、試験、受入、リリースゲート | 作業中 |

#19と#22は今回の作業対象から除外します。#24は別の作業セッションで扱い、この順序には
含めません。

## 公開済みの順序

| 段階 | 公開単位 | 目的 | 次段階へ進む条件 |
| --- | --- | --- | --- |
| 0（完了） | `v0.5.0` | 所有者モデルへの認可単純化 | 完了（2026-07-27、`v0.5.0`タグ） |
| 1 | リポジトリ保守 | 文書とIssueの管理先整理 | GitHub Issue #9、#10の完了 |
| 2（完了） | `v0.6.0` | AdocWeave 0.11の処理別設定へ移行 | 完了（2026-07-27、`v0.6.0`タグ） |
| 3（完了） | `v0.7.0` | Web編集、identity単位ACL、ノート参照 | 完了（2026-07-28、`v0.7.0`タグ） |
| 4 | 評価・ADR | 外部Authorization Serverへの移行可否 | [GitHub Issue #24](https://github.com/KeishiS/marginalis/issues/24)の接続matrixとADR |
| 5 | 検索評価 | 再発見できない固定例から最小方式を選択 | [GitHub Issue #19](https://github.com/KeishiS/marginalis/issues/19)の評価結果 |
| 6 | `v0.8.0`候補 | 評価結果に基づく検索改善 | 固定評価集合、所有者認可、運用試験の成功 |
| 7 | 条件付き機能 | 参照、Resource、Web編集、グラフ | 独立した需要と安全な公開仕様の確定 |

段階1は公開を伴わないリポジトリ保守です。今後のバージョンは評価結果によって変更できます。

## v0.6.0で完了した移行

AdocWeave 0.11では、解析、診断、執筆時URL、描画時URL、出力上限の設定型が責務別に
再設計されました。Marginalisは削除された0.10.1の公開APIを使用しているため、単純な依存更新では
なく破壊的な契約更新として扱います。

固定入力で0.10.1と比較し、新しい既定lintが現行note profileへ与える影響を確認します。
REST、MCP、HTMLで診断とURL安全性を一致させ、必要な場合だけnote profile版を更新します。
SQLiteに解析cacheを保存していないため、保存構造が変わらない限りschemaとarchiveの番号は
更新しません。認可とOAuthの変更は混在させません。

移行では保存時の`AnalysisOptions`と診断profile、執筆時URLの`AuthoredUrlPolicy`、
HTML出力時の`RenderPolicy`と`ActiveUrlPolicy`、出力上限を別々に構成します。0.11.0で追加された
既定lintは警告に留まり、既存の保存可否とMarginalisが公開する問題識別用の`code`を変更しないため、
note profile版`1`を維持します。`macro-boundary`は任意規則のため採用しません。schema番号は維持し、
復元互換性を明示するためarchive identityをv4、AdocWeave package版を`0.11.0`へ更新します。
固定入力の比較結果と採否理由は[AdocWeave 0.11移行判断](adocweave-v0.11-migration.md)に記録します。

## OAuth境界

ChatGPT、Claude Code、Codex CLIを外部Authorization Serverへ接続し、subject、group、
resource、audience、client登録、失効を評価します。

すべての対象clientが成立する場合は、自前の登録、同意、authorization code、access token、
refresh tokenと関連tableを一つの破壊的リリースで削除します。成立しない場合は自前実装を維持し、
登録上限、権限snapshot、失効、診断を強化します。評価前にどちらかの実装へ着手しません。

## 検索と条件付き候補

- **検索**: [GitHub Issue #19](https://github.com/KeishiS/marginalis/issues/19)で日本語・英語の
  固定例を作り、FTS5、正規化、trigram、意味検索の順に比較
- **ノート参照**: [GitHub Issue #17](https://github.com/KeishiS/marginalis/issues/17)で
  ノートID、anchor、所有者認可、URL解決を一つの契約として評価
- **Web編集**: [GitHub Issue #18](https://github.com/KeishiS/marginalis/issues/18)で、作成、編集、
  プレビュー、競合時の差分確認を実装
- **グラフ**: [GitHub Issue #22](https://github.com/KeishiS/marginalis/issues/22)で
  参照契約と検索だけでは辿れない固定例を確認した場合だけ着手
- **PostgreSQL**: [GitHub Issue #20](https://github.com/KeishiS/marginalis/issues/20)で定める
  複数process、高可用性、規模超過の条件まで保留
- **文書のAsciiDoc化**: [GitHub Issue #21](https://github.com/KeishiS/marginalis/issues/21)で
  形式差分に見合う保守上の便益を確認できるまで保留
- **添付Resource**: 保存先、MIME type、容量、認可、backupを定義できるまで非対応

## 継続監視

- ChatGPT、Claude Code、Codex CLIのMCP接続とtool resultの解釈
- 所有者と直接ACL共有先のtransport横断認可
- AdocWeave package版、note profile、OpenAPI、MCP tool schemaの一致
- backup最終成功時刻、保存世代数、四半期復元試験
- database容量、ノート数、revision conflict、検索失敗例

各公開では`cargo make release-gate`と実環境受入を実施します。公開API、MCP tool、NixOS option、
archive、schemaまたはAdocWeave契約を変更する場合は、実装と同じPull Requestで仕様文書、
動作例、受入手順を更新します。
