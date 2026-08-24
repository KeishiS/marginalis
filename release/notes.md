# Marginalis v0.50.0

## 主な変更

### AdocWeave 0.47.0

AsciiDocの解析と描画をAdocWeave 0.47.0へ更新しました。Marginalisが受理する入力規則と、ノートの描画結果は変えていません。この更新にともない、archiveが記録するAdocWeave package版が`0.47.0`になります。詳しくは「公開契約と破壊的変更」を参照してください。

AdocWeaveは、この版から製品ごとに版を分けて公開する方式になりました。Marginalisが取り込むのはRustライブラリとtextlint用Processorで、どちらも0.47.0です。

### バックアップの既定保持世代

`services.marginalis.backupRetention`の既定値を30から7へ変更しました。日次バックアップなので1週間分にあたります。**この設定を明示していない配備では、更新後の最初の保持処理で成功世代が7件まで減ります。** 移行の手順を必ず確認してください。

想定する運用規模(利用者10名程度、ノート約1,000件)に対して30世代は容量の要求が大きく、より長い履歴が必要な場合は保存媒体側のsnapshotやoff-site複製で持つほうが、容量と復元性の両面で扱いやすいと判断しました。

### 構造化ログ

ノートのUUIDである`note_id`をログへ記録できるようにしました。単一ホストの運用ではログとデータベースが同じ信頼境界にあり、どのノートで失敗したかを追えることの利点が上回るためです。Cookie、token、認可code、client secret、利用者identity、ノート本文、題名、タグ、検索語、HTTP headerとbodyを記録しない規則は変えていません。

## 対応環境

x86_64とaarch64のLinuxで動作するNixOSモジュールとして配布します。利用者認証には標準のOIDC IdPが必要で、参照実装はKanidmです。Web UIはChromiumとFirefoxの最新版で確認しています。

Rust 1.97.1とNode.js 24.19.0で構築しています。実行環境へこれらを用意する必要はありません。Node.jsは前の版の22.23.1から上がりました。

## 公開契約と破壊的変更

REST APIとMCPの経路、要求と応答の形は変更していません。`openapi.json`が記録するAdocWeave package版が`0.47.0`になります。

### archive保存契約

v0.50.0が初めて書き出す現行契約は、次の組です。

- `marginalis-archive-18`
- AdocWeave package 0.47.0
- note profile 6

**移行元として受理するarchiveが変わります。** `migrate-archive`と`restore-archive`が受理する旧契約は、v0.49.0が書き出した`marginalis-archive-18` / 0.43.0 / 6だけになりました。v0.48.0以前が書き出したarchiveは、この版だけでは復元できません。利用者ガイドの保存契約の履歴に従い、対応する公開版で契約を一つずつ持ち上げてから渡してください。

新旧の契約は記録する項目が同じで、AdocWeave package版だけが異なります。移行では全ノートを0.47.0で再検証し、代表identityとalias群、保持中の全版、添付画像を引き継ぎます。

### バックアップの保持世代

`backupRetention`の既定値が30から7へ変わります。設定を明示している配備は影響を受けません。明示していない配備では保持数が減るため、更新前に「v0.50.0への移行」を確認してください。

## v0.50.0への移行

この版はSQLite schema 23を使用します。v0.49.0と同じschemaのため、データベースの移行は不要です。

**更新前に、バックアップの保持世代を確認してください。** `backupRetention`を明示していない配備では、更新後の最初の`marginalis-backup.service`の実行で、検証済み成功世代が新しい既定値の7件まで削除されます。削除した世代は戻せません。これまでどおり30世代を保持する場合は、更新と同じ変更で明示してください。

```nix
services.marginalis.backupRetention = 30;
```

保持数を減らしてよい場合でも、更新前に最新のarchiveを別の保存先へ複製しておくことを推奨します。

確認したうえで、NixOS環境では通常どおり更新してください。

```sh
sudo nixos-rebuild switch --flake <利用中のflake>
```

更新後は`marginalis-diagnose.service`、HTTP health、OIDC login、MCP認可を確認してください。

保管しているarchiveがv0.48.0以前のものである場合は、この版へ更新する前に、v0.49.0で`marginalis-archive-18` / 0.43.0 / 6へ変換して保管し直すことを推奨します。

## 更新とロールバック

切戻しでは、新しいserviceを停止し、移行前のNixOS generationへ戻します。schemaを変更していないため、データベースの入れ替えは不要です。

**保持処理で削除したバックアップ世代は、切戻しでは戻りません。** `backupRetention`の既定値変更による削除は、NixOS generationを戻しても復元されないため、更新前の確認が切戻し手段の代わりにはなりません。

v0.50.0が書き出したarchiveは、v0.49.0では現行契約として受理されません。切戻し後に復元が必要な場合は、切戻し前に取得したv0.49.0のarchiveを使ってください。

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
