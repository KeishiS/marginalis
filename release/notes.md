# Marginalis v0.51.0

## 主な変更

### AdocWeave v0.57.0への更新

ノート本文の検証、HTML描画、編集画面の装飾、開発文書の検査に使うAdocWeaveをv0.57.0へ更新しました。

上流の公開構成に合わせ、Rustライブラリは`adocweave-core`を使用します。Marginalisが使用している解析・意味モデル・HTML描画APIは維持されており、ノートの入力規則を表すnote profile版は6のままです。AdocWeaveのtextlint用Processorは0.54.0へ更新しました。

AdocWeave CLIの`--local-targets`廃止へ追随し、`--project-root`によるリポジトリ内参照の検査を継続します。利用者向けのMarginalisコマンドは変わりません。

### 依存固定と移行経路の更新

AdocWeaveの公開commit、Cargo依存、Nix input、textlint用Processorをそれぞれ完全な版へ固定し、相互の不一致を検査します。公開取り下げ済みだった間接依存`chacha20 0.10.1`は0.10.2へ更新しました。

保存契約の履歴には、AdocWeave package版0.57.0の組を初めて採用したMarginalis版としてv0.51.0を記録しました。直前の公開済み保存契約からだけ変換する方針は変わりません。

## 対応環境

x86_64とaarch64のLinuxで動作するNixOSモジュールとして配布します。利用者認証には標準のOIDC IdPが必要で、参照実装はKanidmです。Web UIはChromiumとFirefoxの最新版で確認しています。

Rust 1.97.1とNode.js 24.19.0で構築しています。実行環境へこれらを用意する必要はありません。

## 公開契約と破壊的変更

REST API、MCP、SQLite schema、note profile版はv0.50.1から変更していません。

この版が書き出すarchiveは、次の保存契約を使います。

- `marginalis-archive-18`
- AdocWeave package 0.57.0
- note profile 6

`migrate-archive`と`restore-archive`が直接受理する旧契約は、v0.50.0とv0.50.1が書き出した`marginalis-archive-18` / 0.47.0 / 6です。さらに古いarchiveは、NixOS運用文書の保存契約履歴に記録した公開済みリリースを順番に使ってください。

## v0.51.0への移行

この版はSQLite schema 23を使用します。v0.50.1と同じschemaのため、データベースの移行コマンドは不要です。

NixOS環境では通常どおり更新してください。

```sh
sudo nixos-rebuild switch --flake <利用中のflake>
```

v0.50.0またはv0.50.1が書き出したarchiveを復元する場合は、v0.51.0の`restore-archive`へ直接渡せます。入力を変更せず、0.57.0の規則で全ノートを再検証し、隔離データベースで復元結果を照合してから取り込みます。

```sh
sudo -u marginalis marginalis restore-archive \
  --input /srv/marginalis-migration/archive-v0.50.1.json
```

更新後は`marginalis-diagnose.service`、HTTP health、OIDC login、MCP認可、既存ノートの表示を確認してください。

## 更新とロールバック

更新前に通常のバックアップを完了させてください。問題がある場合はMarginalisを停止し、v0.50.1を使用していたNixOS generationへ戻します。SQLite schemaは同じため、更新後にノートや認可状態を変更していなければ、データベースの変換は不要です。

更新後にデータを変更した場合や、v0.57.0で新たに受理される記法を保存した場合は、切戻し先で内容を検証できるとは限りません。更新前のデータベースsnapshotまたはarchiveとv0.50.1を組にして戻してください。v0.51.0が書き出した0.57.0契約のarchiveをv0.50.1は直接受理しません。

## 既知の制約

添付できるのは画像だけで、汎用のファイル保存には対応していません。同一人物の自動推定を行わないため、利用者のOIDC identityが変わった場合は管理者がaliasを明示的に結び付ける必要があります。

AdocWeaveがNix packageの公開先をLinuxに限定しているため、macOSの開発環境ではAsciiDoc文書の検査(`cargo make docs-check`)を実行できません。運用と利用には影響しません。

公開前の人手受入は、公開済みv0.50.1の実行物で書き出したarchiveの移行・隔離復元と、Nix packageの起動確認を対象としました。実配備先でのOIDC login、外部MCPクライアント、Webhook受信は公開後に確認してください。

## 配布物の検証

機械可読な公開契約として、`openapi.json`と`mcp-tools.json`をこのReleaseへ添付しています。いずれもタグ上の同名ファイルとbyte単位で同じ内容で、GitHub Artifact Attestationを付与しています。次のコマンドで、assetが本リポジトリのworkflowから作られたことを確認できます。

```sh
gh attestation verify openapi.json --repo KeishiS/marginalis
```

自動検証では、AdocWeaveの固定、ノート検証・描画、Kanidm login、ブラウザー操作、ACL、MCP認可、Webhook、schema検査、archiveの移行と復元を確認しています。配備後は、実際に使用する外部MCPクライアント、最新の実archiveを用いた隔離復元、外部Webhook受信サーバーでも受入確認することを推奨します。
