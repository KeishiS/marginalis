# 003: ノート参照構文とResolver

## 目的

AsciiDoc本文からノート間の有向辺を抽出し、ACLを考慮して安全なリンクと診断を生成する。

## 範囲

- 内部表現をAdocWeaveの`ReferenceQuery`／`ReferenceEdge`へ統一する。
- 正式表記を`xref:note:<note-uuid>#<anchor-id>[<label>]`とする。
- 既存要件の`note:<note-uuid>#<anchor-id>[<label>]`も受理し、同じ参照表現へ正規化する
  糖衣構文をAdocWeave拡張として実装する。
- UUID、任意アンカー、空ラベルおよび不完全入力を検証する。
- SQLiteから参照先ノート、アンカーおよび閲覧権限を解決する`ReferenceResolver`を実装する。
- 対象不在、アンカー不在、権限なし、形式不正および内部障害を安定した診断コードへ分ける。
- 解決済みhrefと失敗結果を、同一文書revisionの`RenderInputs`としてHTMLとprojectionへ渡す。
- 参照元の範囲、対象ノートIDおよびアンカーIDをDBへ個別に保存する。

## 範囲外

- 閲覧不能なノートのタイトルその他のメタデータを返すこと。
- 参照先ファイルまたは外部URLを解析中に読み込むこと。

## 完了条件

- 同一ノート、別ノート、自己ループ、リンク切れ、アンカー切れおよび権限なしをfixtureで検証する。
- 同じ始点・終点を持つ複数参照は、DBでは個別に保存し、グラフ表示用には一辺へ集約できる。
- Resolverが返すhrefはAdocWeaveのURL policyで再検証される。
- 参照先、アンカーまたはACL変更時に、解決済み参照と逆参照を再構築できる。

## 依存関係

- 001
- 002
