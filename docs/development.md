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

開発shellには固定したNode.jsとpnpmも含まれます。フロントエンドだけを変更する場合は、
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

3. 変更を独立した単位でコミットし、push前の検証を実行します。

   ```sh
   nix develop --command cargo make pre-push
   ```

   `pre-push`はGitHub Actionsの`ci-verify`、`ci-early`、`ci-coverage`、
   `ci-nixos-e2e`をまとめて実行し、通常検証、現在のCPU上でのnativeコンパイル、
   軽量ブラウザー試験、カバレッジ測定、すべてのNixOS VM E2Eテストを確認します。
   `cargo make`をタスク名なしで実行した場合も同じ検証を行います。
   開発中に短い周期で確認する場合は`cargo make verify`を使用し、push前には
   `pre-push`を省略しないでください。

   CIでは、包括的なNixOS VMと並列に、早期検出用の二つの検査も実行します。

   - `native-aarch64`はGitHubの`ubuntu-24.04-arm`標準runnerで`cargo make native-check`を
     実行し、aarch64 Linux上で全Rust targetとfeatureをコンパイルします。x86_64上でも
     同じtaskを実行できますが、CPU固有の問題を再現するにはaarch64環境が必要です。
   - `browser-smoke`は`cargo make browser-smoke`を実行し、production用Web UIをChromiumで
     開いて、起動、API応答の表示、主要なリンク、ブラウザー例外の不在を確認します。
     `cargo make browser-editor-firefox`はFirefoxで編集欄の書体と範囲選択を確認します。APIは
     試験内で模擬するため、Kanidm、TLS、Cookie、実データベースは検査しません。

   これらは包括的なNixOS VMと依存関係を持たせず並列実行します。そのため、正常時の完了時刻を
   遅らせず、CPU固有のコンパイル失敗や画面の基本回帰をVMの完了前に確認できます。
   認証、認可、サブパス、TLS、永続化を含む統合動作は、引き続き`cargo make
   ci-nixos-e2e`が検査します。

   coverageの対象と解釈は[本番到達性とカバレッジ](coverage.md)を参照してください。

   CI jobの責務とローカルでの再実行方法は次のとおりです。jobを追加するときは、既存jobへ
   含められない独立した責務と、失敗時に残す証拠をこの表へ追加します。

   | job | 責務 | ローカルでの再実行 | 失敗時または完了時の証拠 |
   | --- | --- | --- | --- |
   | `changes` | 文書だけの変更かを一か所の規則で分類 | `bash .github/scripts/classify-ci-change.sh upstream/main HEAD` | path分類stepの標準出力 |
   | `verify` | 整形、静的解析、単体・結合試験、依存・ログ・公開契約・文書・Nix評価 | `cargo make ci-verify` | 失敗したtask名と標準出力 |
   | `coverage` | workspaceと統合経路のcoverage測定 | `cargo make ci-coverage` | `coverage-*` artifactの概要とJSON |
   | `native-aarch64` | aarch64 Linux上の全Rust target・featureのコンパイル | `cargo make native-check` | Cargoの診断 |
   | `browser-smoke` | production用Web UIの起動、主要操作、固定画像、ChromiumとFirefoxの編集欄互換性、ブラウザー例外 | `cargo make browser-smoke`と`cargo make browser-editor-firefox` | 失敗時の`browser-smoke-failure-*` artifactにscreenshotとtrace |
   | `nixos-e2e` | Nix package、module、TLS、Kanidm、Auth0 token、永続化、保守unit | `cargo make ci-nixos-e2e` | 試験実行失敗時の`nixos-e2e-failure-*` artifactに、秘密情報を除去したrunner出力 |
   | `release-gate` | 公開対象の版、受入結果、全検証、Nix packageを公開前に確認 | `MARGINALIS_RELEASE_TAG=vX.Y.Z cargo make release-gate` | 失敗したtask名と標準出力 |

   GitHub Actionsのartifactは試験環境のデータだけを含みます。実環境のログ、token、Cookie、
   ノート本文を追加しません。NixOS E2Eのrunner出力は、保存前に秘密情報を除去して検査します。
   coverageは14日、失敗時のブラウザーとVMの証拠は7日保持します。環境準備中の失敗などで
   試験成果物を生成できない場合は、GitHub Actionsのstep出力を確認します。
   各jobの`timeout-minutes`は停止上限であり、通常時間の目標ではありません。継続して上限の半分を
   超える場合は、責務を弱めずcache、並列化、重複buildを見直します。

   RESTまたはMCPの公開契約を変更する場合は、`marginalis-contract`を正本として生成物を更新します。

   ```sh
   nix develop --command cargo run -p marginalis-contract --bin generate
   nix develop --command cargo make contract-check
   ```

   この処理は`docs/openapi.json`、`docs/mcp-tools.json`、
   `frontend/src/generated/contracts.ts`を更新します。生成物を直接編集しないでください。
   `contract-check`は生成し直した内容との差分を検査します。RESTの全経路がOpenAPIとHTTPルーターの
   両方に存在することと、MCPのtool名、入出力schema、実行時応答が一致することはRustの契約試験で
   検査します。

   失敗した責務だけを再実行する場合は、suite名を指定します。

   ```sh
   nix develop --command cargo test -p marginalis-web http::tests::rest_notes
   nix develop --command cargo test -p marginalis-sqlite tests::notes
   nix develop --command cargo test -p marginalis-auth-oauth
   nix develop --command cargo test -p marginalis-web http::tests::mcp_transport
   ```

   `docs/**`、ルートの案内文書、Issueテンプレートだけを変更する場合は、次の文書検査だけで
   十分です。

   ```sh
   nix develop --command cargo make docs-check
   nix develop --command cargo make traceability-check
   ```

   `docs-check`は空白とローカルリンク、`traceability-check`は要件IDの対応と版別受入証跡を
   独立して検査します。

   CIは変更pathを判定し、文書だけのPull Requestでは`verify`を文書検査へ縮退し、
   `coverage`とNixOS VMの実行を省略します。プログラムが参照する公開仕様
   `docs/openapi.json`と`docs/mcp-tools.json`は、この省略対象に含めません。`.github/**`、
   `Makefile.toml`、Nix、Rust、公開仕様その他のファイルを同時に変更した場合は、通常の検証を
   すべて実行します。この判定規則は
   `.github/scripts/classify-ci-change.sh`と、その入力を判定する
   `.github/scripts/classify-docs-only.sh`に集約し、`cargo make verify`から境界例を検査します。

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
   `native-aarch64`と`browser-smoke`は、継続的に安定して完了することを確認してからrulesetの
   必須チェックへ追加します。追加前も失敗を無視せず、原因を解消してからマージします。

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
