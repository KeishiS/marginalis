# ロードマップ

## 現在地

`v0.1.0` をリリースし、OpenAPI 仕様の互換性保証を開始しました
（2026-07-23、段階 0 完了）。OIDC 認証付きの REST API、OAuth 保護 MCP、NixOS モジュール、
手動受入手順、リリース検証が揃っています。今後は機能を広げる前に、実運用の主要経路を
継続的に検証できる基盤を整えます。

データフォーマットの識別子は v1 を維持していますが、AdocWeave v0.6.1 前提の内容へ
破壊的に再定義しました。以前の v1 は互換対象ではありません。
`v0.2.0`は、実環境受入、Pull Request、`main`上の最終検証、タグ公開、タグで起動した
リリースゲートを完了しました（2026-07-24）。その後の検討により、機能拡充の前に
`v0.3.0`で破壊的なアーキテクチャ再設計を行うことを決定しました。旧 API、旧保存形式、
ローカル `root` 認証との互換性は維持しません。

`v0.2.0`の正式リリース後は`-rc.N`付きの版を公開せず、ロードマップ上の成果を
`v0.x.y`形式の通常版として順次公開します。各公開前には、リリースゲートと変更範囲に
応じた実環境受入を実施します。

各作業の詳細な受入条件と設計判断は [Issue 一覧](../issues/README.md)を正とします。この文書は
着手順と依存関係だけを示します。

## 優先順

| 段階 | 主 Issue | 目的 | 次段階へ進む条件 |
| --- | --- | --- | --- |
| 0（完了） | [009](../issues/009-oidc-provider-registration.md)、[022](../issues/022-v0.1.0-rc.1-release-acceptance.md) | RC.2 の実環境受入を完了し、v0.1.0 をタグ付けして OpenAPI の互換性保証を始める | 完了（2026-07-23、`v0.1.0` タグ） |
| 2（完了） | [029](../issues/029-adocweave-v0.6.1-migration.md) | AdocWeave v0.6.1 へ移行し、正本の解釈・投影・HTML・WASM の互換性基準を更新する | 完了（2026-07-24。旧 v1 は移行せず、`dataDir` を削除して初期化する） |
| 公開準備（完了） | [035](../issues/035-v0.2.0-rc.1-release-acceptance.md) | 空の新 v1 環境で実環境受入を行い、`v0.2.0-rc.1` の公開可否を判断する | 完了（2026-07-24、`v0.2.0-rc.1` タグ） |
| 正式公開（完了） | [036](../issues/036-v0.2.0-release-acceptance.md) | RCの受入結果と正式版の差分を検証し、`v0.2.0`を公開する | 完了（2026-07-24、`v0.2.0`タグ） |
| 1（完了） | [037](../issues/037-v0.3.0-architecture-rebaseline.md) | SQLite 正本、Kanidm group 認可、新 API に再設計する | データ正本、認可、MCP、削除、公開 API の契約を固定した |
| 2（完了） | [038](../issues/038-sqlite-canonical-notes-and-asciidoc-bundles.md) | SQLite の単一正本と AsciiDoc import/export を実装する | ファイル正本・操作ジャーナルなしにノートと ACL を一 transaction で更新する |
| 3（実装完了） | [039](../issues/039-kanidm-group-authorization-and-mcp-oauth.md) | Kanidm 1.10 group claim 認可と MCP OAuth を実装する | 自動試験済み。対象 MCP client の実環境受入は段階 6 で行う |
| 4（実装完了） | [040](../issues/040-v0.3.0-nixos-and-e2e-foundation.md) | NixOS 配備と Kanidm 1.10 E2E を release gate に組み込む | TLS、subpath、OIDC、NixOS module を CI で再現した |
| 5（完了） | [041](../issues/041-web-ui-and-soft-deletion.md) | 閲覧用 Web UI と 30 日間のソフトデリートを提供する | API、MCP、Web UI が同一の可視性を守り、期限後の物理削除を自動化する |
| 横断（現在） | [043](../issues/043-production-reachability-and-test-coverage.md) | 本番到達性とv0.3 test coverageを分離して可視化する | 旧実装のproduction graph復帰を拒否し、未実行箇所を試験不足と不要コードに分類できる |
| 6（現在） | [042](../issues/042-v0.3.0-release-acceptance.md) | 空の新環境で v0.3.0 を受入・公開する | Kanidm 1.10 E2E、MCP 認可、NixOS 配備が成功し、破壊的初期化手順が確定する |

## 継続的な改善

- [012](../issues/012-mcp-fuzzy-search-index.md): 新しい SQLite 正本の検索品質を測定した後、
  曖昧検索や中間表現インデックスの必要性を再評価する。
- archive import と復元の定期受入は、v0.3.0 公開後の運用改善として追加する。
- グラフ可視化、編集プレビュー、PostgreSQL は、v0.3.0 の利用実績を得てから再評価する。

## 判断の節目

1. **段階 1**: SQLite 正本、Kanidm group、MCP OAuth、ソフトデリート、API v2 を
   破壊的に切り替える。旧環境は新しい空環境へ移行せず、退避物として扱う。
2. **段階 3**: Kanidm の所属は OIDC login 時に署名検証済み `groups` claim から確定する。group 変更は
   次回 login から反映し、既存 session と MCP token は有効期限または認可取消まで発行時の権限を保つ。
3. **段階 4**: E2E の Kanidm は実運用と同じ 1.10 系列を使う。実本番 IdP、proxy、MCP client
   との接続確認は公開前の手動受入に残す。
4. **段階 6**: archive import と復元を公開条件から外す。復元の頻度、保存先、保持世代は
   v0.3.0 公開後の実運用を基に定める。

## 監視項目

段階には置かず、実利用からの信号で再評価します。

- **MCP client 相互運用性**: ChatGPT、Claude Code、Codex CLI の更新に対して、OAuth と
  remote MCP の接続試験を継続する。
- **検索品質**: 段階 4 の E2E と実利用で測定した後、Issue 012 の曖昧検索の要否を判断する。
- **データベース**: 10 人・約 1,000 ノートの想定を超える利用実績が得られた場合にだけ、
  PostgreSQL を再評価する。

各段階で `cargo make release-gate` と、該当する実環境受入を実施します。公開 API または
データフォーマットを変更する段階では、次の通常版を公開する前に OpenAPI、MCP 仕様、NixOS
運用手順、受入確認を更新します。
