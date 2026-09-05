# AGENTS.md

## 作業の入口

- 最初に[参加案内](CONTRIBUTING.adoc)を読みます。本リポジトリには`CONTRIBUTING.md`ではなく
  `CONTRIBUTING.adoc`があります。
- 調査前に作業ツリーと関連Issueを確認し、変更は`agent/<種別>/<目的>`ブランチで管理します。
- 検証コマンドは[開発手順](docs/developer-guide/development.adoc)を正本とします。
  開発コマンドはNix環境で、`gh`は直接実行します。
- リリース作業では[release skill](.claude/skills/release/SKILL.md)を読みます。
  自動検出されない環境でも、このパスから参照できます。
- 指示や設定の更新時は[エージェント向け設定](docs/developer-guide/development.adoc#agent-configuration)を確認します。

## 文書の執筆

- リポジトリ固有の用語は[用語集](docs/user-guide/glossary.adoc)に従う。新しい用語を導入する場合は、既存の
  用語で説明できないか確認し、必要であれば用語集も同時に更新する。
- 同じ説明を複数の文書へ繰り返さず、詳しい説明を置く文書へのリンクを使用する。

## Git操作

- 環境変数の`GIT_AUTHOR_*`と`GIT_COMMITTER_*`を変更または削除しない。

## GitHub操作

- Pull Request作成後はsquash方式のauto-mergeを設定する。必須チェックが成功し、
  必要なレビューを得るまで実際のマージは行われない。マージ方式の理由と操作は
  [GitHubを使う開発手順](docs/developer-guide/development.adoc)に従う。
- `main`との衝突は`main`を作業ブランチへマージして解消する。force pushで履歴を書き換えない。
- リリースタグとGitHub Releaseは公開workflowだけが作成する。人はタグを作成もpushもせず、
  `main`の先端SHAを指定して公開workflowを実行する。
- 機能の修正、改善、追加ではIssueテンプレートに現在と理想の動作例を記載する。

詳細な手順は[GitHubを使う開発手順](docs/developer-guide/development.adoc)に従う。
