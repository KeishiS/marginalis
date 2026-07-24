# 027: 検索、xref、閲覧用の変換規則

## 状態

一部完了。タグと作成・更新日時による絞り込み、タイトル優先の検索順位、xref UUIDの
正規化はRC.1で完了した。作成者・参照方向による絞り込みと閲覧用`RenderPolicy`は、
Web UIの着手時に実装する。

## 現在地

タグ、created-atおよびupdated-atはSQLite投影へ保存され、REST/MCPでタグ・日時filterを利用できる。
FTSはtitleを本文より高く評価する明示的な列重みを用いる。作成者と参照方向のfilterはquery portに
存在するが、公開REST APIとMCP toolへはまだ公開していない。

`xref:note:`のUUIDは大文字を許容するが、検索用データへ原文のまま保存するため、
小文字で保存されたノートIDとJOINできない。保存時に使う`RenderPolicy::default()`は、
閲覧時の規則を明示していない。外部リンクの属性、閲覧できない参照先の秘匿、
外部ファイルと数式の制限も配信時には保証していない。

## 作業内容

複数タグによる絞り込みはAND条件とする。指定したタグをすべて持つノートだけを返す。

1. 完了: タグと作成・更新日時を正規化して投影し、ACLを保ったタグ・日時filterをRESTおよびMCPへ追加する。
2. 完了: FTSを明示した列重みで検索し、タイトル一致を本文一致より優先する。
3. 完了: `xref:note:` UUIDをcanonical lowercase UUIDv7へ正規化して、reference投影とresolverで一貫して使う。
4. 後続: 作成者・参照方向filterを公開REST APIとMCP toolへ追加する。
5. Web UI着手時に、閲覧専用のMarginalis `RenderPolicy`を定義する。外部リンクの固定属性、外部ファイルの禁止、
   LaTeX限定、ACL拒否時の完全非表示およびanchor欠落時のノート先頭fallbackをHTML contract testで固定する。

## 完了条件

- タグ・日時filterをREST/MCPでACL非漏洩のまま利用できる。
- 大文字・小文字の違いだけでnote xrefがリンク切れにならない。
- 作成者・参照方向filterを公開する際は、REST/MCPでACL非漏洩のまま利用できる。
- 将来のHTMLエンドポイントは、保存時の入力規則と矛盾しない`RenderPolicy`を必ず使用する。
