---
name: release
description: Marginalisのリリースをrelease.adocに従って段階実行し、各段の完了条件を検証してから公開する
---

# Marginalisのリリース実行

手順の正本は`docs/developer-guide/release.adoc`です。まずそれを読み、以下の運用知を重ねて
順に実行してください。矛盾がある場合はrelease.adocを優先し、このskillの更新を提案してください。

## 全体の禁止事項

- **版上げPRのマージから公開完了まで、他のPRをマージしない。** タグ検証中にmainが進むと
  リリース候補が無効になります(v0.43.0が欠番になった原因)。
- リリースタグとGitHub Releaseを削除しない(protect-release-tagsにより不可)。
- Release Notesの記載と確認より前に公開(`--draft=false`)しない。

## 手順

1. **事前確認**: `git fetch`のうえで、open PRが0件、mainの最新CIが成功していることを確認する。
   最新タグからmainまでの差分を要約し、次版番号の判断(featがあればminorを上げる。破壊的変更も
   patchのままにせずminorを上げる)とあわせてユーザーへ提示する。
2. **剪定と外部依存**: ADR 0017に従い、5マイナー世代より前へ落ちたarchive契約を
   `SUPPORTED_MIGRATION_CONTRACTS`と`docs/user-guide/nixos.adoc`から削除する(該当があれば
   版上げPRに含める)。共有crateの固定は`cargo make shared-authorization-server`と
   `cargo make shared-oidc-login`で確認し、指しているSHAが上流の公開済み内容であることを
   各保守文書の手順で確かめる。
3. **版上げPR**: workspace版を上げ、`cargo make verify`の成功を確認してPRを作成する。
   PR本文へ変更目的・外部依存の確認・人手受入の判断・実行した検証を記載し、
   auto-merge(squash)を設定してマージ完了を監視する。
4. **release-gate(dispatch)**: mainの先端で`release-gate.yml`を`release_tag`付きで手動実行する。
   dispatch実行はHEADがorigin/mainの先端であることを検査するため、mainが進んでいないうちに
   実行する。失敗した場合はlogを確認し、cache.nixos.orgのdownload障害などinfra起因であれば
   `gh run rerun <run-id> --failed`で再実行する(コード起因ならリリースを中止して修正へ戻る)。
5. **タグ**: dispatchを通したmain先端のcommit SHAを明示指定してタグを作成しpushする。
   タグ起点のrelease-gateの成功と、draft Releaseの自動作成を待つ。
6. **draftの検査**: `isDraft`がtrueで、assetsが`openapi.json`と`mcp-tools.json`のちょうど2件で
   あることをrelease.adoc記載のjq式で確認する。不足や重複があれば公開せず調査する。
7. **Release Notes**: 概要・変更内容・互換性への影響(再ログインやデータ移行の要否を明示)・
   動作確認の構成で下書きを作成し、**ユーザーへ提示して承認を得てから**
   `gh release edit <tag> --notes-file <file>`で記載する。
8. **公開**: 6の検査をもう一度通してから`gh release edit <tag> --draft=false`で公開し、
   ReleaseのURLを報告する。
9. **事後処理**: ローカルの作業ブランチを削除し、mainを更新する。関連するメモリーが
   あれば公開済みの旨を反映する。

## 監視の型

マージやCIの完了待ちは、sleepの繰り返しではなくbackgroundのwatcherを使います。

```sh
until state=$(gh pr view <PR> --json state --jq .state); [ "$state" = "MERGED" ]; do
  failed=$(gh pr checks <PR> | awk '$2=="fail"'); [ -n "$failed" ] && { echo "$failed"; exit 1; }
  sleep 180
done
```
