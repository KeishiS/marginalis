# ロードマップ

## 現在地

`v0.3.0`は、SQLite正本、Kanidm 1.10の署名済みgroup claim、REST API v2、OAuthで保護した
MCP、閲覧用Web UI、NixOS moduleを一つの認可モデルへ再設計し、2026-07-25に公開した。
旧API、旧保存形式、ローカル`root`、所属定期監視との互換性は提供しない。

今後は機能数を増やす前に、公開済みの構成を少人数で低コストに運用できることを優先する。
詳細な受入条件と設計判断は[Issue一覧](../issues/README.md)を正とし、この文書は着手順と
判断時期を示す。

## 方針

- `v0.3.1`では公開APIと保存形式を変えず、復元可能性、運用診断、OAuth・MCPの回帰検証を強化する。
- `v0.4.0`ではMCPを主要な執筆経路として改善し、AI clientの無駄な再試行を減らす。
- 検索、グラフ、Web編集、別database backendは、実利用から必要性を確認してから追加する。
- 小規模運用では新しい常駐基盤を安易に増やさず、systemd、journald、SQLite、既存のrelease gateを
  再利用する。
- 破壊的変更は必要な場合に認めるが、公開済みの境界を壊すだけの明確な便益がない変更は行わない。

## 優先順

| 段階 | 対象 | 目的 | 次段階へ進む条件 |
| --- | --- | --- | --- |
| 0（完了） | [037](../issues/037-v0.3.0-architecture-rebaseline.md)〜[044](../issues/044-source-layout-and-documentation-boundaries.md) | v0.3.0の破壊的再設計と公開 | 完了（2026-07-25、`v0.3.0`タグ） |
| 1 | [045](../issues/045-backup-restore-lifecycle.md) | backupを空databaseへ復元できることを継続的に検証する | archive検証、NixOS VM復元試験、運用手順、保存世代管理が同じ仕様で動く |
| 2 | [046](../issues/046-browser-mcp-protocol-regression.md) | browser、subpath、OAuth、MCP clientの回帰を自動検出する | 標準protocol flowを自動化し、実clientだけを手動受入に限定する |
| 3 | [047](../issues/047-runtime-operability-diagnostics.md) | 小規模運用に必要な診断と失敗通知を整える | 秘密を出さずにDB、OIDC、保守jobの状態と失敗原因を特定できる |
| 4 | [048](../issues/048-v0.3.1-release-acceptance.md) | v0.3.1を受入・公開する | 自動gate、復元試験、実環境のOIDC・MCP・backup確認が成功する |
| 5 | [032](../issues/032-mcp-authoring-profile-and-diagnostics.md) | MCP入力規則と位置付き診断を提供する | RESTとMCPが同じ診断型を返し、clientが失敗理由を機械判定できる |
| 6 | [012](../issues/012-mcp-fuzzy-search-index.md) | 実利用に基づいて検索品質を評価する | 固定した評価例で現行FTS5の不足を確認した場合だけ検索方式を拡張する |

段階1から4を`v0.3.1`、段階5を`v0.4.0`の主な公開範囲とする。段階6の公開版は、評価結果から
変更範囲を確定した後に決める。

## v0.3.1の運用目標

- `backupDirectory`を設定した環境では日次backupを既定とし、30世代を保持する設計を基準にする。
- 四半期ごとの週末に、本番を停止せず最新archiveを一時的な空databaseへ復元して検証できるようにする。
- 復元試験は本番databaseと本番OIDC・MCP認可状態を変更しない。
- 失敗時はsystemd unit、終了status、構造化log、request IDから原因を追跡できるようにする。
- Prometheus等の監視基盤やraw検索語の収集は導入しない。必要性が確認された場合は別Issueで扱う。

保存先そのものの冗長化、off-site複製、保存媒体のsnapshotはNixOS host側の運用責務とする。
Marginalisは指定先に整合したarchiveを作り、世代管理と復元可能性を検証する。

## v0.4.0の執筆目標

ChatGPT、Claude Code、Codex等のMCP clientを主な執筆経路とする。`create_note`と`update_note`の
入力規則を機械可読に公開し、検証失敗時は規則の識別子と可能な場合は本文中の位置を返す。
同じ規則を文書、tool schema、REST、MCPへ重複して手書きしない。

閲覧用Web UIを編集アプリケーションへ拡張する作業は、この段階に含めない。MCPでは解決できない
具体的な編集需要が得られた場合に[006](../issues/006-browser-preview.md)を再設計する。

## 条件付きの候補

- **検索拡張**: 日本語・英語の再発見に失敗する固定例を集め、FTS5の語彙検索、表記揺れ対応、
  trigram、意味検索の順に小さく比較する。検索語を無断でlogへ保存しない。
- **グラフUI**: [034](../issues/034-graph-visualization-web-ui.md)は、一覧・検索だけでは参照関係を
  辿れない実例が蓄積した場合に再設計する。
- **連環（Renkan）**: 複数のデータソースを横断する段階ではMarginalisへ検索基盤を内包せず、
  独立serviceとconnector境界を検討する。
- **PostgreSQL**: 複数process、高可用性、または現在の規模を超える運用要件が発生した場合だけ
  [031](../issues/031-postgresql-storage-backend-feasibility.md)を再開する。
- **リポジトリ文書のAsciiDoc化**: [033](../issues/033-repository-documentation-asciidoc-migration.md)は
  大量の形式差分に見合う保守上の便益が確認されるまで着手しない。

## 継続監視

- ChatGPT、Claude Code、Codex CLIの版と、MCP接続の手動受入結果
- backupの最終成功時刻、保存世代数、四半期復元試験の結果
- database容量、ノート数、主要操作の失敗、revision conflict
- 検索で見つからなかった具体例と、MCP入力検証による再試行

各公開では`cargo make release-gate`と変更範囲に応じた実環境受入を実施する。公開API、
MCP tool、NixOS option、archive形式を変更する場合は、実装と同じPull Requestで仕様文書と
受入手順を更新する。
