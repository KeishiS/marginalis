# AGENTS.md

## Communication

- 日本語で対応する。
- 簡単な質問には簡潔に答え、必要性のない長い背景説明や過剰な箇条書きを避ける。
- 研究に関する文章では、仮定、定義、主張、考察を区別する。

## Working agreement

- 原則として、プロジェクトの既存フォーマットと命名規則に従う。
- 変更後は関連する範囲の検証を実行する。
- 破壊的操作または外部へ影響する操作は事前に確認する。

## Git operations

- 環境変数の`GIT_AUTHOR_*`と`GIT_COMMITTER_*`を変更または削除しない。
- `git status`、`git diff`、`git log`などの読み取り操作は必要に応じて実行する。
- 可能な限り、独立した変更単位でコミットする。
- GitHub上の本リポジトリを指すリモート名は`upstream`とする。
- `main`への直接pushは禁止する。必ず目的が分かる作業ブランチを作成し、Pull Requestから
  `main`へマージする。
- force pushやブランチ保護の回避を行わない。

## GitHub operations

- GitHubのPull Request、Issue、Actions、Releaseを操作するときは、原則として`gh`を使う。
  `gh`は`nix develop`の開発環境に含まれる。
- 外部操作の前に対象リポジトリ、ブランチ、Pull Requestを読み取り確認する。
- Pull Requestには変更目的、主な差分、実行した検証を記載する。
- 必須チェックが成功し、必要なレビューを得るまでマージしない。
- リリースタグは、リリース用Pull Requestが`main`へマージされ、対象コミットで
  リリースゲートが成功した後に作成する。

詳細な手順は[GitHubを使う開発手順](docs/development.md)に従う。

## Subagents and Git worktrees

- 単純な質問や、前の結果に依存する逐次作業にはサブエージェントを使用しない。
- 独立して実行できる境界の明確な調査、検証、テスト、レビューには
  サブエージェントを適切に使用する。
- エージェントがファイルを書き換える場合は、Git worktreeと作業ブランチを活用する。
- worktree作成前に、未コミット変更と既存worktreeを確認する。
- 親エージェントはサブエージェントの結果、差分、検証結果を確認し、統合と最終回答に
  責任を持つ。
