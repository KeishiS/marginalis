# ログと障害診断

この文書は運用者と開発者に向けて、Marginalisが出力する構造化ログの契約、秘密情報の境界、
主要event、障害時の調べ方を定めます。systemd unitの操作は
[NixOSでの運用](nixos.md)、CIの失敗は[GitHubを使う開発手順](development.md)を参照してください。
productionコードが出力するログの正本はこの文書です。試験用コード、依存ライブラリ、
Webブラウザーのconsole、GitHub Actionsの標準出力は対象外です。

## ログの契約

通常のログは標準エラーへ出力し、NixOSではjournalへ保存します。`RUST_LOG`は出力対象を選ぶ
filterであり、fieldや秘密情報の出力を有効にする設定ではありません。端末の色を表す制御文字は
出力せず、端末とCIとjournalで同じfield表記を使用します。

Marginalis自身が出力するすべてのproductionログは、次の規則に従います。依存ライブラリのログを
`RUST_LOG`で明示的に有効化した場合、そのライブラリ固有のfieldはこの契約の対象外です。

| 項目 | 規則 |
| --- | --- |
| `event` | 処理を識別する安定した名前。検索や通知には本文ではなくこの値を使用 |
| 結果 | `completed`、`failed`、`unavailable`、`rejected`などのevent末尾、またはHTTPの`outcome` |
| `reason` | 拒否または障害を分類できる場合の固定値。外部から受け取った本文を入れない |
| `error` | 運用者が原因を調べるための詳細。認証情報を保持する型を渡さない |
| 件数・時刻・path | eventの判断に必要な非秘密情報だけを記録 |

ログ本文は人が読む補足であり、監視契約ではありません。文言を使って分類せず、`event`と固定fieldを
使用します。HTTP requestのspanには`request_id`、`method`、実際の識別子を含まないroute
templateを`path`として記録します。未一致の経路は`<unmatched>`とし、queryや入力された経路を
記録しません。handlerが応答statusを決定した時点の`http.request.completed`には、`outcome`、
`status`、`latency_ms`、該当する場合は固定値の`problem_code`を記録します。streaming responseでは
本文の送信完了時刻ではありません。

## 記録しない情報

次の情報は、通常時と失敗時のどちらもログへ記録しません。

- Cookie、Bearer token、OIDCの認可code、state、nonce、PKCE verifier、client secret
- ID tokenとaccess tokenのclaim値、利用者の`issuer`と`subject`
- ノート本文、ノートID、題名、タグ、検索語
- HTTP requestとresponseのheaderおよびbody

認証・認可の失敗は値そのものではなく、`token-format`、`standard-claims`、
`insufficient-scope`などの`reason`で区別します。ログをIssueまたはPull Requestへ添付する場合も、
実環境の値が含まれないことを確認します。

## 主要event

<!-- observability-event-catalog:start -->

### HTTPとservice

| event | level | 主なfield | 意味 |
| --- | --- | --- | --- |
| `http.request.completed` | INFOまたはERROR | `request_id`、`method`、`path`、`outcome`、`status`、`latency_ms`、任意の`problem_code` | HTTP handlerの完了。5xxだけERROR |
| `http.request.rejected` | WARN | `request_id`、`reason` | Cookieを使う変更requestのorigin拒否 |
| `mcp.request.rejected` | WARN | `request_id`、`reason` | browser MCP requestのorigin拒否 |
| `service.listening` | INFO | `address` | listenerの起動完了 |
| `service.failed` | ERROR | `command`、`error` | serviceの設定、依存先、listener、実行中の障害 |
| `service.signal_handler.failed` | ERROR | `reason`、`error` | 終了signal handlerを準備できない状態 |
| `service.shutdown.started` | INFO | なし | 新規requestを止め、処理中requestの完了待ちを開始 |
| `service.shutdown.completed` | INFO | なし | 処理中requestの完了待ちとlistener停止の完了 |
| `command.failed` | ERROR | `command=unknown`、`error` | 未知のcommandを値を記録せずに拒否 |

### OIDCとMCP

| event | 結果・原因 |
| --- | --- |
| `oidc.discovery.completed`、`oidc.discovery.failed` | Kanidm discoveryの成功または到達不能 |
| `mcp.authorization.discovery.completed`、`mcp.authorization.discovery.failed` | Auth0 metadataと初期JWKSの取得結果 |
| `mcp.authorization.jwks_refresh.completed`、`mcp.authorization.jwks_refresh.failed` | 未知の`kid`を受けた後のJWKS更新結果 |
| `mcp.authentication.failed` | access tokenの拒否。`reason`は安全な検証段階 |
| `mcp.authentication.unavailable` | token検証基盤の障害 |
| `mcp.authorization.failed` | 検証済みtokenのscope不足 |
| `mcp.protocol.selected` | 固定した`protocol_era`、対応済み`protocol_version`、`method`によるmodern/legacy経路の選択 |
| `mcp.request.completed` | 固定した`method`、`outcome`、任意の`reason`によるJSON-RPC requestの論理結果 |
| `mcp.tool.completed` | 固定した`tool`、`outcome`、任意の`reason`によるtool use caseの論理結果 |

JSON-RPC ID、未知のmethod名、未知のtool名、tool引数と応答内容は記録しません。JSON-RPC errorや
tool結果の`isError`はHTTP 200になる場合があるため、HTTPの`outcome`ではなく
`mcp.request.completed`と`mcp.tool.completed`で論理的な成功、拒否、障害を判断します。
`mcp.protocol.selected`はclient名やheader値を記録せず、対応しているprotocol版だけを記録します。

### 保守処理

| 処理 | 成功event | 失敗event |
| --- | --- | --- |
| 期限切れデータの削除 | `maintenance.purge.completed` | `maintenance.purge.failed` |
| backup作成 | `maintenance.backup.completed` | `maintenance.backup.failed` |
| backup復元検証 | `maintenance.backup_verification.completed` | `maintenance.backup_verification.failed` |
| backup世代整理 | `maintenance.backup_prune.completed` | `maintenance.backup_prune.failed` |
| archive書き出し | `maintenance.archive_export.completed` | `maintenance.archive_export.failed` |
| 文書書き出し | `maintenance.document_export.completed` | `maintenance.document_export.failed` |
| 文書取り込み | `maintenance.document_import.completed` | `maintenance.document_import.failed` |
| archive移行 | `maintenance.archive_migration.completed` | `maintenance.archive_migration.failed` |
| archive取り込み | `maintenance.archive_import.completed` | `maintenance.archive_import.failed` |
| archive検証 | `maintenance.archive_validation.completed` | `maintenance.archive_validation.failed` |
| 隔離復元検証 | `maintenance.restore_verification.completed` | `maintenance.restore_verification.failed` |

`maintenance.restore_cleanup.failed`は、隔離復元の結果にかかわらず一時directoryを削除できなかった
ことを示します。SQLite診断は標準出力のJSONに`event: diagnostics.completed`と各検査結果を出し、
異常時はjournalにも`maintenance.diagnostics.failed`を記録します。

<!-- observability-event-catalog:end -->

## 旧eventからの移行

この変更はログの監視契約を破壊的に更新します。旧eventや旧fieldは併記しません。保存済みの
検索条件、通知規則、dashboardは次の対応で更新します。

| 以前の出力 | 現在の出力 | 移行内容 |
| --- | --- | --- |
| `http_request` spanの既定完了event | `http.request.completed` | `latency`を`latency_ms`へ変更し、`outcome`、`status`、任意の`problem_code`を追加 |
| requestの実pathを含む`path` | route templateまたは`<unmatched>`を含む`path` | ノートID、未知のpath、queryを監視条件から削除 |
| `archive.export.completed` | `maintenance.archive_export.completed` | event名の置換 |
| `archive.import.completed` | `maintenance.archive_import.completed` | event名の置換 |
| `command.failed` | commandごとの`maintenance.*.failed`または`service.failed` | `command.failed`は未知のcommandだけに限定 |
| `error_kind` | `reason` | OIDC discovery失敗のfield名を統一 |
| eventのない終了signalログ | `service.signal_handler.failed`、`service.shutdown.started`、`service.shutdown.completed` | signal準備失敗と正常終了の段階を分離 |
| HTTP statusだけで判断したMCP結果 | `mcp.request.completed`、`mcp.tool.completed` | JSON-RPCとtoolの論理結果をHTTP結果から分離 |

## 障害時の確認

まず同じsystemd invocationと`event`へ絞り、HTTPの場合は`request_id`を使用します。

```bash
journalctl -u marginalis.service --since today
journalctl -u marginalis.service \
  _SYSTEMD_INVOCATION_ID="$(systemctl show marginalis.service -p InvocationID --value)"
journalctl -u marginalis.service -g 'http.request.completed'
journalctl -u marginalis-backup.service -g 'maintenance.backup.'
```

5xxでは同じ`request_id`の認証・認可eventまたはservice eventを確認します。保守unitでは、失敗eventの
`error`、保存先容量、権限、mount状態を確認し、原因を解消してから同じunitを再実行します。

## 自動検査

`cargo make observability-check`はproductionの各`tracing`呼出しに固定文字列の`event`があること、
macroが`tracing::`で修飾されていること、spanを含めてtoken、Cookie、利用者identity、ノート内容、
HTTP header由来の値を表すfield名と未正規化URIがないこと、実装上のevent一覧とこの文書の
event一覧が一致することを検査します。検査自身の変異fixtureは、禁止例を一つずつ混入した場合に
失敗することを確認します。HTTP試験とNixOS VM試験は、requestの成功、拒否、障害、discovery、
保守処理の主要eventを実際の出力でも確認します。
