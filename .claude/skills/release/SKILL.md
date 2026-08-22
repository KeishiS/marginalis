---
name: release
description: Marginalisのリリースをrelease.adocに従って段階実行し、各段の完了条件を検証してから公開する
---

# Marginalisのリリース実行

手順の正本は`docs/developer-guide/release.adoc`です。まずそれを読み、以下の運用知を重ねて
順に実行してください。矛盾がある場合はrelease.adocを優先し、このskillの更新を提案してください。

公開は、検証済みの`main`のcommitをそのままtagとReleaseへ昇格する方式です。人が行うのは、
版上げPRのマージと、`main`の先端SHAを指定した公開workflowの実行だけです。tagの作成、
Release Notesの転記、下書きの公開は行いません。

## 全体の禁止事項

- **版上げPRのマージから公開workflowの成功まで、他のPRをマージしない。** mainが進むと
  指定したSHAが先端でなくなり、workflowが公開を拒否します。
- `git tag`や`gh release create`などでtagとReleaseを手作業で作らない。
- リリースタグとGitHub Releaseを削除しない(protect-release-tagsにより不可)。

## 手順

1. **事前確認**: `git fetch`のうえで、open PRが0件、mainの最新CIが成功していることを確認する。
   最新タグからmainまでの差分を要約し、次版番号の判断(featがあればminorを上げる。破壊的変更も
   patchのままにせずminorを上げる)とあわせてユーザーへ提示する。
2. **剪定と外部依存**: ADR 0017に従い、5マイナー世代より前へ落ちたarchive契約を
   `SUPPORTED_MIGRATION_CONTRACTS`と`docs/user-guide/nixos.adoc`から削除する(該当があれば
   版上げPRに含める)。共有crateの固定は`cargo make pinned-git-crates`で確認し、指している
   SHAが上流の公開済み内容であることを各保守文書の手順で確かめる。
3. **版上げPR**: workspace版と`release-manifest.json`の`packageVersion`を同じ版へ上げ、
   `release/notes.md`へ今回のRelease Notesを記述する。必須見出しと本文の有無は
   `cargo make verify`(内部の`release-contract`)が検査する。**Release Notesの本文は
   ユーザーへ提示して承認を得てから**PRへ含める。公開前の完全なゲートは
   `cargo make release-check`で確認し、PR本文へ変更目的・外部依存の確認・人手受入の判断・
   実行した検証を記載して、auto-merge(squash)を設定しマージ完了を監視する。
4. **候補の確認**: マージ後のmainのCIが成功し、`release-candidate` jobが候補artifactを
   作ったことを確認する。infra起因の失敗であれば`gh run rerun <run-id> --failed`で再実行する
   (コード起因ならリリースを中止して修正へ戻る)。
5. **公開**: `git fetch upstream main`のうえで先端SHAを取得し、そのSHAを指定して
   `gh workflow run release-dispatch.yml --ref main --field candidate_sha="$candidate_sha"`を
   一度だけ実行する。workflowはSHAが先端であること、同じSHAの候補が成功していること、
   tagとReleaseがないことを検査してから、tag、attestation、asset、Release Notesを作って公開する。
6. **確認**: workflowの成功、公開されたReleaseのURL、assetが`openapi.json`と`mcp-tools.json`の
   ちょうど2件であることを確認してユーザーへ報告する。`binary-cache` jobだけが失敗した場合は、
   公開はそのままで該当jobを再実行する。
7. **事後処理**: ローカルの作業ブランチを削除し、mainを更新する。関連するメモリーが
   あれば公開済みの旨を反映する。

## 監視の型

マージやCIの完了待ちは、sleepの繰り返しではなくbackgroundのwatcherを使います。

```sh
until state=$(gh pr view <PR> --json state --jq .state); [ "$state" = "MERGED" ]; do
  failed=$(gh pr checks <PR> | awk '$2=="fail"'); [ -n "$failed" ] && { echo "$failed"; exit 1; }
  sleep 180
done
```
