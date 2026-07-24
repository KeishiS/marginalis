# 研究データ横断検索ハブの将来構想

## 状態

本書は、Marginalis と文献管理ツール等の複数データソースを横断して検索する、独立した検索ハブの
将来構想を記録する。現行 Marginalis の製品要件、公開 API、リリース計画を変更する規範文書では
ない。実装時期と段階ごとの受入条件は未定であり、着手時に個別の Issue で確定する。

## 目的

研究活動の記録を再発見し、現在の関心に関連するノートや文献を探索できるようにする。検索結果を
AI エージェントと検討し、その成果を AsciiDoc の研究メモとして Marginalis に新規作成または
追記する循環を支援する。

```text
研究データを収集する
        │
        ▼
横断検索・関連探索
        │
        ▼
利用者と AI エージェントによる検討
        │
        ▼
Marginalis の研究メモを作成・更新
        │
        └──────── 次回以降の検索対象へ
```

検索結果の利用と明示的な評価を学習データとして蓄積し、個人の検索意図に合わせて検索順位を
継続的に改善する。将来、次に探求すべき対象を提案するフロンティア探索へ拡張できる境界は残すが、
初期実装の対象にはしない。

## 仮定

- 主な利用者は一人である。将来は最大 10 名程度の研究室単位で利用する可能性がある。
- Marginalis のほか、Paperpile を最初の文献管理データソースとして想定する。特定製品に
  検索ハブ全体を依存させず、複数の文献管理ツールと接続できる構成にする。
- 各データソースには、書誌情報、ノート本文、抄録等のテキストとメタデータが既に存在する。
- PDF 全文や OCR 結果の抽出はコネクターの入力拡張として扱う。検索ハブの中核は、受け取った
  テキストを正規化し、適切な粒度へ分割して検索可能にすることである。
- 将来 Marginalis が SQLite から PostgreSQL へ移行しても、Marginalis の正本と検索ハブの
  カタログは責務、database または schema、DB role を分離する。
- 埋め込み表現を生成するモデルは別途選定する。検索ハブはモデル実装に依存しないポートを持つ。

## 定義

- **データソース**: Marginalis、Paperpile 等、検索対象データの正本を管理するアプリケーション。
- **コネクター**: データソース固有の API、エクスポート形式、変更通知を共通の取込形式へ変換する
  アダプター。
- **Resource**: ノート、論文、書籍等、利用者へ検索結果として提示する文書単位。
- **Chunk**: 見出し、段落、抄録、節等、検索索引と埋め込みを作る単位。検索結果では Resource
  ごとにまとめる。
- **検索カタログ**: PostgreSQL に保存する、Resource、Chunk、出典、識別子、同期状態、権限、
  埋め込み、利用フィードバックの正規化済みデータ。
- **検索投影**: 検索カタログとデータソースから再構築できる Elasticsearch の索引。
- **一級データ**: 独自の識別子、保存形式、ライフサイクル、操作境界を持つデータ。初期構想では
  Source、Resource、Chunk、SearchSession、Feedback、Outcome、Connector を一級データとする。
  Concept、Claim、Question は独立管理せず、まず Marginalis の研究メモの内容として扱う。

## 対象範囲

### 初期対象

- 複数データソースからのメタデータとテキストの増分取込
- 文書種別に応じた Chunk への分割
- Elasticsearch による日本語・英語の全文検索、表記揺れ、部分一致
- PostgreSQL と pgvector による意味検索
- 語彙検索と意味検索の候補統合および再順位付け
- 検索語による再発見と、Resource を起点にした関連探索
- MCP を介した人間向け提示と AI エージェントによる自律的な利用
- 明示的・暗黙的フィードバックと研究メモへの利用結果の記録
- 個人向けの軽量な Learning to Rank

### 初期対象外

- フロンティア探索と研究課題の自動提案
- Concept、Claim、Question の自動抽出と独立管理
- PDF 解析、OCR、数式・図表抽出そのものの開発
- 検索ハブから各データソースへの汎用的な編集
- 大規模な分散検索クラスタ
- 埋め込み基盤モデルの個人利用履歴だけによる新規学習

## 全体構成

```text
Marginalis ───── Connector ─┐
Paperpile ────── Connector ─┤
その他データ源 ─ Connector ─┘
                            │
                  正規化 / 同一実体の照合
                            │
                  PostgreSQL 検索カタログ
          ┌─────────────────┼──────────────────┐
          │                 │                  │
      メタデータ        pgvector           同期・学習
      Resource/Chunk     埋め込み            feedback/outcome
          │                 │                  │
          └─────── Elasticsearch 投影 ────────┘
                            │
                    Retrieval Service
              候補統合 / ACL / 再順位付け
                            │
                   OAuth 保護 MCP
                    ┌───────┴────────┐
                 人間向け UI       AI エージェント
                                      │
                                      └─ Marginalis MCP
                                         create/update note
```

### 責務境界

- 各データソースは本文、メタデータ、編集、削除、アクセス制御の正本を保持する。
- 検索ハブの PostgreSQL は、横断検索に必要な共通メタデータ、Chunk、埋め込み、同期状態、
  フィードバックを保持する。データソース固有の編集正本にはならない。
- Elasticsearch は全文検索用の非正規化された投影であり、失われても PostgreSQL と各
  データソースから再構築できる。
- Retrieval Service だけが Elasticsearch と pgvector の候補を統合し、利用者へ結果を返す。
  MCP クライアントへ Elasticsearch や PostgreSQL を直接公開しない。
- AI エージェントは検索ハブ MCP と Marginalis MCP をそれぞれ利用する。検索ハブが Marginalis
  のノートを代理編集しない。

## 共通取込形式

コネクターは少なくとも次の論理形式を出力する。物理的な JSON schema と版管理規則は実装 Issue
で確定する。

```text
SourceRecord
  source_id
  external_id
  revision
  resource_kind
  title
  creators
  dates
  identifiers       // DOI、ISBN、arXiv ID 等
  tags
  abstract
  text_segments
  canonical_url
  source_relations
  visibility
  provenance
```

`source_id` と `external_id` の組をデータソース内の同一性に使う。DOI 等の外部識別子は、
異なるデータソースにある同じ研究対象を照合するために使うが、元 Resource を無条件に統合しない。
照合結果には根拠と確信度を持たせ、誤った統合を解除できるようにする。

### Paperpile コネクター

初期コネクターは、Paperpile が提供する自動同期 BibTeX を定期取得する方式を候補とする。
Paperpile はライブラリ全体、フォルダー、ラベルを Google Drive、GitHub、またはダウンロード
URL の BibTeX へ同期できる。コネクターは内容ハッシュと取得時刻を記録し、前回との差分を
冪等に反映する。

Paperpile の JSON エクスポートは、ユーザー編集可能なメタデータ、添付、フォルダー、ラベルを
含むため、手動の初回取込や整合性確認の候補とする。PDF は Paperpile の Google Drive 同期から
取得できるが、書誌 Resource との安定した対応付けと本文抽出は独立した後続機能とする。

参考:

- [Automatically sync BibTeX files](https://paperpile.com/h/sync-bibtex-files/)
- [Export your library data](https://paperpile.com/h/export-library-data/)
- [Sync with Google Drive](https://paperpile.com/h/sync-google-drive/)

## PostgreSQL のデータ境界

検索カタログは少なくとも次の論理テーブルを持つ。

| 対象 | 内容 |
| --- | --- |
| `workspaces` | 個人または将来の研究室単位の検索空間 |
| `sources` | コネクター種別、接続設定参照、同期状態 |
| `resources` | 出典、外部 ID、種別、題名、著者、日時、識別子、正規 URL、revision、削除状態 |
| `resource_identifiers` | DOI、ISBN、arXiv ID 等の複数識別子 |
| `resource_relations` | 同一候補、添付、引用、ノート参照等の関係 |
| `chunks` | Resource 内の検索用テキスト、位置、アンカーまたはページ、分割規則版 |
| `embedding_models` | モデル ID、次元、距離尺度、前処理版 |
| `chunk_embeddings` | Chunk、モデル ID、埋め込み、元 revision |
| `resource_permissions` | subject、group、visibility の正規化投影 |
| `sync_cursors` | データソースごとの増分同期位置 |
| `index_jobs` | 取込、分割、埋め込み、Elasticsearch 反映の状態と再試行 |
| `search_sessions` | 問い合わせ、実行者、検索設定版 |
| `search_impressions` | 提示候補、順位、各検索器のスコア、モデル版 |
| `search_feedback` | 明示・暗黙評価、評価主体、理由 |
| `search_outcomes` | Marginalis ノートへの利用等、検索後の成果 |

埋め込みモデルを変更するときは既存ベクトルを上書きせず、別の `model_id` として並行生成する。
評価後に有効モデルを切り替え、旧モデルへ戻せるようにする。

## 取込と整合性

Marginalis が将来 PostgreSQL を正本にする場合、ノート変更と同じ transaction で transactional
outbox に変更イベントを記録する。検索ハブのコネクターはイベントを少なくとも一回受け取り、
`source_id`、`external_id`、`revision` により冪等に処理する。

イベントを提供しないデータソースは、更新日時、エクスポートの内容ハッシュ、定期全件照合を
組み合わせる。コネクター間で配信方式は異なっても、検索カタログ以降の処理を共通化する。

```text
SourceRecord upsert/delete
       │
       ▼
Resource と Chunk を transaction で更新
       │
       ├─ 埋め込み生成 job
       └─ Elasticsearch 反映 job
                │
                ▼
          revision を確認して公開
```

全文投影と埋め込み投影には一時的な遅延を許す。削除と権限縮小は優先して処理し、結果返却時にも
PostgreSQL の現在の Resource 状態と権限を検証する。

## 検索能力

### 再発見

`search_research` は検索語と任意の出典、Resource 種別、著者、年、タグを受け取る。
Elasticsearch による題名、著者、識別子、タグ、本文の語彙検索と、pgvector による意味検索から
候補を取得する。初期統合はスコア尺度に依存しない Reciprocal Rank Fusion 等を使い、学習済み
reranker の導入後も元の各順位とスコアを特徴量として保持する。

### 関連探索

`find_related` は一つの `resource_id` を起点にする。起点 Resource の代表 Chunk または複数
Chunk を使った意味的類似性、メタデータ、明示的な参照関係から関連 Resource を返す。

どちらも内部では Chunk 単位で候補を得るが、外部結果は Resource 単位に集約する。AI
エージェントが関連性を検討できるよう、安全な短い一致箇所と位置を付けられる設計にする。

```text
search_research(query, filters, limit)
find_related(resource_id, filters, limit)
```

フロンティア探索はこの二つの実利用と評価データが蓄積した後に再評価する。

## MCP と研究メモへの還流

検索ハブは OAuth で保護した独立 MCP server とする。Marginalis の MCP はノートの正本を
作成・更新する境界として維持する。

典型的な処理は次のとおりである。

1. AI エージェントが検索ハブの `search_research` または `find_related` を呼び出す。
2. 検索ハブは `search_id`、`result_id`、Resource のメタデータ、短い一致箇所を返す。
3. 利用者と AI エージェントが候補を検討する。
4. AI エージェントが Marginalis MCP で AsciiDoc ノートを作成または更新する。
5. 検索結果をノート内で引用した場合、AI エージェントが検索ハブへ利用結果を報告する。

```text
record_search_outcome(
  search_id,
  outcome,             // note_created | note_updated | abandoned
  marginalis_note_id,
  marginalis_revision,
  used_resource_ids
)
```

ノートを作成・更新しただけでは、提示した各 Resource の正例とみなさない。AsciiDoc 内で
実際に引用または根拠として利用した Resource だけを `used_resource_ids` に含める。検索結果を
引用せず、議論だけがノート更新の契機になった場合は、検索セッション全体の成功と個々の
Resource の関連性評価を分離して記録する。

### Marginalis 更新イベントによる引用検出

明示報告を基本経路としつつ、Marginalis のノート更新イベントを検索ハブが受け取り、保存された
AsciiDoc から次の識別子を検出する経路を追加する。

- DOI の正規 URL または識別子
- arXiv ID、ISBN 等の外部識別子
- Paperpile 等の安定した Resource URL または外部 ID
- 将来定義する検索ハブの安定した Resource URI

検出した識別子を `resource_identifiers` と照合し、ノート ID、ノート revision、Resource ID の
利用関係を冪等に保存する。明示報告と自動検出が一致すれば、引用された Resource の強い正例と
する。不一致は監査可能な状態で保持し、自動的に負例へ変換しない。

更新イベントだけでは、どの検索セッションが引用の契機だったかを常に特定できない。検索との
因果を学習に使う場合は `record_search_outcome` の `search_id` を正とし、自動検出は引用利用の
検証、報告漏れの補完、長期的な Resource 利用実績に使う。

将来、検索来歴を AsciiDoc 内へ機械判読可能に保存する場合も、通常の DOI や正規 URL による引用を
妨げない。具体的な AsciiDoc 表現は Marginalis のノートプロファイル、export、外部可搬性を検討
して別 Issue で決める。

## フィードバック

フィードバックは評価主体と取得方法を区別する。

```text
selected_by
  human
  agent
  agent_confirmed_by_human

feedback_kind
  explicit
  implicit

judgment
  relevant
  irrelevant
  wrong_intent
  outdated
  duplicate
  known_but_relevant
```

表示されたが選択されなかった候補を、そのまま負例にしない。上位ほど閲覧・選択されやすい
位置バイアスがあるため、全提示候補、提示順位、候補生成器、モデル版を
`search_impressions` に記録する。

信号の強さは概ね次の順とする。

```text
人間による明示的な正例・負例
  > 人間が承認し Marginalis で引用した Resource
  > AI エージェントが引用し、人間がノートを承認
  > AI エージェントによる選択
  > 単なる表示、クリック、取得
```

AI エージェントの自律選択には、利用者の嗜好だけでなくエージェントとプロンプトの特性が含まれる。
`actor_kind`、MCP client、agent/model 識別情報、利用者の承認有無を、秘密情報を含まない範囲で
保存する。

## 個人向け学習

初期学習対象は、埋め込み基盤モデルではなく候補統合後の軽量な reranker とする。特徴量の候補は
次のとおりである。

- Elasticsearch の順位、BM25、題名・著者・タグ・識別子の一致
- pgvector の順位と距離
- Resource 種別、出典、出版年、更新日時
- 検索語と Resource の言語
- 明示的な個人辞書、同義語、出典・種別の選好
- Marginalis からの過去の引用・参照実績

個人利用では学習データが疎であるため、線形モデルまたは勾配ブースティング等の解釈・版管理が
容易なモデルから開始する。日次または週次の batch 学習、オフライン評価、shadow ranking を経て
有効化し、直前のモデルへ戻せるようにする。

将来、十分な明示評価が蓄積した場合は、検索語、引用された Chunk、意味的には近いが明示的に
不適切とされた Chunk の組を hard negative として、reranker または埋め込みモデルの調整を
再評価する。

## 成功指標

主な成果は、検索が Marginalis の研究メモの新規作成または実質的な更新につながることである。
ただし不要な更新を増やす方向へ最適化しないよう、段階ごとに測定する。

| 段階 | 指標 |
| --- | --- |
| 候補品質 | 明示的な relevant / irrelevant、MRR、NDCG |
| 選択 | 人間が採用した候補、AI の候補を人間が承認した割合 |
| 引用利用 | 検索 Resource が AsciiDoc 内で引用された割合 |
| 研究記録 | `note_created` / `note_updated` へ到達した検索セッション |
| 継続価値 | 作成・更新したノートが後日再利用、参照、再更新されたか |

ノート更新の回数だけを単独の最適化目標にしない。引用された Resource、明示評価、後日の再利用を
合わせて評価する。

## 認証、個人利用、研究室利用

初期版は一つの個人 workspace とするが、主要データには `workspace_id`、`owner_subject`、
`created_by`、`visibility` を持たせる。将来の研究室利用では、各データソースの権限を
`resource_permissions` へ投影し、結果返却時に現在の権限を検証する。

学習データと個人モデルは subject ごとに分離する。研究室の共有モデルを導入する場合も、個人の
検索語、明示評価、閲覧履歴を本人の選択なく共有学習へ混ぜない。

## 段階的な実装候補

1. 共通 `SourceRecord`、PostgreSQL 検索カタログ、全件再構築を定義する。
2. Marginalis コネクターと Paperpile BibTeX コネクターを実装する。
3. Elasticsearch の語彙検索、pgvector の意味検索、固定 RRF を実装する。
4. `search_research`、`find_related`、明示的な `record_search_outcome` を MCP で提供する。
5. Marginalis 更新イベントと AsciiDoc 引用検出を実装し、明示報告を検証・補完する。
6. 人間と AI エージェントを区別したフィードバック UI / MCP を追加する。
7. 固定評価セットと shadow ranking を整備してから、個人向け reranker を有効化する。
8. PDF・OCR コネクター、追加文献管理ツール、研究室 workspace を利用実績に応じて追加する。
9. フロンティア探索は、再発見・関連探索の品質と学習データを確認した後に再評価する。

## 未決事項

- 検索ハブを Marginalis と同じ repository の別 service とするか、独立製品とするか。
- Resource と Chunk の公開 ID、安定 URI、削除後の tombstone の保持期間。
- AsciiDoc 内で DOI、外部 Resource、検索来歴を表す可搬な形式。
- Paperpile の BibTeX entry と Google Drive 上の PDF を安定して対応付ける方法。
- 日本語と英語を含む埋め込みモデル、次元、距離尺度、chunking の評価基準。
- Elasticsearch と pgvector の候補数、RRF、フィルター後の再探索に関する性能基準。
- 検索語、フィードバック、AI エージェント利用履歴の保持期間、export、削除方法。
- 研究室 workspace における共有索引、個人モデル、共有モデルの境界。
