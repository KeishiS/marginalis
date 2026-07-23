# ロードマップ

## 現在地

`v0.1.0-rc.2`で、OIDC認証を伴うREST API、OAuth保護MCP、NixOS module、手動受入手順および
release gateを成立させた。今後は機能を広げる前に、実運用の経路を継続的に検証できる基盤を整える。

各作業の詳細な受入条件と設計判断は[Issue一覧](../issues/README.md)を正とする。この文書は着手順と
依存関係を示す。

## 優先順

| 段階 | 主Issue | 目的 | 次段階へ進む条件 |
| --- | --- | --- | --- |
| 1 | [030](../issues/030-end-to-end-test-automation-readiness.md) | browser、test IdP、reverse proxy、MCP clientを通すE2E基盤をCIへ導入する。 | OIDC、REST CRUD、MCP OAuth、proxy境界の主要経路を非対話で再現できる。 |
| 2 | [029](../issues/029-adocweave-v0.4.0-adoption.md) | AdocWeave v0.4.0へ移行し、正本解釈・投影・HTML・WASMの契約を再固定する。 | data format、再構築、backup/restoreを含む互換性方針が確定する。 |
| 3 | [033](../issues/033-repository-documentation-asciidoc-migration.md) | repository文書をAsciiDocへ移行し、文書検証をCIへ組み込む。 | README、仕様、運用手順、Issueの形式・参照・閲覧方針が固定される。 |
| 4 | [032](../issues/032-mcp-authoring-profile-and-diagnostics.md) | MCP client向けprofile公開と位置付き検証診断を追加する。 | MCP/RESTが後方互換な診断を返し、clientが推測なしで入力を修正できる。 |
| 5 | [027](../issues/027-search-reference-and-rendering-projections.md) | RenderPolicy、xref表示、公開filterを完成させる。 | 閲覧用HTMLと参照表示の可視性・安全性契約がfixtureで固定される。 |
| 6 | [013](../issues/013-root-administration-and-approval.md) | 再有効化、招待、専用管理origin・mTLSなどの管理運用を必要に応じて追加する。 | 管理経路の脅威モデルと運用手順が実配備に適合する。 |
| 7 | [006](../issues/006-browser-preview.md) | 通常利用者向けのブラウザー編集・previewを提供する。 | profile、診断、RenderPolicyを再利用し、保存時検証と編集時表示が乖離しない。 |
| 8 | [031](../issues/031-postgresql-storage-backend-feasibility.md) | PostgreSQLを任意backendとして採用する価値と移行可能性を判断する。 | SQLite継続・PostgreSQL実装・採用見送りのいずれかを根拠とともに決定する。 |

## 継続的な改善

- [021](../issues/021-test-architecture-and-release-gates.md): E2E suiteの定着後にtest moduleを分離し、
  release gateの保守性を改善する。
- [012](../issues/012-mcp-fuzzy-search-index.md): E2Eで検索の可視性・性能を測定した後、曖昧検索または
  中間表現indexの必要性を再評価する。
- [026](../issues/026-oidc-login-binding-and-runtime-limits.md): 現在のapplication側上限とproxy側rate limitを
  E2Eで検証し、trusted proxyまたは専用管理originの導入要否を判断する。

## 判断の節目

1. **段階1の開始前**: Issue 030の実行基盤、test IdP、proxy再現範囲、MCP自動client、artifact方針の
   五項目を決める。
2. **段階2の完了時**: AdocWeave v0.4.0の互換性差分がdata format v2または明示的な移行を要するか決める。
3. **段階3の完了時**: GitHub表示と外部リンクの互換性を踏まえ、AsciiDocをrepository文書の標準形式として
   固定する。
4. **段階5の完了時**: 通常利用者向けWeb UIを公開するか、API/MCP中心の運用を継続するか決める。
5. **段階8の調査後**: 利用者数、ノート数、同時書込み、backup要件を根拠にSQLiteを継続するかPostgreSQLを
   任意backendとして実装するか決める。

各段階で`cargo make release-gate`と、該当する実環境受入を実施する。公開APIまたはdata formatを変更する
段階では、次のrelease candidateを作る前にOpenAPI、MCP契約、NixOS運用手順および受入確認を更新する。
