# Marginalis v0.52.0

## 主な変更

### MCPからの過去revision取得

MCPの`get_note`、`get_note_outline`、`get_note_fragment`で、保持中の過去revisionを指定できるようにしました。正の`revision`を省略した場合は、従来どおり現在版を返します。

長いノートでは、`get_note_outline`と`get_note_fragment`へ同じ`revision`を渡すことで、過去版の見出し構造を確認してから必要な行だけを取得できます。過去版の閲覧にも現在のACLを適用し、存在しないrevisionと閲覧できないノートはどちらも`not_found`を返します。

`get_note_fragment`の`expected_revision`は、現在版を取得する間に更新が起きていないことを確認する入力です。過去版を選ぶ`revision`とは役割が異なるため、両方を同時に指定した要求は引数エラーとして拒否します。

## 対応環境

x86_64とaarch64のLinuxで動作するNixOSモジュールとして配布します。利用者認証には標準のOIDC IdPが必要で、参照実装はKanidmです。Web UIはChromiumとFirefoxの最新版で確認しています。

Rust 1.97.1とNode.js 24.19.0で構築しています。実行環境へこれらを用意する必要はありません。

## 公開契約と破壊的変更

MCP契約の`get_note`、`get_note_outline`、`get_note_fragment`へ、省略可能で1以上の`revision`を追加しました。既存の要求は変更せずに受理します。`get_note_fragment`で`revision`と`expected_revision`を同時に指定する要求は受理しません。

REST API、SQLite schema、archive形式、AdocWeave package版、note profile版はv0.51.0から変更していません。この版が書き出すarchiveの保存契約は、v0.51.0と同じ次の組です。

- `marginalis-archive-18`
- AdocWeave package 0.57.0
- note profile 6

保存契約が変わらないため、`migrate-archive`が直接受理する直前契約も変更していません。

## v0.52.0への移行

この版はv0.51.0と同じSQLite schema 23を使用します。データベースとarchiveの移行コマンドは不要です。NixOS環境では通常どおり更新してください。

```sh
sudo nixos-rebuild switch --flake <利用中のflake>
```

更新後は`marginalis-diagnose.service`、HTTP health、OIDC login、既存ノートの表示を確認してください。MCPクライアントでは、revisionを省略した従来の取得と、保存済みrevisionを指定した全文・見出し・断片の取得を確認してください。

## 更新とロールバック

更新前に通常のバックアップを完了させてください。問題がある場合はMarginalisを停止し、v0.51.0を使用していたNixOS generationへ戻します。SQLite schemaとarchive保存契約は同じため、データベースの変換は不要です。

v0.52.0で追加した`revision`をMCP要求へ指定しているクライアントは、v0.51.0へ戻す前にその入力を取り除いてください。現在版だけを取得する既存のMCP要求は、そのまま利用できます。

## 既知の制約

MCPはrevision番号を指定した取得に対応しますが、履歴の一覧、二つの版の差分、過去版への復元は提供しません。取得するrevision番号は、現在版の応答や利用者が保持している更新結果から指定してください。

削除済みノートの履歴は、ノート自体を現在のACLで閲覧できないためMCPから取得できません。存在しないrevisionと閲覧できないノートを応答から区別することもできません。

添付できるのは画像だけで、汎用のファイル保存には対応していません。同一人物の自動推定を行わないため、利用者のOIDC identityが変わった場合は管理者がaliasを明示的に結び付ける必要があります。

公開前の受入では、生成したMCP契約、MCP transportの結合試験、v0.52.0のNix packageを確認しました。結合試験では、同じ過去revisionの全文・見出し・断片、現在版の既存動作、ACLによる不可視化、存在しないrevision、排他的な入力の拒否を確認しています。実配備先でのOIDC login、外部MCPクライアント、Webhook受信は公開後に確認してください。

## 配布物の検証

機械可読な公開契約として、`openapi.json`と`mcp-tools.json`をこのReleaseへ添付しています。いずれもタグ上の同名ファイルとbyte単位で同じ内容で、GitHub Artifact Attestationを付与しています。次のコマンドで、assetが本リポジトリのworkflowから作られたことを確認できます。

```sh
gh attestation verify mcp-tools.json --repo KeishiS/marginalis
```

自動検証では、ノート検証・描画、Kanidm login、ブラウザー操作、ACL、MCP認可、Webhook、schema検査、archiveの移行と復元を確認しています。配備後は、実際に使用する外部MCPクライアント、最新の実archiveを用いた隔離復元、外部Webhook受信サーバーでも受入確認することを推奨します。
