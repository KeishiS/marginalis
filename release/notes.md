# Marginalis v0.48.0

## 主な変更

### 版履歴と復元

ノートの各版について、本文、変更者、添付画像の参照を完全な状態として追記保存するようになりました。Web UIとREST APIから版の一覧、任意の二版の行単位差分、過去版の内容を新しいrevisionとして復元する操作を利用できます。復元しても過去の履歴は書き換えません。

### 添付画像

PNG、JPEG、WebP画像をノートへアップロードし、編集画面へのドラッグ＆ドロップ、Live Preview、閲覧画面での表示を利用できます。画像は上限付きのSQLite BLOBとして保存し、取得時にはノートと同じACLを検査します。SVG、HTML、外部URLの代理取得、汎用ファイル保存は対象外です。

### 利用者identityの引き継ぎ

保存済みデータの所有者と認可を、OIDCの`issuer`と`subject`そのものではなく、内部principalへ結び付けるようになりました。管理者は、確認済みの新しいOIDC identityを既存principalのaliasとして明示的に結び付け、代表identityを切り替えられます。自動的な同一人物の推定は行いません。

### 同期とWebhook

ノート、ACL、文献情報の変更を一つの`domain_changes`へ記録し、外部検索向け同期とWebhookを同じtransactionから派生させるようにしました。

## 対応環境

x86_64とaarch64のLinuxで動作するNixOSモジュールとして配布します。利用者認証には標準のOIDC IdPが必要で、参照実装はKanidmです。Web UIはChromiumとFirefoxの最新版で確認しています。

Rust 1.97.1とNode.js 22.23.1で構築しています。実行環境へこれらを用意する必要はありません。

## 公開契約と破壊的変更

MCPの`sync_notes` toolを削除し、同じ`notes:sync` scopeで認可する`GET /api/v3/sync/notes`へ移しました。外部検索向けの投影を実装している場合は、呼び出し先をこのREST endpointへ変更してください。

AsciiDocの解析と描画をAdocWeave 0.42.0へ更新しました。Marginalisが受理する入力規則の範囲は変えていません。利用者が指定した任意のblock roleはHTML classへ出力しません。

### archive保存契約

v0.48.0が初めて書き出す現行契約は、次の組です。

- `marginalis-archive-18`
- AdocWeave package 0.42.0
- note profile 6

現行archiveは、代表identityとalias群、保持中の全ノート版、添付画像のbytesと各版の参照を含みます。v0.46.0またはv0.47.0が書き出した直前契約`marginalis-archive-17 / AdocWeave 0.41.0 / note profile 5`は、v0.48.0の`restore-archive`で現行契約へ変換して復元できます。さらに古いarchiveは、利用者ガイドの保存契約表に従い、対応する過去版で契約を一つずつ進めてください。

## v0.48.0への移行

この版はSQLite schema 23を使用します。v0.47.0のschema 22は通常起動時に自動移行されないため、`backupDirectory`を設定したNixOS環境では次の順で明示的に更新してください。

```sh
sudo systemctl stop marginalis.service
sudo nixos-rebuild switch --flake <利用中のflake>
sudo systemctl start marginalis-migrate-database.service
sudo journalctl -u marginalis-migrate-database.service -o cat -n 50
sudo systemctl start marginalis.service
```

`marginalis-migrate-database.service`は、Web session、MCP token、Webhook配送状態を含むSQLite全体の退避を`backupDirectory`へ作り、権限、SQLiteの整合性、外部キー、schema履歴を検査してから、一つのtransactionでschema 22から23へ進めます。移行後は`marginalis-diagnose.service`、HTTP health、OIDC login、MCP認可を確認してください。

同期用とWebhook用で分かれていた変更番号を統合するため、既存の同期cursorと変更索引は移行しません。Renkanなどの外部検索用投影は、移行後にcursorを省略した全量同期から再開してください。

## 更新とロールバック

切戻しでは、新しいserviceを停止し、移行後のdatabaseを別名で保全してから、移行時に作られたSQLite退避と移行前のNixOS generationを組にして戻します。旧schemaの退避をv0.48.0の実行ファイルと組み合わせても起動できません。

## 既知の制約

添付できるのは画像だけで、汎用のファイル保存には対応していません。同一人物の自動推定を行わないため、利用者のOIDC identityが変わった場合は管理者がaliasを明示的に結び付ける必要があります。

## 配布物の検証

機械可読な公開契約として、`openapi.json`と`mcp-tools.json`をこのReleaseへ添付しています。いずれもタグ上の同名ファイルとbyte単位で同じ内容で、GitHub Artifact Attestationを付与しています。次のコマンドで、assetが本リポジトリのworkflowから作られたことを確認できます。

```sh
gh attestation verify openapi.json --repo KeishiS/marginalis
```

自動検証では、Kanidmログイン、ブラウザー操作、ACL、MCP認可、Webhook、schema移行、archive復元を確認しています。配備後は、実際に使用する外部MCPクライアント、最新の実archiveを用いた隔離復元、外部Webhook受信サーバーでも受入確認することを推奨します。
