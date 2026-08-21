# Marginalis v0.49.0

## 主な変更

### AdocWeave 0.43.0

AsciiDocの解析と描画をAdocWeave 0.43.0へ更新しました。題を持つlisting block(`----`)とliteral block(`....`)は`figure`と`figcaption`で描画するようになり、題が閲覧画面から失われなくなりました。Marginalisが受理する入力規則の範囲は変えていません。

編集画面では、`[source,rust]`のような属性行を、直前のblock attribute行として区切り記号と同じ装飾の対象に含めるようになりました。

### 公開手順の自動化

リリースの公開方式を、検証済みの`main`のcommitをそのままタグとGitHub Releaseへ昇格する形へ変更しました。`main`のCIが、公開するasset、Release Notesの本文、source commit、各fileのSHA-256を固定した候補を作り、公開workflowがそれを作り直さずに公開します。この版から、タグとGitHub Releaseは人ではなくworkflowが作成します。

利用者から見た配布物は変わりません。公開する`openapi.json`と`mcp-tools.json`は、タグ上の同名ファイルとbyte単位で同じ内容であることをworkflowが検査し、GitHub Artifact Attestationを付与します。

## 対応環境

x86_64とaarch64のLinuxで動作するNixOSモジュールとして配布します。利用者認証には標準のOIDC IdPが必要で、参照実装はKanidmです。Web UIはChromiumとFirefoxの最新版で確認しています。

Rust 1.97.1とNode.js 22.23.1で構築しています。実行環境へこれらを用意する必要はありません。

## 公開契約と破壊的変更

REST APIとMCPの経路、要求と応答の形は変更していません。`openapi.json`が記録するAdocWeave package版が`0.43.0`になります。

### archive保存契約

v0.49.0が初めて書き出す現行契約は、次の組です。

- `marginalis-archive-18`
- AdocWeave package 0.43.0
- note profile 6

**移行元として受理するarchiveが変わります。** `migrate-archive`と`restore-archive`が受理する旧契約は、v0.48.0が書き出した`marginalis-archive-18` / 0.42.0 / 6だけになりました。v0.47.0以前が書き出したarchiveは、この版だけでは復元できません。利用者ガイドの保存契約の履歴に従い、対応する公開版で契約を一つずつ持ち上げてから渡してください。

新旧の契約は記録する項目が同じで、AdocWeave package版だけが異なります。移行では全ノートを0.43.0で再検証し、代表identityとalias群、保持中の全版、添付画像を引き継ぎます。

## v0.49.0への移行

この版はSQLite schema 23を使用します。v0.48.0と同じschemaのため、データベースの移行は不要です。NixOS環境では通常どおり更新してください。

```sh
sudo nixos-rebuild switch --flake <利用中のflake>
```

更新後は`marginalis-diagnose.service`、HTTP health、OIDC login、MCP認可を確認してください。

保管しているarchiveがv0.47.0以前のものである場合は、この版へ更新する前に、v0.48.0で`marginalis-archive-18` / 0.42.0 / 6へ変換して保管し直すことを推奨します。

## 更新とロールバック

切戻しでは、新しいserviceを停止し、移行前のNixOS generationへ戻します。schemaを変更していないため、データベースの入れ替えは不要です。

v0.49.0が書き出したarchiveは、v0.48.0では現行契約として受理されません。切戻し後に復元が必要な場合は、切戻し前に取得したv0.48.0のarchiveを使ってください。

## 既知の制約

添付できるのは画像だけで、汎用のファイル保存には対応していません。同一人物の自動推定を行わないため、利用者のOIDC identityが変わった場合は管理者がaliasを明示的に結び付ける必要があります。

公開前の人手受入は、配備した環境での画面操作と外部MCPクライアントの確認を対象としています。この版では自動検証だけを実施しているため、配備後に実環境での確認を行ってください。

## 配布物の検証

機械可読な公開契約として、`openapi.json`と`mcp-tools.json`をこのReleaseへ添付しています。いずれもタグ上の同名ファイルとbyte単位で同じ内容で、GitHub Artifact Attestationを付与しています。次のコマンドで、assetが本リポジトリのworkflowから作られたことを確認できます。

```sh
gh attestation verify openapi.json --repo KeishiS/marginalis
```

自動検証では、Kanidmログイン、ブラウザー操作、ACL、MCP認可、Webhook、schema検査、archiveの移行と復元を確認しています。配備後は、実際に使用する外部MCPクライアント、最新の実archiveを用いた隔離復元、外部Webhook受信サーバーでも受入確認することを推奨します。
