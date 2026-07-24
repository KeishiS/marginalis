# GitHubを使う開発手順

## 開発環境

開発コマンドはNix開発環境で実行します。`gh`はPATHから直接実行します。
`nix develop --command gh`で包む必要はありません。

```sh
nix develop
gh auth status
```

`gh auth status`で、操作対象のGitHubアカウントとホストを確認してください。認証情報や
アクセストークンをコマンド、ログ、Issue、Pull Requestへ記録してはいけません。
本書では、GitHub上の本リポジトリを指すGitリモート名を`upstream`とします。

## ブランチとPull Request

`main`は保護ブランチです。ローカル・GitHubのどちらでも直接pushせず、すべての変更を
Pull Requestからマージします。

1. `main`をfast-forwardで最新化します。

   ```sh
   git switch main
   git pull --ff-only upstream main
   ```

2. 目的が分かる名前の作業ブランチを作成します。

   ```sh
   git switch -c codex/<purpose>
   ```

3. 変更を独立した単位でコミットし、関連する検証を実行します。
4. 作業ブランチをpushし、`gh`でPull Requestを作成します。

   ```sh
   git push -u upstream codex/<purpose>
   gh pr create --base main --head codex/<purpose>
   ```

5. Pull Requestの差分とチェックを確認します。

   ```sh
   gh pr diff
   gh pr checks --watch
   ```

6. Pull Request作成後にrebase方式のauto-mergeを設定します。`main`のrulesetでは
   GitHub Actionsの`verify`が必須であるため、このチェックと必要なレビューが完了するまで
   実際のマージは行われません。

   ```sh
   gh pr merge --auto --rebase --delete-branch
   ```

   auto-mergeを設定できない場合は、rulesetの必須チェックとリポジトリの
   `Allow auto-merge`設定を確認します。ブランチ保護の無効化、管理者権限による
   必須チェックの回避、force pushは行いません。

7. マージ後にローカル環境を整理します。

   ```sh
   git switch main
   git pull --ff-only upstream main
   git branch -d codex/<purpose>
   ```

## リリース

バージョン、変更履歴、リリース文書を変更する場合も、専用ブランチとPull Requestを使用します。
Pull Requestの必須チェックと[リリース手順](release.md)の検証が成功した後に`main`へマージし、
タグはマージ済みの`main`が指すコミットへ付けます。

GitHub Actionsの確認には次のコマンドを使用します。

```sh
gh run list --workflow release-gate.yml
gh run watch <run-id>
```

`v0.2.0`の正式リリース後はリリース候補版を公開せず、`v0.x.y`形式の通常版だけを
この手順で公開します。
