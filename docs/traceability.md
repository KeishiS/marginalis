# 要件と検証の対応表

この文書は、[現行要件](requirements.md)をどの検証階層で確認するかを示します。ここでいう検証階層は、
単体試験、公開契約試験、複数コンポーネントを接続する結合試験、NixOS仮想マシン試験、運用時の
人手受入です。個別の試験名は変更できるため、安定した要件IDと責務別の検証先を対応させます。

| 要件ID | 主な自動検証 | 運用受入 |
| --- | --- | --- |
| REQ-OPS-001 | `nix flake check`、NixOS VM | 配備 |
| REQ-OPS-002 | `marginalis-sqlite`単体試験 | バックアップ・復元 |
| REQ-OPS-003 | SQLite契約試験、約1,000ノートでの図の問い合わせと点の組み立ての試験 | 実運用監視 |
| REQ-OPS-004 | OIDC結合試験、Kanidm VM | ログイン |
| REQ-OPS-005 | 旧schema拒否、archive移行の入力不変・出力非上書き・安全な失敗位置試験 | 更新前退避 |
| REQ-OPS-006 | SQLiteファイル診断・CLI秘密情報非記録試験、NixOS VM反復診断・ファイル不変性試験 | 障害診断 |
| REQ-OPS-007 | `cargo make observability-check`、`observability_logs_safe_http_and_mcp_results`、`checks.x86_64-linux.kanidm-discovery-vm` | event・request IDによる障害診断 |
| REQ-DATA-001 | domain・SQLite・属性環境単体試験 | アーカイブ照合 |
| REQ-DATA-002 | archive・CLI試験 | 隔離復元 |
| REQ-DATA-003 | AsciiDoc単体試験 | なし |
| REQ-DATA-004 | schema検査 | なし |
| REQ-DATA-005 | SQLite認可・参照試験、ブラウザー試験 | 非開示確認 |
| REQ-DATA-006 | application入力検査、SQLite所有者境界・競合・revision試験、REST・MCP公開契約試験 | Web UIでの検索、追加、編集、削除とMCPでの検索、追加、削除 |
| REQ-DATA-007 | 引用解決の所有者境界と重複排除の試験、生成した参考文献の相互link試験、書誌情報を記法として解釈しない試験 | 共有したノートでの引用表示 |
| REQ-DATA-008 | 図の認可境界・絞り込み・片端の欠けた線の試験、起点と階層数の試験、想定規模の試験、REST経路と認証と範囲外指定の試験、ブラウザー試験 | 関係の図の表示 |
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
| REQ-API-004 | OAuth application・SQLite単体試験、CIMD取得境界・`iss`・metadata・MCP HTTP試験、NixOS module評価 | MCP接続 |
| REQ-API-005 | ノート・書誌scope対応、scope上限の共通部分・revision・選択的失効の単体試験、同意scopeの部分選択・フォーム改変拒否のHTTP試験、REST・React設定画面試験、MCP scope結合試験、metadata契約試験 | MCP権限照合、同意scopeの選択、上限変更後の再認可 |
| REQ-API-006 | RFC 7009・token family失効試験、認可取消APIのHTTP契約試験と本番到達性検査 | client接続解除、即時失効 |
| REQ-API-007 | MCP JSON Schema生成差分、MCP HTTP契約試験、AsciiDoc入力規則・HTML描画試験、警告拒否前の永続化防止試験 | client同期・参考文献作成・診断修正後の再実行確認 |
| REQ-UI-001 | React単体試験、ブラウザー試験、設定画面の経路・REST契約試験、経路別chunkと配布物の大きさ検査 | 主要画面 |
| REQ-UI-002 | TypeScript実行時検査試験 | 異常応答表示 |
| REQ-UI-003 | React状態・画面単体試験、ブラウザー試験 | 一覧の復帰 |
| REQ-UI-004 | AdocWeave診断、REST契約、TypeScript応答検査、React編集・UTF-8位置単体試験、ブラウザー試験 | 警告表示、診断からの修正 |
| REQ-UI-005 | AdocWeave公開HTML・React描画fixture・MathJax失敗試験、ブラウザー試験 | 閲覧・プレビュー |
| REQ-UI-006 | CodeMirror操作・CSP nonce単体試験、5,000行・320px幅・本番CSP・固定画像ブラウザー試験、配布容量検査 | 長文入力、IME、表示切替、キーボード保存 |
| REQ-UI-007 | application所有者選択試験、SQLite分離・revision試験、REST契約試験、React設定画面・MathJax設定試験 | `\argmax`・`\bm`の閲覧と共有表示 |
| REQ-ACL-001 | application・SQLite・React試験 | 共有操作 |
| REQ-ACL-002 | 公開契約検査 | なし |
| REQ-ACL-003 | SQLite認可試験、ブラウザー試験 | 利用者別操作 |
| REQ-ACL-004 | REST・SQLite契約試験 | 一覧非開示 |
| REQ-ACL-005 | SQLiteスナップショット試験、REST契約試験 | 閲覧整合性 |
| REQ-LIFE-001 | SQLite所有者境界・保持期限・ACL維持試験、REST契約試験、React単体試験、ブラウザー試験、保守CLI・NixOS VM | 削除・復元 |
| REQ-API-008 | REST・MCP失敗一致試験、MCP失敗出力schema生成差分 | 異常時のclient表示 |
| REQ-DEPLOY-001 | NixOS module評価・VM | 本番設定確認 |
| REQ-DEPLOY-002 | 環境変数宣言の単体試験、起動と診断の判断一致CLI試験、NixOS module評価 | 診断出力確認 |
| REQ-FORMAT-001 | schema・archive拒否、archive 7から13までの対応契約から14への移行試験 | 更新前退避 |
| REQ-FORMAT-002 | 文書書き出しの構成・ファイル名・削除済み除外・manifest版情報の試験、CLIの権限と上書き拒否の試験 | 取り出した内容の他ツールでの読み取り |
| REQ-FORMAT-003 | 文書の書き出しと取り込みの往復一致試験、版差での再検証試験、書庫外pathの拒否試験 | 別環境への移行 |

## 実行単位

- **高速検証**: `nix develop --command cargo make verify`
- **本番到達性**: `nix develop --command cargo make production-reachability`
- **カバレッジ**: `nix develop --command cargo make coverage`
- **NixOS受入**: `nix build .#checks.x86_64-linux.nixos-module-vm`および関連VM
- **版別の人手受入**: [受入基準](acceptance.md)からリンクする版別結果

要件を追加、削除、改名する場合は、この表を同じ変更で更新します。文書検査は、要件IDの重複、
対応表への記載漏れ、対応表だけに残ったIDを拒否します。
