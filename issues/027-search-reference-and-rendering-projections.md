# 027: 検索・xref・閲覧用RenderPolicyの完成

状態: 一部完了。タグ・作成/更新日時filter、title優先の順位付け、xref UUIDのcanonical化はRC.1で完了した。
作成者・参照方向filterの公開と閲覧用RenderPolicyはWeb UI着手時の後続作業であり、RC.1 release blockerではない。

## 問題

タグ、created-atおよびupdated-atはSQLite投影へ保存され、REST/MCPでタグ・日時filterを利用できる。
FTSはtitleを本文より高く評価する明示的な列重みを用いる。作成者と参照方向のfilterはquery portに
存在するが、公開REST APIとMCP toolへはまだ公開していない。

`xref:note:`のUUIDは大文字を許容するが、投影へ原文のまま保存するため、小文字で保存されたnote IDと
JOINできない。保存時に使う`RenderPolicy::default()`は閲覧時の明示的policyではなく、外部リンク属性、
閲覧不能参照の非開示、resourceと数式の制限を配信時に保証していない。

## 実装項目

複数タグfilterはAND semanticsとする。すなわち、指定したすべての正規化tagを持つノートだけを返す。

1. 完了: タグと作成・更新日時を正規化して投影し、ACLを保ったタグ・日時filterをRESTおよびMCPへ追加する。
2. 完了: FTSを明示した列重みで検索し、タイトル一致を本文一致より優先する。
3. 完了: `xref:note:` UUIDをcanonical lowercase UUIDv7へ正規化して、reference投影とresolverで一貫して使う。
4. 後続: 作成者・参照方向filterを公開REST APIとMCP toolへ追加する。
5. Web UI着手時に、閲覧専用のMarginalis RenderPolicyを定義する。外部リンクの固定属性、resource禁止、
   LaTeX限定、ACL拒否時の完全非表示およびanchor欠落時のノート先頭fallbackをHTML contract testで固定する。

## 完了条件

- タグ・日時filterをREST/MCPでACL非漏洩のまま利用できる。
- 大文字・小文字の違いだけでnote xrefがリンク切れにならない。
- 作成者・参照方向filterを公開する際は、REST/MCPでACL非漏洩のまま利用できる。
- 将来のHTML endpointは保存時profileと矛盾しないRenderPolicyを必ず使用する。
