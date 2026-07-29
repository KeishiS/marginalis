# 要件と検証の対応表

この文書は、[現行要件](requirements.md)をどの検証階層で確認するかを示します。ここでいう検証階層は、
単体試験、公開契約試験、複数コンポーネントを接続する結合試験、NixOS仮想マシン試験、運用時の
人手受入です。個別の試験名は変更できるため、安定した要件IDと責務別の検証先を対応させます。

| 要件ID | 主な自動検証 | 運用受入 |
| --- | --- | --- |
| REQ-OPS-001 | `nix flake check`、NixOS VM | 配備 |
| REQ-OPS-002 | `marginalis-sqlite`単体試験 | バックアップ・復元 |
| REQ-OPS-003 | SQLite契約試験 | 実運用監視 |
| REQ-OPS-004 | OIDC結合試験、Kanidm VM | ログイン |
| REQ-OPS-005 | 旧schema拒否、archive移行の入力不変・出力非上書き・安全な失敗位置試験 | 更新前退避 |
| REQ-DATA-001 | domain・SQLite・属性環境単体試験 | アーカイブ照合 |
| REQ-DATA-002 | archive・CLI試験 | 隔離復元 |
| REQ-DATA-003 | AsciiDoc単体試験 | なし |
| REQ-DATA-004 | schema検査 | なし |
| REQ-DATA-005 | SQLite認可・参照試験、ブラウザー試験 | 非開示確認 |
| REQ-AUTH-001 | OIDC単体・結合試験 | ログイン拒否 |
| REQ-AUTH-002 | domain・archive・SQLite単体試験 | 所有者照合 |
| REQ-AUTH-003 | SQLite認可試験、ブラウザー試験 | 利用者別操作 |
| REQ-AUTH-004 | SQLite認可決定表 | 利用者別操作 |
| REQ-AUTH-005 | 本番到達性検査、認可結合試験 | 対象外グループ確認 |
| REQ-AUTH-006 | OIDC・session単体試験 | 再ログイン反映 |
| REQ-AUTH-007 | session期限・並行延長・保守処理単体試験、NixOS VM | 期限確認 |
| REQ-API-001 | REST・MCP結合試験 | transport間照合 |
| REQ-API-002 | OpenAPI生成差分、router契約試験 | 接続確認 |
| REQ-API-003 | REST単体・結合試験 | 競合操作 |
| REQ-API-004 | Auth0 adapter単体試験、Protected Resource Metadata・MCP HTTP試験、NixOS module評価 | MCP接続 |
| REQ-API-005 | MCP scope結合試験 | MCP権限照合 |
| REQ-API-006 | Authorization Server Metadata確認、認可取消APIの本番到達性検査 | client接続解除、grant取消、JWT失効時間 |
| REQ-API-007 | MCP JSON Schema生成差分、MCP HTTP契約試験 | client同期確認 |
| REQ-UI-001 | React単体試験、ブラウザー試験 | 主要画面 |
| REQ-UI-002 | TypeScript実行時検査試験 | 異常応答表示 |
| REQ-UI-003 | React状態・画面単体試験、ブラウザー試験 | 一覧の復帰 |
| REQ-UI-004 | AdocWeave診断、REST契約、TypeScript応答検査、React編集・UTF-8位置単体試験、ブラウザー試験 | 警告表示、診断からの修正 |
| REQ-UI-005 | AdocWeave公開HTML・React描画fixture・MathJax失敗試験、ブラウザー試験 | 閲覧・プレビュー |
| REQ-ACL-001 | application・SQLite・React試験 | 共有操作 |
| REQ-ACL-002 | 公開契約検査 | なし |
| REQ-ACL-003 | SQLite認可試験、ブラウザー試験 | 利用者別操作 |
| REQ-ACL-004 | REST・SQLite契約試験 | 一覧非開示 |
| REQ-ACL-005 | SQLiteスナップショット試験、REST契約試験 | 閲覧整合性 |
| REQ-LIFE-001 | SQLite・保守CLI・NixOS VM | 削除・復元 |
| REQ-DEPLOY-001 | NixOS module評価・VM | 本番設定確認 |
| REQ-FORMAT-001 | schema・archive拒否、archive 7から8へのNix移行試験 | 更新前退避 |

## 実行単位

- **高速検証**: `nix develop --command cargo make verify`
- **本番到達性**: `nix develop --command cargo make production-reachability`
- **カバレッジ**: `nix develop --command cargo make coverage`
- **NixOS受入**: `nix build .#checks.x86_64-linux.nixos-module-vm`および関連VM
- **版別の人手受入**: [受入基準](acceptance.md)からリンクする版別結果

要件を追加、削除、改名する場合は、この表を同じ変更で更新します。文書検査は、要件IDの重複、
対応表への記載漏れ、対応表だけに残ったIDを拒否します。
