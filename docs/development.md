# GitHubを使う開発手順

## 開発環境

開発コマンドはNix開発環境で実行します。`gh`もこの環境から利用できます。

```sh
nix develop
gh auth status
```

`gh auth status`で、操作対象のGitHubアカウントとホストを確認してください。認証情報や
アクセストークンをコマンド、ログ、Issue、Pull Requestへ記録してはいけません。

## ブランチとPull Request

`main`は保護ブランチです。ローカル・GitHubのどちらでも直接pushせず、すべての変更を
Pull Requestからマージします。

1. `main`をfast-forwardで最新化します。

   ```sh
   git switch main
   git pull --ff-only origin main
   ```

2. 目的が分かる名前の作業ブランチを作成します。

   ```sh
   git switch -c codex/<purpose>
   ```

3. 変更を独立した単位でコミットし、関連する検証を実行します。
4. 作業ブランチをpushし、`gh`でPull Requestを作成します。

   ```sh
   git push -u origin codex/<purpose>
   gh pr create --base main --head codex/<purpose>
   ```

5. Pull Requestの差分とチェックを確認します。

   ```sh
   gh pr diff
   gh pr checks --watch
   ```

6. 必須チェックが成功し、必要なレビューを得た後、GitHubで有効な方式を使ってマージします。
   線形履歴を維持できる場合はrebase mergeを使用します。

   ```sh
   gh pr merge --rebase --delete-branch
   ```

   リポジトリ設定がrebase mergeを許可していない場合は、許可された方式を選びます。
   ブランチ保護の無効化、管理者権限による必須チェックの回避、force pushは行いません。

7. マージ後にローカル環境を整理します。

   ```sh
   git switch main
   git pull --ff-only origin main
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
