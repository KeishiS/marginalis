# ロードマップ

## 現在地

`v0.3.1`は、SQLite正本、Kanidm 1.10、REST API v2、OAuthで保護したMCP、閲覧用Web UI、
NixOS moduleという`v0.3.0`の公開境界を維持しながら、backup・隔離復元、protocol回帰試験、
運用診断を強化し、2026-07-27に公開した。

現行実装はAdocWeave `v0.6.1`へ完全一致で固定している。一方、次の執筆支援で必要になる診断、
位置情報、Resource解決およびHTML出力の契約はAdocWeave `v0.10.1`までに更新されている。
したがって、旧契約の上に執筆支援を追加せず、`v0.4.0`でAsciiDoc契約と診断を一体として
再基準化する。

着手順と公開単位はこの文書を正とする。個別の受入条件はGitHub Issuesで管理する。
既存の`issues/`は完了済み判断の履歴として参照し、移行完了後に削除する。

## 設計方針

- 移行コストと後方互換性を設計判断の制約にしない。ただし、複雑さ、安全性または将来の変更容易性を
  改善しない破壊的変更は行わない。
- AdocWeaveの解析結果、診断、Rust API、conformance fixtureおよび配布物は、同じパッケージ版へ
  完全一致で固定する。WASMを導入する場合は、同じ版を使う独立した成果物契約として扱う。
- AdocWeaveが解析できる構文と、Marginalisが保存・表示を許可するノートprofileを分離する。
- 入力規則と診断コードの正本を一つにし、REST、MCP、文書およびtool schemaへ投影する。
- 検索、Web編集、グラフ、添付Resource、別database backendは、実利用から必要性を確認してから
  独立した公開単位で追加する。
- 小規模運用では新しい常駐基盤を安易に増やさず、systemd、journald、SQLiteおよび既存の
  release gateを再利用する。

## 優先順

| 段階 | 対象 | 目的 | 次段階へ進む条件 |
| --- | --- | --- | --- |
| 0（完了） | `v0.3.0`、`v0.3.1` | 現行アーキテクチャの公開と運用堅牢化 | 完了（2026-07-27、`v0.3.1`タグ） |
| 1 | 文書とIssueの管理境界 | 文書の役割を整理し、作業管理をGitHub Issuesへ移す | 正本、履歴、作業項目の置き場所が明確になり、`issues/`を削除できる |
| 2 | AdocWeave `v0.10.1` | AsciiDoc解析・描画契約を次の執筆支援の基準へ更新する | Rust、Nix、fixtureの版が一致し、`v0.6.1`との差分を固定例で説明できる |
| 3 | 保存形式v2とノートprofile | パーサー版とMarginalis固有の入力規則を独立して識別する | v1を安全側に拒否し、package版とprofile版の不一致を検出できる |
| 4 | 共通診断と執筆profile | AI clientが入力前に規則を取得し、失敗原因と位置を機械判定できるようにする | RESTとMCPが同じ診断を返し、実clientの固定シナリオが成功する |
| 5 | 認可・認証モデルの判断 | 利用不能なACLと自前OAuth Authorization Serverの将来像を確定する | 共有要件と実client接続試験に基づくADRが承認される |
| 6 | 認可・認証モデルの実装 | 段階5で選んだ単純なモデルへ破壊的に移行する | 不要な永続状態と公開境界が削除され、権限変更と失効を自動試験できる |
| 7 | 検索評価 | 現行検索で再発見できない固定例から最小の改善を選ぶ | 評価用入力と期待結果があり、現行方式の不足を再現できる |

段階1は公開を伴わないリポジトリ保守とする。段階2から4を`v0.4.0`、段階5から6を
`v0.5.0`の候補範囲とする。段階7以降の版は評価結果から決める。

## v0.4.0のAsciiDoc契約

### AdocWeave更新

AdocWeave `v0.6.1`から`v0.10.1`へ一括して更新する。中間版を配布せず、各版の変更を
固定した入力と期待結果へ対応付ける。

- `v0.7.0`のsemantic model、見出しID位置および公開text API
- `v0.9.0`の等幅文字境界と表header推論
- `v0.10.0`の型付きrender診断、Resource用途および検証済みMIME type
- `v0.10.1`の相対targetのLint規則とHTML5適合出力

依存commit、`adocweave::VERSION`、`Cargo.lock`、Nixの依存hash、conformance fixtureを
完全一致させる。現行MarginalisはWASM成果物を配布しない。HTMLのbyte列、DOM、解析結果または
診断が変わる入力は、差分を明示して
期待結果を更新する。

### 保存形式とノートprofile

保存・archive形式はv2へ更新し、v1を読み込まない。同じ形式番号を異なる意味へ再定義しない。
形式には少なくともAdocWeave package版とMarginalis note profile版を記録し、どちらかが
実行時の期待値と異なる場合は安全側に拒否する。

AdocWeaveが通常のLintで受理する相対linkや文書間xrefを、Marginalisが自動的に許可するとは
みなさない。`v0.4.0`では相対link、includeおよび外部Resourceを引き続き禁止する。
ノート間参照と添付Resourceは、参照先、ACLおよびURL解決の責務を定義する後続作業で扱う。

### 診断と執筆profile

AdocWeaveの診断を文字列へ縮退させず、Marginalis固有の入力規則とともに通信方式に依存しない
診断型へ変換する。診断は少なくとも、安定したcode、対象field、任意のUTF-8 byte範囲を持つ。
タイトルやタグのように本文位置を持たない違反へ`0..0`の疑似位置を割り当てない。

MCPでは、JSONまたはtool引数自体が不正な場合だけJSON-RPC errorを返す。正しいtool呼び出しで
ノート検証に失敗した場合は、`isError: true`のtool resultと`structuredContent`に診断一覧を返す。
RESTは同じ診断型をエラー応答へ投影する。

`get_note_profile`は、検証器と同じ規則カタログから生成した機械可読な入力規則と短い動作例を返す。
同じ内容を文書、tool schema、RESTおよびMCPへ重複して手書きしない。

## v0.5.0候補の認可・認証再設計

### ACL

現在のノートACLはdatabaseとarchiveに存在するが、RESTとMCPから管理できない。この中間状態を
維持せず、段階5で次のどちらかを選ぶ。

- 具体的な共有要件がなければ、ノート単位ACLを削除し、所有者と`server-admins`へ単純化する。
- 共有を製品要件とする場合は、group単位の権限管理を正式なuse case、REST、MCPおよび受入試験として
  一度に実装する。

### OAuth Authorization Server

自前Authorization Serverを維持するかは、ChatGPT、Claude Code、Codex CLIを使った期間限定の
接続試験で決める。外部Authorization ServerがDynamic Client Registration、必要なsubject・group・
audienceおよび短命tokenを提供し、すべての対象clientが接続できる場合は、自前の登録、同意、code、
access token、refresh tokenおよび関連tableを削除する。

外部化できない場合は自前実装を維持し、未認証client登録の上限枯渇、権限変更後も残る
authorization snapshot、token寿命および再評価時期を優先して修正する。

時刻と乱数はcomposition rootから明示的に注入し、失効、rotationおよびrevision生成を決定的に
試験できるようにする。

## 条件付きの候補

- **検索拡張**: 現行MCPには検索toolがないため、最初にACLを守る最小の`search_notes`が必要かを
  判断する。日本語・英語の再発見に失敗する固定例を集めてから、FTS5、表記揺れ、trigram、
  意味検索の順に比較する。raw検索語を無断でlogへ保存しない。
- **ノート間参照**: 一覧と検索だけでは関係を辿れない実例を得た後、ノートID、anchor、ACLおよび
  URL解決を一つの契約として設計する。
- **添付Resource**: 保存先、MIME type、容量、ACLおよびbackupを定義できる段階まで、
  AdocWeaveのmedia Resourceを有効化しない。
- **Web編集とグラフ**: それぞれ独立した需要と受入例を得てから別の公開単位で扱う。
- **PostgreSQL**: 複数process、高可用性、または現在の規模を超える運用要件が発生した場合だけ
  検討する。
- **リポジトリ文書のAsciiDoc化**: 大量の形式差分に見合う保守上の便益が確認されるまで着手しない。

## 継続監視

- ChatGPT、Claude Code、Codex CLIのMCP接続とtool resultの解釈
- AdocWeaveのpackage版、公開契約、WASM schemaおよびMarginalis note profile
- backupの最終成功時刻、保存世代数、四半期復元試験の結果
- database容量、ノート数、主要操作の失敗およびrevision conflict
- 入力検証による再試行と、検索で見つからなかった固定例

各公開では`cargo make release-gate`と変更範囲に応じた実環境受入を実施する。公開API、
MCP tool、NixOS option、archive形式またはAdocWeave契約を変更する場合は、実装と同じ
Pull Requestで仕様文書、動作例および受入手順を更新する。
