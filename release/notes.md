# Marginalis v0.53.1

## 主な変更

### MCPから取得できる数学文書の書き方

`get_note_profile`の`authoring_guidance`へ、定義、命題、補題、定理、系、証明に対応する`role`と、AsciiDoc open blockへの指定方法を追加しました。題名、共有カウンタ、anchor、xref、証明終了記号の扱いも確認できます。

`examples`には`mathematical_statements`を追加しました。定義、定理、証明、系を一つの完全なAsciiDoc文書として示し、MCPの書き込みで拒否されるwarningが生じないことを自動検証しています。

## 対応環境

Rust 1.97.1とNode.js 24.19.0で構築しています。実行環境へこれらの開発用toolchainを導入する必要はありません。

## 公開契約と破壊的変更

MCPの`get_note_profile`が返す執筆支援情報を更新し、OpenAPIの`x-note-profile-version`を20へ上げました。`get_note_profile`のJSON構造、MCP toolの要求、REST APIの要求・応答、SQLite schemaはv0.53.0から変更していません。

archiveの保存契約はv0.53.0と同じ次の組です。MCPで公開する執筆規則の版とは別の契約であり、今回の変更では更新しません。

- `marginalis-archive-18`
- AdocWeave package 0.57.0
- archive note profile 6

保存契約が変わらないため、`migrate-archive`が直接受理する直前契約も変更していません。

## v0.53.1への移行

この版はv0.53.0と同じSQLite schema 23を使用します。データベースとarchiveの移行コマンドは不要です。NixOS環境では通常どおり更新してください。

```sh
sudo nixos-rebuild switch --flake <利用中のflake>
```

MCPクライアントは接続後に`get_note_profile`を取得し直してください。応答を保存している場合は、profile版20の内容へ更新してください。JSON構造は変わらないため、クライアントコードの変更は不要です。

## 更新とロールバック

更新前に通常のバックアップを完了させてください。問題がある場合はMarginalisを停止し、v0.53.0を使用していたNixOS generationへ戻します。SQLite schemaとarchive保存契約は同じため、データベースやarchiveの変換は不要です。

v0.53.1の案内から作成した数学文書の原文はv0.53.0でも保存・表示できます。v0.53.0へ戻した後は`get_note_profile`が詳しい説明と例を返さないため、MCPクライアントが保存しているprofileも版19へ戻してください。

## 既知の制約

数学文書用の種類は表示と執筆支援のための`role`であり、論理的な正しさは検査しません。番号はAsciiDocのカウンタを原文で指定します。

今回追加した完全な例は、代表として定義、定理、証明、系を含みます。命題と補題は`authoring_guidance`に示す`proposition`と`lemma`を同じopen block構文へ指定してください。

公開前の自動受入では、公開例がwarningなしで保存できること、数学文書用の枠、番号、anchor、xrefが描画へ残ること、MCPの`structuredContent`と`text`の双方へ案内と例が含まれることを確認しました。さらに、Chromium、Firefox、aarch64、NixOS仮想マシン、実Kanidmを含む検証を実施しました。実配備先でのOIDC login、外部MCPクライアント、Webhook受信は公開後に確認してください。

## 配布物の検証

機械可読な公開契約として、`openapi.json`と`mcp-tools.json`をこのReleaseへ添付します。いずれも候補として検証したファイルとbyte単位で同じ内容とし、GitHub Artifact Attestationを付与します。次のコマンドで、assetが本リポジトリのworkflowから作られたことを確認できます。

```sh
gh attestation verify mcp-tools.json --repo KeishiS/marginalis
gh attestation verify openapi.json --repo KeishiS/marginalis
```
