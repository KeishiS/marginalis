# Marginalisへの参加

この文書は、AIエージェントを含む開発参加者向けの案内です。開発環境、検証、GitHubでの詳しい
作業手順は[GitHubを使う開発手順](docs/development.md)、システムの仕様や運用手順は
[資料案内](docs/README.md)を参照してください。

## 作業の始め方

1. 作業前に関連するGitHub Issueと現行仕様を確認します。
2. 最新の`main`から目的が分かる作業ブランチを作成します。
3. 既存の形式とコンポーネントの役割分担に従い、変更に対応する文書とテストも更新します。
4. 関連する検証を実行し、目的、主な差分、検証結果をPull Requestへ記載します。
5. Pull Requestから`main`へマージします。`main`へ直接pushしません。

新しい作業項目と完了条件はGitHub Issuesで管理します。機能の修正、改善、追加では、
Issueテンプレートに現在と理想の動作例を記載してください。設計判断を長期に参照する場合は、
実装Issueとは別に[文書管理方針](docs/documentation.md)に従ってADRを追加します。

## 基本検証

```text
cargo make format
cargo make verify
cargo make pre-push
```

`verify`は開発中の通常検証、`pre-push`はカバレッジ測定とすべてのNixOS VM E2Eテストを
含むpush前の検証です。

文書だけを変更した場合も`cargo make docs-check`を実行します。公開前の検証と受入は
[リリース手順](docs/release.md)に従います。

## セキュリティー

秘密情報、個人情報、実際のCookie、token、認可code、client secret、ノート本文をIssue、
Pull Request、ログ、テストの成果物へ記録しません。脆弱性や認証情報の漏えいが疑われる場合は、
公開Issueを作成せず、リポジトリ所有者へ非公開経路で連絡してください。
