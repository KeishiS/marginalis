# 016: プロダクト契約と要件定義の再整合

状態: 提案。実装着手前に要件を確定する。

## 目的

`docs/requirements.md`、issue、READMEおよび実装の間で、初期公開の必須機能と将来構想を一致させる。
現在は招待、再有効化、OpenAPI、検索filter、CSRFの追加検証等が「確定要件」に残る一方、実装・issueでは
後続として扱われている。この状態では、完成条件とrelease判断を一意にできない。

## 範囲

- 各要件へ`current release`、`next release`、`future`、`rejected`の状態を付ける。
- current releaseのHTTP API、MCP、NixOS module、運用手順およびsecurity最低条件を一つの契約にする。
- 各issueの状態を、実装済み・実環境確認待ち・設計待ち・後続に正規化する。
- 仕様から削除した機能を失わないよう、future roadmapへ移す。

## 完了条件

- `docs/requirements.md`に、実装済み範囲を誤って将来機能として記述した箇所、および未実装機能を
  current releaseの必須として記述した箇所がない。
- README、REST/MCP仕様、NixOS運用、issueの優先順位が同じrelease境界を示す。
- API、data formatおよび運用の破壊的変更を許容する期間と、freezeする契約を明記する。

## 要判断事項

- current releaseを「研究室内でREST/MCPを実運用できる最小版」と定義する。
- 次の優先順位を採用する: REST/MCP実運用、SMTP・OIDCユーザー再有効化・Web UI、招待・グループACL。
- REST APIを外部client向けの安定契約としていつfreezeするか。
