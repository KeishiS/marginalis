# Marginalis v0.52.1

## 主な変更

### IME変換入力中の装飾位置

編集画面で日本語IMEを使って入力したとき、Live Previewの装飾が関係のない文字へ一時的に移る不具合を修正しました。

これまでは、IMEの変換中に本文だけが更新され、強調などの装飾位置は変更前の座標へ残っていました。今回の修正では、変換中は既存の装飾を本文の変更に合わせて移動します。変換が確定すると、従来どおりサーバーが返す最新の解析結果へ更新します。

## 対応環境

x86_64とaarch64のLinuxで動作するNixOSモジュールとして配布します。利用者認証には標準のOIDC IdPが必要で、参照実装はKanidmです。Web UIはChromiumとFirefoxの最新版で確認しています。

Rust 1.97.1とNode.js 24.19.0で構築しています。実行環境へこれらを用意する必要はありません。

## 公開契約と破壊的変更

REST API、MCP契約、SQLite schema、archive形式、AdocWeave package版、note profile版はv0.52.0から変更していません。この版が書き出すarchiveの保存契約は、v0.52.0と同じ次の組です。

- `marginalis-archive-18`
- AdocWeave package 0.57.0
- note profile 6

保存契約が変わらないため、`migrate-archive`が直接受理する直前契約も変更していません。

## v0.52.1への移行

この版はv0.52.0と同じSQLite schema 23を使用します。データベースとarchiveの移行コマンドは不要です。NixOS環境では通常どおり更新してください。

```sh
sudo nixos-rebuild switch --flake <利用中のflake>
```

更新後は`marginalis-diagnose.service`、HTTP health、OIDC login、既存ノートの表示を確認してください。編集画面では、装飾された箇所より前へカーソルを移動し、日本語IMEで入力しても装飾が元の文字へ追従することを確認してください。

## 更新とロールバック

更新前に通常のバックアップを完了させてください。問題がある場合はMarginalisを停止し、v0.52.0を使用していたNixOS generationへ戻します。SQLite schema、archive保存契約、公開APIは同じため、データベースやクライアントの変換は不要です。

## 既知の制約

Live Previewの装飾はサーバーが解析した結果を正本とするため、通常の入力中は応答を受け取るまで短い遅延が生じることがあります。IMEの変換中は新しい構文を解析せず、直前に確定した装飾の位置だけを本文へ追従させます。

MCPはrevision番号を指定した取得に対応しますが、履歴の一覧、二つの版の差分、過去版への復元は提供しません。添付できるのは画像だけで、汎用のファイル保存には対応していません。

公開前の受入では、単体試験とChromiumのIME入力を使うブラウザー試験により、変換中も強調装飾が元の文字へ追従することを確認しました。さらに、Firefox、aarch64、NixOS仮想マシンを含む公開前の自動検証を実施しています。実配備先でのOIDC login、外部MCPクライアント、Webhook受信は公開後に確認してください。

## 配布物の検証

機械可読な公開契約として、`openapi.json`と`mcp-tools.json`をこのReleaseへ添付しています。いずれもタグ上の同名ファイルとbyte単位で同じ内容で、GitHub Artifact Attestationを付与しています。次のコマンドで、assetが本リポジトリのworkflowから作られたことを確認できます。

```sh
gh attestation verify openapi.json --repo KeishiS/marginalis
```

自動検証では、ノート検証・描画、Kanidm login、ブラウザー操作、ACL、MCP認可、Webhook、schema検査、archiveの移行と復元を確認しています。配備後は、実際に使用する外部MCPクライアント、最新の実archiveを用いた隔離復元、外部Webhook受信サーバーでも受入確認することを推奨します。
