# Marginalis v0.50.1

## 主な変更

### NixOS構成の評価時に表示される警告の解消

MarginalisのNixOSモジュールを取り込んだ構成を評価したときに表示されていた、`stdenv.isLinux`と`stdenv.isDarwin`の非推奨警告を解消しました。

`rust-overlay`を非推奨属性の参照が修正されたcommitへ更新し、Marginalis自身のLinux判定も`stdenv.hostPlatform.isLinux`を使うようにしました。Rust toolchainの版は変えていません。

## 対応環境

x86_64とaarch64のLinuxで動作するNixOSモジュールとして配布します。利用者認証には標準のOIDC IdPが必要で、参照実装はKanidmです。Web UIはChromiumとFirefoxの最新版で確認しています。

Rust 1.97.1とNode.js 24.19.0で構築しています。実行環境へこれらを用意する必要はありません。

## 公開契約と破壊的変更

REST API、MCP、SQLite schema、archive保存契約はv0.50.0から変更していません。

この版が書き出すarchiveは、v0.50.0が初めて採用した次の契約です。

- `marginalis-archive-18`
- AdocWeave package 0.47.0
- note profile 6

`migrate-archive`と`restore-archive`が受理する旧契約も、v0.49.0が書き出した`marginalis-archive-18` / 0.43.0 / 6のままです。

## v0.50.1への移行

この版はSQLite schema 23を使用します。v0.50.0と同じschemaのため、データベースの移行は不要です。

NixOS環境では通常どおり更新してください。

```sh
sudo nixos-rebuild switch --flake <利用中のflake>
```

更新後は`marginalis-diagnose.service`、HTTP health、OIDC login、MCP認可を確認してください。v0.49.0以前から更新する場合は、途中の版に固有の移行条件も各Release Notesで確認してください。

## 更新とロールバック

切戻しでは、新しいserviceを停止し、移行前のNixOS generationへ戻します。schemaと保存契約を変更していないため、v0.50.0へ戻す場合はデータベースやarchiveの入れ替えは不要です。

## 既知の制約

添付できるのは画像だけで、汎用のファイル保存には対応していません。同一人物の自動推定を行わないため、利用者のOIDC identityが変わった場合は管理者がaliasを明示的に結び付ける必要があります。

AdocWeaveがNix packageの公開先をLinuxに限定しているため、macOSの開発環境ではAsciiDoc文書の検査(`cargo make docs-check`)を実行できません。運用と利用には影響しません。

公開前の人手受入は、配備した環境での画面操作と外部MCPクライアントの確認を対象としています。この版では自動検証だけを実施しているため、配備後に実環境での確認を行ってください。

## 配布物の検証

機械可読な公開契約として、`openapi.json`と`mcp-tools.json`をこのReleaseへ添付しています。いずれもタグ上の同名ファイルとbyte単位で同じ内容で、GitHub Artifact Attestationを付与しています。次のコマンドで、assetが本リポジトリのworkflowから作られたことを確認できます。

```sh
gh attestation verify openapi.json --repo KeishiS/marginalis
```

自動検証では、Kanidmログイン、ブラウザー操作、ACL、MCP認可、Webhook、schema検査、archiveの移行と復元を確認しています。配備後は、実際に使用する外部MCPクライアント、最新の実archiveを用いた隔離復元、外部Webhook受信サーバーでも受入確認することを推奨します。
