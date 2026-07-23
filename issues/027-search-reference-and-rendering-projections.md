# 027: 検索・xref・閲覧用RenderPolicyの完成

状態: タグ・検索・xrefはRC.1 release blocker。HTML配信部分はWeb UI着手前に必須。

## 問題

タグ、created-atおよびupdated-atがSQLite投影に保存されないため、タグ検索・日時絞込み・MCPのfiltersを
実装できない。FTSはタイトルと本文を同じ重みで順位付けする。

`xref:note:`のUUIDは大文字を許容するが、投影へ原文のまま保存するため、小文字で保存されたnote IDと
JOINできない。保存時に使う`RenderPolicy::default()`は閲覧時の明示的policyではなく、外部リンク属性、
閲覧不能参照の非開示、resourceと数式の制限を配信時に保証していない。

## 実装項目

1. タグと作成・更新日時を正規化して投影し、ACLを保ったタグ・作成者・日時・参照方向filterをquery port、
   RESTおよびMCPへ追加する。
2. FTSを`bm25(note_search, 10.0, 1.0)`等の明示した列重みで検索し、タイトル一致を本文一致より優先する。
3. `xref:note:` UUIDをcanonical lowercase UUIDv7へ正規化して、reference投影とresolverで一貫して使う。
4. Web UI着手時に、閲覧専用のMarginalis RenderPolicyを定義する。外部リンクの固定属性、resource禁止、
   LaTeX限定、ACL拒否時の完全非表示およびanchor欠落時のノート先頭fallbackをHTML contract testで固定する。

## 完了条件

- タグ・日時・xref filterをREST/MCPでACL非漏洩のまま利用できる。
- 大文字・小文字の違いだけでnote xrefがリンク切れにならない。
- 将来のHTML endpointは保存時profileと矛盾しないRenderPolicyを必ず使用する。
