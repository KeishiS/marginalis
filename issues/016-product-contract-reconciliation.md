# 016: 製品契約と要件定義の整合

状態: 完了。

## 目的

`docs/requirements.md`、Issue、READMEおよび実装の間で、初期公開の必須機能と将来構想を一致させる。
現在は招待、再有効化、OpenAPI、検索filter、CSRFの追加検証等が「確定要件」に残る一方、実装・issueでは
後続として扱われている。この状態では、完成条件とrelease判断を一意にできない。

## 実施範囲

- 各要件へ`current release`、`next release`、`future`、`rejected`の状態を付ける。
- current releaseのHTTP API、MCP、NixOS module、運用手順およびsecurity最低条件を一つの契約にする。
- 各issueの状態を、実装済み・実環境確認待ち・設計待ち・後続に正規化する。
- 仕様から削除した機能を失わないよう、future roadmapへ移す。

## 完了条件

- `docs/requirements.md`に、実装済み範囲を誤って将来機能として記述した箇所、および未実装機能を
  current releaseの必須として記述した箇所がない。
- README、REST/MCP仕様、NixOS運用、issueの優先順位が同じrelease境界を示す。
- API、data formatおよび運用の破壊的変更を許容する期間と、freezeする契約を明記する。

## 決定事項

- 現行版を「研究室内でREST/MCPを実運用できる最小版」と定義した。
- 優先順位を、REST/MCP実運用、SMTP・OIDCユーザー再有効化・Web UI、招待・グループACLの順とした。
- 当時の`/api/v1`は個人開発の再構成期間に破壊的変更を許容し、OpenAPI導入後に外部クライアント向け
  互換性を固定する方針とした。

## 実施結果

- `docs/requirements.md`へ現行版、次期版および将来版の境界を明記した。
- 現行版をREST/MCP実運用、次期版をSMTP・再有効化・Web UI、将来版を招待・グループACLとして固定した。
- `/api/v1`はOpenAPI完成まで破壊的変更を許容し、data formatはv1で固定した。
