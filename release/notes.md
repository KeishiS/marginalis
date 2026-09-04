# Marginalis v0.53.0

## 主な変更

### 数学文書用ブロック

定義、命題、補題、定理、系、証明を、AsciiDocのopen blockへ専用の`role`を付けて記述できるようにしました。閲覧画面と編集画面のプレビューでは、ブロック全体を枠で囲み、題名、線、余白によって本文との範囲を示します。証明は破線の枠と証明終了記号で区別します。

利用できる`role`は`definition`、`proposition`、`lemma`、`theorem`、`corollary`、`proof`です。表示規則はopen blockだけに適用し、通常の段落や許可していない`role`へは波及しません。数式、カウンタによる番号、anchor、xrefは従来どおり利用できます。

### IME変換入力中の装飾位置

編集画面で日本語IMEを使って入力したとき、Live Previewの装飾が関係のない文字へ一時的に移る不具合を修正しました。IMEの変換中は既存の装飾を本文の変更に合わせて移動し、変換が確定するとサーバーが返す最新の解析結果へ更新します。

### 依存管理とCI

`fast-uri`を修正版へ固定しました。また、npm Audit APIの障害が通常のPull Requestを停止させないように、依存差分の検査と全依存のオンライン監査を分離しました。Pull Requestでは追加・更新した依存を必ず検査し、全依存の監査は定期実行とリリース前に行います。

## 対応環境

x86_64とaarch64のLinuxで動作するNixOSモジュールとして配布します。利用者認証には標準のOIDC IdPが必要で、参照実装はKanidmです。Web UIはChromiumとFirefoxの最新版で確認しています。

Rust 1.97.1とNode.js 24.19.0で構築しています。実行環境へこれらを用意する必要はありません。

## 公開契約と破壊的変更

MCPの`get_note_profile`が返す`syntax`へ、必須項目`allowed_mathematical_block_roles`を追加しました。値は、HTMLへ残して数学文書用の表示を適用できる6種類の`role`です。MCPクライアントが応答を旧schemaで厳密に検査している場合は、v0.53.0の`mcp-tools.json`へ更新してください。

OpenAPIの`x-note-profile-version`とMCPの執筆規則を19へ更新しました。REST APIの要求・応答、その他のMCP要求、SQLite schemaはv0.52.0から変更していません。

archiveの保存契約はv0.52.0と同じ次の組です。MCPで公開する執筆規則の版とは別の契約であり、今回の変更では更新しません。

- `marginalis-archive-18`
- AdocWeave package 0.57.0
- note profile 6

保存契約が変わらないため、`migrate-archive`が直接受理する直前契約も変更していません。

## v0.53.0への移行

この版はv0.52.0と同じSQLite schema 23を使用します。データベースとarchiveの移行コマンドは不要です。NixOS環境では通常どおり更新してください。

```sh
sudo nixos-rebuild switch --flake <利用中のflake>
```

更新後は`marginalis-diagnose.service`、HTTP health、OIDC login、既存ノートの表示を確認してください。数学文書用ブロックを使う場合は、利用者ガイドの例に従ってopen blockへ題名と`role`を指定します。既存のAsciiDoc原文を書き換える必要はありません。

MCPクライアントが`get_note_profile`の応答を保存または厳密に検査している場合は、同梱する`mcp-tools.json`から契約を再生成してください。

## 更新とロールバック

更新前に通常のバックアップを完了させてください。問題がある場合はMarginalisを停止し、v0.52.0を使用していたNixOS generationへ戻します。SQLite schemaとarchive保存契約は同じため、データベースやarchiveの変換は不要です。

v0.53.0で追加した数学文書用の`role`を含む原文はv0.52.0でも失われませんが、専用の枠と種類ごとの表示は適用されません。`allowed_mathematical_block_roles`を必須とする新しいMCPクライアントは、v0.52.0へ戻したサーバーの旧`get_note_profile`応答を受理できないため、クライアントの契約も同時に戻してください。

## 既知の制約

数学文書用の種類は表示と執筆支援のための`role`であり、定理証明支援系のような論理的な正しさの検査は行いません。番号はAsciiDocのカウンタを原文で指定します。種類が文章だけでも分かるように、各ブロックには題名を付けてください。

Live Previewの装飾はサーバーが解析した結果を正本とするため、通常の入力中は応答を受け取るまで短い遅延が生じることがあります。IMEの変換中は新しい構文を解析せず、直前に確定した装飾の位置だけを本文へ追従させます。

MCPはrevision番号を指定した取得に対応しますが、履歴の一覧、二つの版の差分、過去版への復元は提供しません。添付できるのは画像だけで、汎用のファイル保存には対応していません。

公開前の自動受入では、6種類の`role`、未許可の`role`の除去、数式・番号・参照の維持、通常段落への非干渉、IME変換入力中の装飾追従を確認しました。さらに、Chromium、Firefox、aarch64、NixOS仮想マシン、実Kanidmを含む検証を実施しました。実配備先でのOIDC login、外部MCPクライアント、Webhook受信は公開後に確認してください。

## 配布物の検証

機械可読な公開契約として、`openapi.json`と`mcp-tools.json`をこのReleaseへ添付しています。いずれも候補として検証したファイルとbyte単位で同じ内容で、GitHub Artifact Attestationを付与しています。次のコマンドで、assetが本リポジトリのworkflowから作られたことを確認できます。

```sh
gh attestation verify mcp-tools.json --repo KeishiS/marginalis
gh attestation verify openapi.json --repo KeishiS/marginalis
```

自動検証では、ノート検証・描画、Kanidm login、ブラウザー操作、ACL、MCP認可、Webhook、schema検査、archiveの移行と復元を確認しています。配備後は、実際に使用する外部MCPクライアント、最新の実archiveを用いた隔離復元、外部Webhook受信サーバーでも受入確認することを推奨します。
