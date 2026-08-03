# 変更履歴

この文書には利用者に影響する変更だけを記録する。
公開 API、データフォーマット、NixOSモジュールの動作を変えない内部的な再構成は記載しない。

## 0.25.0 — 未公開

### 破壊的変更

- MCPのAuthorization Serverを内蔵した。外部のAuthorization Serverは使用しない。判断の内容は
  [MCPのAuthorization Serverを内蔵する](docs/adr/0007-mcpのauthorization-serverを内蔵する.md)に
  記録した。利用者の認証は従来どおりKanidmが行い、MCPのclient登録、認可code、access token、
  refresh tokenはMarginalisが管理する。更新前に発行したMCPのtokenとclient登録は移行しないため、
  更新後にMCPクライアントの接続を作り直す。
- MCPの有効・無効を`MARGINALIS_MCP_ENABLE`で指定するようにした。値は`true`または`false`とし、
  それ以外は起動時に拒否する。外部Authorization Server用の`MARGINALIS_MCP_AUTHORIZATION_ISSUER`、
  `MARGINALIS_MCP_UPSTREAM_ISSUER_CLAIM`、`MARGINALIS_MCP_UPSTREAM_SUBJECT_CLAIM`、
  `MARGINALIS_MCP_GROUPS_CLAIM`は廃止した。NixOSモジュールが値を組み立てるため、
  `services.marginalis.mcp.enable`を使う場合は設定の変更が不要である。
- NixOSモジュールの`services.marginalis.mcp.authorization`を削除した。この設定を書いている場合は
  取り除く。issuerと各endpointは`baseUrl`から自動的に導く。
- SQLiteスキーマを13から15へ更新した。MCPのclient、認可code、tokenを保存する表と、認可要求で
  `redirect_uri`が指定されたかを記録する項目を追加したため、
  稼働中の旧データベースファイルはそのまま使用できない。更新前に`export-archive`を実行し、
  新しい版で空の`dataDir`へ`import-archive`する。archiveの形式は13のまま変わらない。
- MCPの起動条件を変更した。認可に外部サービスを使わなくなったため、起動可否はKanidmの
  discoveryだけで決まる。以前は外部Authorization Serverのdiscoveryに失敗すると起動しなかった。

### 追加

- コードブロックの言語として`python`を指定できるようにした。`get_note_profile`の
  `allowed_source_languages`にも同じ値を追加し、執筆プロファイルの版を14へ上げた。
- MCPクライアントの登録方式としてClient ID Metadata Documentに対応した。HTTPSのURLを`client_id`と
  して使い、そのURLでclient名と`redirect_uris`を公開する方式である。事前登録を必要とせず、DCRとは
  登録上限を分けて管理する。有効な文書だけをHTTPのcache指示に従って保持し、失敗や不正な文書は
  保持しない。同じclientへの同時要求は一つの取得結果を共有し、全体の同時実行数と取得回数、DNSを
  含む処理時間を制限する。認可GET内では解決結果を検証にも使い、不必要な再取得を行わない。
- `DELETE /api/v3/mcp-authorizations/{client_id}`を追加した。利用者本人のsessionとCSRFトークンで、
  MCPクライアント単位に認可を取り消す。対象のaccess tokenとrefresh tokenをtoken family単位で
  直ちに失効する。
- Authorization Server Metadataを`/.well-known/oauth-authorization-server`で公開した。RFC 7009の
  token失効endpointとDynamic Client Registration endpointも併せて公開する。
- `marginalis purge-expired`が、期限切れのMCP認可code、token、参照されていない古いclientも
  削除するようにした。削除件数は構造化ログに記録する。

### 修正

- Client ID Metadata Documentが`token_endpoint_auth_methods_supported`で複数のclient認証方式を
  示す場合も、Marginalisが対応する`none`を選べるようにした。従来の単数形も引き続き受理するため、
  ChatGPTと既存のMCPクライアントの両方を使用できる。
- Client ID Metadata Documentの取得と検証に失敗した場合、取得先のhostと固定した理由を構造化ログへ
  記録するようにした。client IDのpath、redirect URI、OAuthパラメーター、応答本文は記録しない。

## 0.24.0 — 2026-08-01

### 追加

- 既存ノートの編集プレビュー用に`POST /api/v3/notes/{note_id}/preview`を追加した。新規作成用の
  `POST /api/v3/notes/preview`と分けることで、対象ノートの認可と所有者をサーバーで判断する。

### 修正

- 共有されたノートの編集プレビューが、編集者ではなくノート所有者の書誌ライブラリーを使って
  引用を解決するようにした。閲覧画面および保存後の表示と同じ結果になり、閲覧権限だけを持つ
  利用者には編集プレビューを返さない。
- archiveと文書の書き出し中に処理が失敗しても、途中まで書いたファイルが完成品として残らない
  ようにした。書き出し先と同じディレクトリの一時ファイルを検査し、完了後に確定する。
- 書誌ライブラリーで検索結果の到着順が入れ替わった場合に、古い結果が新しい検索結果を上書き
  しないようにした。登録、更新、削除の成功通知も、その後の一覧更新で消えないようにした。
- 共有設定の保存を連続して開始できないようにし、保存中の操作を無効にした。
- 数式を含む表示内容を素早く切り替えた場合に、以前のMathJax組版結果が新しい内容を上書き
  しないようにした。
- 関係の図と書誌ライブラリーの検索で、`%`と`_`をワイルドカードではなく入力された文字として
  扱うようにした。

## 0.23.0 — 2026-08-01

### 破壊的変更

- ノートのタグを並べる文書属性の名前を`:tags:`から`:marginalis-tags:`へ変更した。Marginalisが
  独自に決めた文書属性は`marginalis-`で始める規則にしたためである。以前の`:tags:`を書いた
  ノートは、対応していない文書属性として保存を拒否する。既存のarchiveは`migrate-archive`が
  文書headerの属性名を書き換えるため、そのまま取り込める。archiveのnote profile版は5になる。
- AdocWeaveを0.27.0へ更新した。`get_note_profile`とarchiveが記録する
  `adocweave_package_version`が`0.27.0`になる。`0.23.0`を記録した既存のarchiveは
  `migrate-archive`で変換する。ノートの受理範囲と描画したHTMLは変わらない。判断の内容は
  [AdocWeave 0.27移行判断](docs/adocweave-v0.27-migration.md)に記録した。
- `get_note_profile`が返すnote profile版を11から13へ更新した。`syntax`へ、文書headerに書ける
  属性の名前を並べた`allowed_document_attributes`と、引用スタイルとして選べる値を並べた
  `allowed_citation_styles`が加わる。文書例も2件加わる。
- `include`、`ifdef`、`ifndef`、`ifeval`を書いたノートを拒否するときの診断codeを
  `preprocessor_directive_disabled`へ変更した。条件分岐はこれまで、`ifeval:`がURLのschemeに
  見えるという理由で`invalid_url_scheme`として拒否されていた。これらのdirectiveを受理しない
  方針は変わらない。

### 追加

- 引用の見せ方を、文書属性`:marginalis-citation-style:`でノートごとに選べるようにした。
  `author-year`は本文へ`(Smith 2024)`、`numeric`は本文での初出順の番号で`[1]`と表示する。
  属性を書かないノートは`author-year`になり、表示は今までと変わらない。`numeric`では参考
  文献一覧の項目にも同じ番号が見出しとして付き、本文の引用と一覧の項目は今までどおり相互に
  移動できる。番号は書誌情報の記述には混ざらない。選べる値以外を書いた場合は
  `unsupported_citation_style`として保存を拒否する。任意のCSLスタイル名は受け付けず、
  サーバー上でCSLを実行しない。

### 修正

- `get_note_profile`が、使用できる文書属性を伝えていなかった。「対応していない文書属性は
  拒否される」という規則だけを広告しており、何が使えるかを知る手段がなかったため、MCP経由で
  作るノートには`:sectnums:`や`:toc:`が入らなかった。許可する属性の正本をノート入力規則へ置き、
  入力検査と広告の両方をそこから導くようにした。

## 0.22.0 — 2026-08-01

### 破壊的変更

- 引用の索引を保存するため、SQLite schemaを13へ更新した。既存のdataDirはそのまま使えない。
  更新前の版で`export-archive`を実行し、新しい版で空のdataDirへ`import-archive`で取り込む。
  archiveの形式は変わらないため`migrate-archive`は不要である。

### 追加

- ノート間の参照と、ノートから文献への引用を図として表示する「関係の図」を追加した。点を選ぶと
  ノートは閲覧画面、文献は書誌ライブラリーへ移動する。題名、本文、タグに語を含むノートだけへ
  絞り込める。図と同じ内容をつながりの多い順に並べた一覧としても示す。
- 閲覧画面の「周辺の関係」から、そのノートを起点にした関係の図を開けるようにした。起点から
  辿る階層数は図の上で選び直せる。
- 関係の図で点に触れると、ノートは更新日時とタグ、文献はcitation keyと題名を吹き出しで示す
  ようにした。マウスのホバーとキーボードのフォーカスの両方で出る。同じ内容は点の名前としても
  支援技術へ伝える。
- 図の描画は経路へ入ってはじめて読み込む。図を開かない利用者の読み込み量は増えない。

### 修正

- ビルドしたJavaScriptとCSSを、ファイル名を書き並べずに配信するようにした。分割読み込みで増える
  chunkの名前には内容のhashが付くため、書き並べる方式では新しい出力が配信されず404になっていた。
  関係の図はこの経路で読み込むため、画面全体が空になっていた。
- `import-documents`が、内容の壊れていない書き出しを`archive logical round-trip validation
  failed`として中止することがあった。取り込み前の自己検査が、内容の一部ではない要素の並びまで
  比べていたためである。利用者が2人以上いる場合はnote IDが所有者をまたいで交互に並ぶため、
  通常の書き出しでも起きた。書誌情報では、citation keyの順とitem IDの順が食い違うと所有者が
  1人でも起きた。比較の前に両側を同じ規則で並べ替えるようにした。内容が実際に違う場合は、
  従来どおりデータベースへ触れずに中止する。

## 0.21.0 — 2026-07-31

### 追加

- 書誌ライブラリーで、文献カードのcitation keyや題名を選ぶと編集を始められるようにした。
  「編集」ボタンは情報部分そのものが操作になったため取り除いた。編集中のカードは左端の帯で
  示す。未保存のまま別のカードを選ぶと、確認なしで切り替わる。

### 変更

- 閲覧画面の本文領域の上限を72remから96remへ広げた。幅の広い画面で、表、コード、数式を
  横スクロールなしで読める場面が増える。ヘッダーと本文の左右端がそろうよう、外側の上限も
  98.5remへそろえた。一覧、編集、共有設定、書誌ライブラリーも同じ上限になる。
- 編集画面だけ本文領域を100remまで広げていた指定を取り除いた。上限が全画面で98.5remにそろい、
  幅1,576px以上の画面でヘッダーと編集領域の左右端が一致する。編集領域は最大24px狭くなる。
- 余白、文字の大きさ、境界線の太さに目盛りを定め、CSSを役割ごとの6ファイルへ分けた。近い値が
  少しずつ増えないようにし、目盛りから外れた値をstylelintで拒否する。数pxの差が出る箇所がある。
- 読み込み中、該当項目なし、成功の知らせを全画面で同じ見た目にそろえた。利用者の対応が要る
  失敗は別の見た目にし、支援技術へも割り込んで伝える。書誌ライブラリーでは失敗も成功と同じ
  扱いで穏やかに伝えていた。
- 書誌ライブラリーで、登録・更新にアクセント色、削除に警告色を使うようにした。他の画面と同じ
  強調の規則にそろえる。書誌情報の削除は取り消せないため、補助操作と同じ見た目にしない。

### 修正

- 書誌ライブラリーの文献カードで、「編集」がカードの中央付近に置かれていた。操作をひとまとまり
  にし、「編集」と「削除」が隣り合って右端に並ぶようにした。

## 0.20.0 — 2026-07-31

### 追加

- ノートをAsciiDocファイル、書誌情報をCSL-JSONの配列として一度に書き出す`export-documents`を
  追加した。所有者ごとにディレクトリーを分け、ファイル名は題名とnote IDを並べる。削除済み
  ノートは書き出さない。出力は`tar.xz`形式の書庫1つで、展開すると出力ファイル名の
  ディレクトリーが作られる。`manifest.json`は各ファイルとnote IDの対応、所有者、日時、revision、
  タグ、ACLに加えて、形式名、Marginalisの版、AdocWeave packageの版、ノート受理規則の版を持つ。
  取り込む側はこの版情報で移行の要否を判断できる。
- 書き出した書庫を取り込む`import-documents`を追加した。本文の正は`.adoc`ファイル、識別子と
  日時の正はmanifestとし、別の道具で編集した本文を戻せる。manifestの版が稼働中の値と違う場合は
  全ノートを現行規則で再検証し、満たさないノートがある場合はdatabaseを変更しない。取り込み先は
  空のdatabaseに限る。日次の退避と復元は従来どおりarchiveで行う。

### 修正

- 共有されたノートを別の利用者が更新する場合に、引用の未登録判定が操作している利用者の
  書誌ライブラリーで行われていた。閲覧時は作成者のライブラリーで解決するため、保存できた
  引用が表示ではcitation keyのままになることがあった。更新時も作成者のライブラリーで
  判定するようにそろえた。
- `get_note_profile`が公開する参考文献の文書例が、1行100文字を超えて`line-too-long`の警告に
  なっていた。MCPの書き込みは警告があるとノートを変更しないため、公開した例をそのまま
  保存できなかった。例を折り返し、note profile版を10へ更新した。

## 0.19.0 — 2026-07-31

### 追加

- 本文の`cite:[key]`を書誌ライブラリーで解決し、`(Smith 2024)`のような著者・年の表示にする
  ようにした。引用した文献だけを重複なく並べた参考文献一覧を表示時に組み立て、本文の引用と
  一覧の項目を相互に移動できる。一覧は保存する本文へ書き込まない。
- 引用の解決にはノートを作成した利用者のライブラリーを使う。共有したノートは誰が見ても同じ
  表示になる。判断は[ADR 0006](docs/adr/0006-引用はノート作成者の書誌ライブラリーで解決する.md)に
  記録した。
- ライブラリーに無いcitation keyを`unknown_citation_key`の警告として報告するようにした。
  Web UIとREST APIでは保存でき、MCPでは警告を拒否する既定の方針どおり保存されない。

### 破壊的変更

- AdocWeave package版を0.23.0へ更新した。記録するAdocWeave版が変わるため、archiveを
  `marginalis-archive-13`へ更新した。archive 7、8、9、10、11、12は`migrate-archive`で
  全件再検証してarchive 13へ変換できる。判断は
  [0.23移行判断](docs/adocweave-v0.23-migration.md)に記録した。
- MCPとOpenAPIが示すnote profileを9へ更新した。`cite:`の受理と、引用を含む文書例および
  執筆時の注意事項が加わる。
- ノートの入力規則の正本を一か所へ集約した。受理する入力は変わらないが、`get_note_profile`が
  広告する内容の由来が変わるため、上記のnote profile版に含めて公開する。
- ノートIDと書誌項目IDの公開JSON Schemaの`pattern`をUUID形式へ厳格化した。従来は36文字の
  16進数とハイフンの並びであれば通過したため、実装が受理する規則より緩かった。実際に受理する
  値は変わらない。
- MCP toolが失敗したときの`structuredContent`を、REST APIと同じ失敗表現へそろえた。同じ失敗に
  対する`code`と`message`が接続方法によって変わらない。`get_note`が見つからない場合の`message`は
  `note was not found`から`note is not available`へ、書誌の`add_bibliography_items`で項目ごとに
  返す`message`は`CSL-JSON must be an object with valid id and type fields`から
  `CSL-JSON must contain valid id and type fields`へ変わる。
- すべてのMCP toolの`outputSchema`が、成功出力と失敗出力の選択になった。以前は`create_note`と
  `update_note`だけが失敗出力を宣言しており、他のtoolのschemaは実行時の失敗応答を表していなかった。
- serviceが読み取る環境変数の接頭辞を`MARGINALIS_`へ統一した。`OIDC_ISSUER_URL`、
  `OIDC_CLIENT_ID`、`OIDC_CLIENT_SECRET`、`OIDC_CLIENT_SECRET_FILE`、
  `OIDC_CA_CERTIFICATE_FILE`は、それぞれ`MARGINALIS_`を付けた名前へ変更した。NixOSモジュールが
  値を組み立てるため、`services.marginalis`のoptionを使う場合は設定の変更が不要である。
  環境変数を直接指定している場合は名前を変更する。
- `MARGINALIS_MCP_ENABLE`を廃止した。MCPの有効・無効は
  `MARGINALIS_MCP_AUTHORIZATION_ISSUER`の設定有無で決まる。NixOSモジュールの
  `services.marginalis.mcp.enable`は従来どおり使用できる。
- `marginalis diagnose`が出力する`configuration`の形式を変更した。環境変数名を鍵とする
  `variables`と、判断結果の`mcp_enabled`を出力する。各項目は`set`と`required`を持ち、
  秘密でも保存先でもない変数にだけ`value`が付く。

### 変更

- エラー型の表現を`thiserror`で宣言するようにした。`NoteRepositoryError`、
  `AuthenticationUseCaseError`、`SessionRepositoryError`、`IdentityProviderError`など、
  これまで表現を持たなかった型にも記録用の文言が付く。利用者向けの`code`と`message`は
  従来どおりtransport側の写像が決めるため、公開応答は変わらない。
- MCP toolのJSON Schemaで、入力上の問題の位置を表す定義名を`ValidationTargetResponse`から
  `NoteValidationTarget`へ変更した。`{"field": "source"}`のような実際の値の形式は変わらないため、
  `$ref`を解決して利用するクライアントへの影響はない。schemaから型を生成している場合は、
  生成した型名が変わる。

### 修正

- 空白だけを設定した環境変数について、`diagnose`は「未設定」、起動処理は「設定済み」と
  判断が食い違っていた。どちらも未設定として扱うよう統一した。

## 0.18.0 — 2026-07-30

### 破壊的変更

- 利用者ごとのCSL-JSON書誌ライブラリーを追加し、SQLite schemaを12、archiveを
  `marginalis-archive-11`へ更新した。旧データベースは自動移行せず、archive 10を
  `migrate-archive`でarchive 11へ変換して空のデータベースへ取り込む。

### 追加

- Web UIとREST APIからCSL-JSON書誌情報を検索、登録、編集、削除できるようにした。MCPでは
  検索、1件または最大100件の一括登録、削除を提供する。一括登録は成功項目と入力位置付きの失敗を
  分けて返す。citation keyは利用者ごとに一意とし、登録値を推測または補完しない。

## 0.17.0 — 2026-07-30

### 追加

- MCP仕様`2026-07-28`の自己完結したrequest metadata、`server/discover`、標準HTTP header、
  `resultType`、cache情報に対応した。`2025-11-25`と`2025-03-26`の初期化方式も移行期間中は
  維持する。

## 0.16.1 — 2026-07-30

### 追加

- Noto Sans JP、Noto Serif JP、Noto Sans Monoを文字範囲ごとに分割したWeb字体として同梱し、
  操作画面、閲覧本文、編集欄、コードへ用途別に適用した。外部の字体配信サービスへ接続せず、
  利用者の端末に字体がない場合も同じ表示を使用する。
- 編集欄の書体と複数行選択を、キーボード操作とマウス操作、ライト表示とダーク表示について
  ChromiumとFirefoxで検査するブラウザー試験を追加した。

### 修正

- AdocWeaveを0.20.0へ更新し、順序付き・順序なしリストの項目本文を空行なしで複数行へ
  折り返せるようにした。hard break、ノート参照、警告の位置も継続行に対応し、authoring
  profile版を6へ更新した。
- 編集欄の選択色をアクティブ行より優先し、複数行を選択した場合も最終行まで明瞭に表示するように
  した。本文と行番号には同じ等幅書体を適用し、文字、カーソル、選択範囲の位置をそろえた。

### 破壊的変更

- AdocWeave package版の更新に合わせ、archiveを`marginalis-archive-10`へ更新した。
  archive 7、8、9は、`migrate-archive`で全件再検証してarchive 10へ変換できる。

## 0.16.0 — 2026-07-30

### 破壊的変更

- AdocWeaveを0.19.0へ更新した。SQLite schema 11とnote profile版4は維持し、archiveを
  `marginalis-archive-9`へ更新した。v0.15.0のarchive 8とv0.10.0のarchive 7は、
  `migrate-archive`で全件再検証してarchive 9へ変換できる。

### 追加

- 閲覧画面で、note IDの隣にノートのタグを文書内の順序で表示するようにした。タグが長い場合や
  画面が狭い場合も、操作欄からはみ出さずに折り返す。

### 変更

- MathJaxの遅延読み込み用字体をMarginalisから配信し、外部CDNへ接続せずに数式を組版するように
  した。MathJaxが生成するstyle要素にもContent Security Policyのnonceを設定する。

## 0.15.0 — 2026-07-29

### 追加

- Web UIへ共通ヘッダーと一貫した画面構成を導入し、ノート一覧、編集、アクセス制御の表示を
  整えた。ライト・ダーク表示と狭い画面に対応し、主要画面を画像比較テストで検査する。
- `Ctrl+S`または`Command+S`で保存に成功した場合、結果を画面右上のトーストで通知するように
  した。保存の失敗と競合は、操作に必要な情報を確認できるよう画面内へ継続して表示する。

### 変更

- フロントエンドのパッケージ管理をnpmからpnpmへ変更した。固定したpnpm版をローカル開発、
  CI、Nixによる配布物の生成で共通して使用する。

## 0.14.0 — 2026-07-29

### 破壊的変更

- 構造化ログの監視契約を更新した。すべてのproductionログへ固定した`event`を付け、HTTPの実pathを
  route templateへ変更し、保守処理のevent名と失敗eventを統一した。旧eventとfieldの対応は
  [ログと障害診断](docs/observability.md)に記載する。

### 追加

- HTTP、MCP、OIDC、service、保守処理の結果と安全に記録できるfieldを定義し、実装と文書の
  event一覧、機密情報を表すfield、未正規化URIをCIで検査するようにした。
- CI jobの責務、ローカルでの再実行方法、失敗時に残す秘密情報除去済みの証拠を文書化した。
- Web UIのAsciiDoc編集欄へCodeMirror 6を採用し、行番号、検索、編集履歴、Tabによる字下げ、
  `Ctrl+S`と`Command+S`による保存を追加した。
- 執筆、左右分割、プレビューの表示切り替えと、分割幅の調整を追加した。狭い画面では執筆と
  プレビューを明示的に切り替え、カーソル、選択範囲、編集履歴、スクロール位置を維持する。
- プレビューと閲覧画面で、すべてのコードブロックへ行番号を表示するようにした。開始行を
  指定した場合は、その番号から表示する。

### 変更

- 日本語IMEの変換中は保存とプレビュー更新を待機し、未確定文字列を変更しないようにした。
  診断を選んだ場合は執筆表示へ切り替え、対象範囲が見える位置へ移動する。
- 執筆表示中に生成された数式を、プレビューまたは分割へ切り替えた時点でMathJaxにより組版し、
  プレビューと分割を続けて切り替えた場合も組版済みの表示を維持するようにした。
- 編集領域が利用可能な画面高を使うようにし、ライト・ダーク表示、320px幅、5,000行の文書を
  ブラウザー試験で確認するようにした。

## 0.12.0 — 2026-07-29

### 破壊的変更

- `POST /api/v3/notes/preview`の成功応答へ必須の`diagnostics`を追加し、入力診断へ必須の
  `severity`を追加した。重大度は`error`、`warning`、`information`、`hint`のいずれかである。
  AdocWeave由来の診断は、`macro-boundary`などAdocWeaveのcodeをそのまま返す。
- MCPとOpenAPIが示すnote profileを5へ更新した。archiveのノート受理規則は版4のままであり、
  `marginalis-archive-8`の互換性は変わらない。

### 追加

- MCPの`get_note_profile`へ、本文から参考文献を参照して相互に移動できる完全なAsciiDoc例を
  追加した。利用者または参照元から得た書誌情報だけを使用し、不明な著者名、題名、発行年、
  DOIなどを推測しない指針も同じ応答で返す。
- 保存を妨げないAdocWeaveの診断を、安全なHTMLと同時に編集画面へ表示するようにした。位置付きの
  診断から入力範囲へ移動でき、入力を修正すると古い診断を直ちに取り除く。

### 修正

- 未対応の古いSQLite schemaを確認する接続が、schemaを拒否する前にデータベースをWAL modeへ
  変更しないようにした。診断はschema不一致とSQL実行失敗を区別し、失敗した検査、分類、
  SQLite result codeを本文や認証情報を含めずに報告する。

## 0.11.0 — 2026-07-29

### 破壊的変更

- AdocWeaveを0.17.0へ更新し、SQLite schemaを11、note profileを4、archiveを
  `marginalis-archive-8`へ更新した。schema 10は直接起動せず、v0.10.0のarchive 7を
  `migrate-archive`で全件再検証してから空のschema 11へ取り込む。OpenAPIが示すAdocWeave版と
  note profile版も同じ値へ更新した。
- 文書属性を出現順に評価し、タグを最終値から導出する。属性参照と複数行値を評価し、
  header後の属性操作と改行を含むタグを拒否する。

### 追加

- archive 7を変更せずにarchive 8へ変換する`migrate-archive`を追加した。一件でも現行規則に
  合わないノートがある場合は出力せず、既存ファイルも上書きしない。失敗した項目は本文や
  識別子をログへ出さず、archive内の位置で示す。

### 変更

- MCPの`list_notes`へタグと更新日時、`get_note`へ更新日時を追加した。MCP toolの入力と出力の
  JSON Schemaを`docs/mcp-tools.json`として公開し、実行時の応答と同じ型から生成する。
- NixOSモジュールを有効にすると、サービスと同じ版の`marginalis`管理コマンドをシステムの
  `PATH`へ追加する。
- コードブロックの題名・言語・行番号指定と数式の言語・表示形式を、AdocWeaveが公開する
  HTML属性から表示する。Web UIはclassや親子関係からこれらを推測しない。

### 修正

- 同じWebセッションから閲覧とプレビューなどを同時に要求した場合も、セッション期限の延長が
  SQLiteのsnapshot競合によって503にならないようにした。

## 0.10.0 — 2026-07-29

### 破壊的変更

- MCPのAuthorization Serverを内蔵実装からAuth0へ変更した。`/oauth/*`と
  `DELETE /api/v3/mcp-authorizations/{client_id}`を削除し、MCPを有効にする場合はNixOSの
  `mcp.authorization`設定を必須とした。
- SQLite schemaを10へ更新し、MCP client、認可code、access token、refresh tokenのテーブルを
  削除した。schema 9以前のdatabaseは自動移行せず、archiveを書き出して空の`dataDir`へ復元する。

### 変更

- Auth0 access tokenの署名、issuer、MCP URLのaudience、scope、Kanidm由来identityとgroupを
  Marginalisで検証する。token拒否と認証基盤障害を区別する診断ログを追加した。

## 0.9.0 — 2026-07-28

### 追加

- Web UIのノート一覧へタグ、更新日時、実効アクセス水準、タグ・更新日の絞り込み、ページ分割を
  追加した。絞り込み条件とページはURLに保持し、閲覧や編集の後も同じ一覧へ戻れる。
- Web UIの入力診断からAsciiDoc文書の該当範囲へ移動できるようにした。プレビュー更新に失敗しても、
  最後に成功したプレビューを失敗状態と区別して表示する。
- Web UIの編集画面へ未保存、保存中、保存成功、保存失敗の状態表示を追加した。
- 閲覧画面と編集プレビューの表、引用、リスト、目次、コード、数式の表示回帰を固定した。長い表と
  数式を個別にスクロールでき、MathJaxの読み込みまたは組版に失敗した場合は画面上に通知する。

### 変更

- `GET /api/v3/notes`は各ノートの概要に、現在の利用者の`read`、`edit`、`manage`のいずれかの
  実効アクセス水準を加えて返す。

## 0.8.0 — 2026-07-28

### 破壊的変更

- SQLite schemaを9、archiveを`marginalis-archive-7`、note profileを3へ更新した。v0.7.0が
  作成したschema 6のdatabaseとarchive 6は自動移行せず、更新時はデータを退避して空の
  `dataDir`から初期化する。
- サーバー全体の管理者権限を廃止した。`server-users`は利用可否だけを決め、所属グループによって
  個別ノートのACLを迂回する経路は設けない。
- REST APIを`/api/v3`へ更新した。変更操作の期待revisionはJSON本文ではなく、取得応答の
  `ETag`を指定する`If-Match`ヘッダーで受け取る。
- ノートの作成・更新入力を`title`、`body`、`tags`から、一つの完全なAsciiDoc文書を表す
  `source`へ変更した。題名とタグは文書ヘッダーから導出し、`:sectnums:`など許可した表示属性を
  文書ヘッダーで使用できる。

### 追加

- 一覧、閲覧、編集、共有設定を一つのReactアプリケーションへ統合した。REST型、実行時検査、
  クライアント関数はOpenAPIとMCPツール定義と同じ契約crateから生成する。
- コードブロックへ言語名、背景、余白、横スクロールを追加し、固定したMathJaxでLaTeX数式を
  閲覧画面とプレビューへ組版する。

### 変更

- domain型の不変条件とapplication境界を整理し、HTTP、SQLite、AsciiDoc、OIDCを用途別の
  portから利用する構成へ変更した。旧`marginalis-server` crateは削除した。
- ACL判定、revision確認、削除状態をSQLiteの同一transactionへ拘束し、一覧、詳細、関連ノート、
  更新、削除、復元、共有設定へ同じ認可決定表を適用した。
- Web UIの題名、本文、タグの入力欄を一つのAsciiDoc文書編集欄へ統合した。競合時も完全な文書を
  行単位で比較する。
- 識別子、所有者、時刻、revision、ACL、削除状態をAsciiDoc文書から分離し、利用者が
  サーバー管理属性を記述した場合は位置付きの入力エラーを返す。
- 要件ID、検証階層、版別受入結果を対応づけ、OpenAPI、生成物、文書、要件対応表を独立して
  検査するリリースゲートを追加した。

## 0.7.0 — 2026-07-28

### 破壊的変更

- SQLite schemaを6へ、archiveを`marginalis-archive-6`へ、note profileを2へ更新した。
  以前のdatabaseとarchiveは自動移行せず、更新時は既存データを退避して空の`dataDir`から
  初期化する。
- ノートの所有者モデルへ`issuer`と`subject`を組み合わせたACLを追加した。`read`は閲覧、
  `edit`は閲覧と内容更新を許可し、ACL管理と削除・復元は所有者だけが実行できる。

### 追加

- ReactとTypeScriptによるWeb UIを追加し、ノートの作成、編集、安全な保存前プレビュー、
  入力診断、明示的な保存をブラウザーから行えるようにした。
- revision競合時に編集開始時点、編集中、現在保存済みの内容を比較し、最新revisionを取得して
  修正後に再保存できる画面を追加した。
- ノート参照を保存時に索引化し、現在の利用者に見える直接参照元・参照先を閲覧画面へ追加した。
- 所有者、閲覧者、編集者、対象外利用者の権限境界を実Kanidm環境で確認する
  NixOSブラウザー試験を追加した。

### セキュリティ

- REST、MCP、Web UI、参照解決へ同じACL判定を適用し、権限のないノートと関連情報を
  `not_found`として扱うようにした。
- ACL更新へ同一オリジン、CSRFトークン、revision確認を適用し、不正、重複、所有者自身の
  ACL対象には位置付きの入力診断を返すようにした。
- ノートとACLを同じSQLite読み取りtransactionからアーカイブへ書き出し、整合しない時点の
  snapshotが生成されないようにした。

## 0.6.0 — 2026-07-27

### 変更

- AdocWeaveを0.11.0へ更新し、解析規則、問題の報告方法、執筆時URL、描画時URL、
  HTML出力上限のそれぞれに専用の公開設定を設けた。
- archiveとOpenAPIが記録するAdocWeave package版を0.11.0へ更新した。保存規則は変わらないため、
  SQLite schemaとnote profile版は維持し、復元互換性を明示するarchive形式はv4へ更新した。
- v0.5.0のSQLite schema 4 databaseは`dataDir`を保持したまま更新できる。AdocWeave 0.10.1の
  archiveはv0.6.0へ復元できないため、更新後に0.11.0のarchiveを新しく作成する必要がある。

### 修正

- 所有者identityの長さ、issuer URL、制御文字を一つのdomain規則で検証し、
  不正なarchiveからAsciiDocの管理属性を注入できないようにした。

## 0.5.0 — 2026-07-27

### 破壊的変更

- 利用経路のないノート単位ACLを廃止し、通常利用者は自身が作成したノートだけを操作できる所有者モデルへ単純化した。
  `server-admins`は従来どおりすべてのノートを管理できる。
- SQLite schemaを4へ、archiveを`marginalis-archive-3`へ更新した。
  旧databaseと旧archiveの移行は提供せず、更新時は旧`dataDir`全体を削除して再初期化する。

### 修正

- REST、MCP、Web UIで所有者認可を共通化し、権限のないノートを一覧から除外して個別操作を`not_found`として扱うようにした。
- archiveからACL bundleを削除し、ノートと所有者情報を直接保存する形式へ変更した。

## 0.4.0 — 2026-07-27

### 破壊的変更

- SQLite schemaを3へ、archiveを`marginalis-archive-2`へ更新した。
  旧schemaと旧archiveの自動移行は提供せず、空のdatabaseから再初期化する。
- ノート本文の解析規則をAdocWeave 0.10.1のStrict modeへ更新した。
  従来は保存できた本文でも、現行profileに適合しない場合は拒否する。

### 追加

- RESTとMCPで共通して使える入力検査の結果を追加した。問題の種類をプログラムで判別するcode、
  問題のあるfield、UTF-8 byte単位の位置を結果に含める。
- MCPに`get_note_profile`を追加し、入力上限、正規化、許可構文、許可言語、禁止規則、
  有効な本文例を、MCPクライアントが処理できる形式で公開した。
- AdocWeave 0.10.1が文書の解析とHTML生成で見つけた問題を、保存時と表示時に検査するようにした。

### 修正

- 保存時と表示時の構文modeを統一し、保存済みノートが後から描画不能になるケースを修正した。
- archiveの全階層において、定義外のfieldを拒否し形式の誤認を防ぐようにした。

## 0.3.1 — 2026-07-27

### 追加

- archiveの構造と、書き出し・復元したデータの一致を検証するコマンド、隔離した空の
  SQLite databaseへの復元検証、
  最新成功backupの検証、安全な世代整理を追加した。
- NixOS moduleに日次backup、直近30世代の保持、四半期の復元検証timer、読み取り専用の
  `marginalis diagnose`を追加した。
- Kanidm 1.10、private CA、nginxのsubpath、実ブラウザー、Dynamic Client Registration、
  Authorization Code + PKCE、token rotation、MCP初期化、認可取消を通すNixOS VM試験を追加した。
- backup、復元、purge、OIDC discovery、MCP OAuthをjournaldで継続して追跡できるよう、
  変更されないevent名を追加した。

### セキュリティ

- MCP承認フォームでは不透明Originとの互換性を維持しつつ、
  異なる具体的Originからの送信を拒否するようにした。
- protocol回帰テストの失敗出力からCookie、Bearer、認可コード、OAuth token、CSRF token、
  client secret、PKCE verifierを除去してから保存・表示するテストを追加した。

## 0.3.0 — 2026-07-25

SQLiteを単一の正本とし、Kanidmの署名済みOIDC group claim、閲覧用Web UI、REST API、
OAuth 2.1 で保護した MCP endpoint を一つの認可モデルへ統合する。

### 破壊的変更

- `v0.2.x` の database、ファイル正本、`/api/v1`、ローカル root、MCP token は移行しない。
  空の SQLite database から初期化する。
- 開発中の旧schema version 1も自動移行せず、schema version 2の空のdatabaseから再初期化する。
  Dynamic Client Registrationと利用者のMCP認可をやり直す。
- 公開 REST API を `/api/v2` とし、OpenAPI 正本を `docs/openapi.json` に置く。
- Kanidm の所属定期照会と service account を廃止し、OIDC login 時に検証した `groups` claim を
  Web session と MCP authorization の有効期間中の権限 snapshot とする。

### 変更

- ノート、ACL、ソフトデリート状態を SQLite transaction で更新し、30 日後の日次 purge を提供する。
- ノート単位の AsciiDoc export と、ACL・削除状態を含む JSON archive の import/export を提供する。
- Authorization Code + PKCE S256、refresh token rotation・family replay 失効、Dynamic Client
  Registration、認可取消を備えた MCP Streamable HTTP endpoint を提供する。
- NixOS module に OIDC credential、MCP Origin allowlist、backup 保存先、purge timer を集約する。

### セキュリティ

- PKCEの検証に成功した場合だけauthorization codeを使用済みにし、この二つの処理を一つの
  トランザクションで行う。
- 使用済みauthorization codeの再提示時に、発行済みtoken family全体を失効させる。
- 使用済み refresh token の再提示時に token family 全体を失効させる。
- token endpointで未対応のHTTP client認証が提示された場合、OAuth 2.1に従う`401` challengeを返す。
- ブラウザー mutation は session 結合 CSRF token と同一 Origin を要求し、MCP browser request は
  明示した HTTPS Origin のみ許可する。

## 0.2.0 — 2026-07-24

バージョン0.2.0から、データフォーマットv1の処理にAdocWeave v0.6.1を使用する。
`/api/v1`のOpenAPI仕様とMCPツール仕様は変更しない。

### 破壊的変更

- データフォーマットv1をAdocWeave v0.6.1の解析規則とノートプロファイルで上書きした。
  以前のv1との互換性や移行手順は提供しない。
- 更新前にサービスを停止し、必要な退避を行ったうえで既存`dataDir`を完全に削除し、
  空のディレクトリから初期化する必要がある。旧バックアップを用いた復元機能は提供しない。

### 変更

- AdocWeaveのRust版とWASM版をv0.6.1の同一コミットへ固定し、文書モデル、HTML、
  文書から生成する構造化データ、属性出現箇所を公開APIへ移行した。
- `/acceptance`に、ACLを適用したノート一覧と、作成・取得・更新・検索・削除を確認する
  JavaScript不要のフォームを追加した。
- `marginalis --version`と`marginalis -V`で、実行中のバイナリに組み込まれた
  アプリケーション版を確認できるようにした。

### 修正

- `/acceptance`のHTMLフォームではhidden fieldのセッション連動CSRFトークンを検証し、
  任意ヘッダーを設定できない通常のブラウザーフォームから送信できるようにした。
- レスポンスの`X-Request-Id`と、同じリクエストのtracingスパンへ記録するrequest IDが
  一致するよう、HTTPミドルウェアの適用順を修正した。

## 0.2.0-rc.1 — 2026-07-24

AdocWeave v0.6.1への移行を検証する、v0.2.0の最初のリリース候補である。
`/api/v1` の OpenAPI 仕様と MCP 仕様は変更しない。

### 破壊的変更

- AdocWeaveをv0.6.1へ更新し、データフォーマットv1の意味を新しい解析規則と
  ノートプロファイルで上書きした。以前のv1との互換性や移行手順は提供しない。
- 更新前にサービスを停止し、必要な退避を行ったうえで既存`dataDir`を完全に削除し、
  空のディレクトリから初期化する必要がある。アプリケーションはこの削除を自動実行しない。
- 以前のv1と新しいv1は`FORMAT`マーカーだけでは識別できない。旧`dataDir`や旧バックアップを
  v0.2.0系列で開いてはならない。

### 変更

- AdocWeaveのRust版とWASM版をv0.6.1の同一コミットへ固定し、実行時およびWASM応答の
  `packageVersion`が完全一致することを検証する。
- 文書モデル、HTML、文書から生成する構造化データ、属性出現箇所をAdocWeaveの公開APIへ移行した。
- `note-id`、`creator-id`、`created-at`、`updated-at`の置換を、属性の原文範囲に基づく
  処理として`marginalis-asciidoc`へ集約した。
- Nixパッケージも同じAdocWeaveコミットと固定ハッシュから再現可能にビルドする。

### 修正

- 一部のブラウザーやプロキシ環境から、`/acceptance`のフォームを送信できない問題を修正した。
  このサーバー生成HTMLフォームは、hidden fieldのセッション連動CSRFトークンを検証する。
  REST APIでは公開オリジンとCSRFトークンを引き続き必須とし、`Sec-Fetch-Site`がある場合は
  `same-origin`または`none`だけを許可する。

## 0.1.1 — 2026-07-24

v0.1.0 と同じ機能範囲の保守リリースである。公開 API、データフォーマット、MCP 仕様は
変更しない。

### 修正

- バイナリが報告するバージョンを正しい値に整えた（v0.1.0 タグのビルドは内部的に
  `0.1.0-rc.2` を報告していた）。

## 0.1.0 — 2026-07-23

研究室内で REST API と MCP を実運用するための最初のリリースである。`/api/v1` の OpenAPI
API 仕様とデータフォーマット v1 の互換性保証をこのバージョンから開始した。

### 追加

- OIDC ログイン、root 管理、ユーザーへの直接 ACL、監査ログ。
- AsciiDoc 正本を用いる REST ノート CRUD、FTS5 検索、`ETag` による条件付き更新、物理削除。
- OAuth Authorization Code + PKCE で保護された MCP ツール（検索・取得・参照一覧・作成・
  更新・削除）。
- OpenAPI 3.1 仕様、NixOS モジュール、バックアップ・復元、検索・参照データの再構築、root 監査の
  365 日保持。

### セキュリティ

- Cookie を伴う変更操作で、CSRF トークン・公開オリジン・Fetch Metadata を検証する。
- root 管理ルーターを通常 API から分離し、プロキシの forwarded クライアント IP ヘッダーを
  信頼しない。

### 修正

- NixOS のランタイム VM リリーステストへ `sqlite3` CLI を含め、root 資格情報の検証を実行
  できるようにした。

### 既知の制約

- Web UI、SMTP、招待、ユーザー再有効化、グループ ACL、専用管理オリジン・mTLS は含まれない。
- 実際の Kanidm と MCP クライアントを使う受入確認は、秘密情報を CI へ置かずに手動で行う。
- `/api/v1` への破壊的変更は、新しいバージョンパスと非推奨告知を伴って行う。
