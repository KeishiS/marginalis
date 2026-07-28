# GitHubを使う開発手順

この文書は、AIエージェントを含む開発参加者に向けて、作業ブランチの作成からPull Requestの
マージまでを説明します。リリースの詳しい操作は[リリース手順](release.md)を参照してください。

## 開発環境

開発コマンドはNix開発環境で実行します。`gh`はPATHから直接実行します。
`nix develop --command gh`で包む必要はありません。

```sh
nix develop
gh auth status
```

開発shellには固定したNode.jsとnpmも含まれます。フロントエンドだけを変更する場合は、
次のコマンドで整形、静的解析、型検査、単体試験、依存監査、production buildをまとめて
確認できます。

```sh
cargo make frontend-verify
```

通常の`cargo make verify`と`cargo make coverage`は必要なフロントエンドアセットを先に
構築します。`frontend/dist`と`frontend/node_modules`は生成物であり、Gitへ追加しません。

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

   ```sh
   nix develop --command cargo make verify
   nix develop --command cargo make coverage
   ```

   coverageの対象と解釈は[本番到達性とカバレッジ](coverage.md)を参照してください。

   RESTまたはMCPの公開契約を変更する場合は、`marginalis-contract`を正本として生成物を更新します。

   ```sh
   nix develop --command cargo run -p marginalis-contract --bin generate
   nix develop --command cargo make openapi-check
   ```

   この処理は`docs/openapi.json`と`frontend/src/generated/contracts.ts`を更新します。生成物を直接
   編集しないでください。`openapi-check`は生成し直した内容との差分を検査し、契約に含まれる全経路が
   OpenAPIとHTTPルーターの両方に存在することはRustの単体試験で検査します。

   `docs/**`、ルートの案内文書、Issueテンプレートだけを変更する場合は、次の文書検査だけで
   十分です。

   ```sh
   nix develop --command cargo make docs-check
   ```

   CIは変更pathを判定し、文書だけのPull Requestでは`verify`を文書検査へ縮退し、
   `coverage`とNixOS VMの実行を省略します。プログラムが参照する公開仕様
   `docs/openapi.json`は
   この省略対象に含めません。`.github/**`、`Makefile.toml`、Nix、Rust、OpenAPIその他のファイルを
   同時に変更した場合は、通常の検証をすべて実行します。

   新しい作業項目はGitHub Issuesへ作成します。リポジトリ内にIssueファイルを追加しません。
   v0.5.0以前のローカルIssueとの対応は[移行対応表](issue-migration.md)を参照してください。
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
   GitHub Actionsの`verify`と`nixos-e2e`が必須であるため、このチェックと必要なレビューが完了するまで
   実際のマージは行われません。文書だけの変更でもチェック名は維持し、`verify`は文書検査、
   `nixos-e2e`は明示的な省略成功として完了するため、必須チェックが待機状態に残りません。

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
`main`の先端で`release-gate`を手動実行します。入力する`release_tag`には作成予定のタグを
指定します。gateの成功後、検証した`main`のコミットへタグを付けます。タグのpushでも同じ
gateが再実行されます。

GitHub Actionsの確認には次のコマンドを使用します。

```sh
gh run list --workflow release-gate.yml
gh run watch <run-id>
```

`v0.2.0`の正式リリース後はリリース候補版を公開せず、`v0.x.y`形式の通常版だけを
この手順で公開します。
