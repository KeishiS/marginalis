# 033: リポジトリ文書のAsciiDoc移行

## 状態

未着手。AdocWeave v0.6.1への移行（Issue 029）が完了した後に開始する。

## 目的

リポジトリ内で保守する文書をMarkdownからAsciiDoc（`.adoc`）へ移行する。ノート本文と
同じ文書形式を使い、AdocWeaveによる検証を共通化する。リンク、アンカー、コード例、表、
NixOS運用手順、GitHub上の閲覧性を維持する。

AdocWeave v0.6.1への移行前に文書形式を変更すると、API移行による差分と形式変換による差分を
分けて確認できない。このため、Issue 029の完了を開始条件とする。

## 対象と対象外

対象は、このリポジトリで保守する次のMarkdown文書である。

1. rootの`README.md`、`CHANGELOG.md`。
2. `docs/`配下の運用・仕様・受入・リリース・ロードマップ文書。
3. `issues/`および`issues/upstream/`配下のIssue・計画文書。

`docs/openapi.json`、Nix・Rust・JSON・YAMLなどの機械可読ファイル、ライセンス本文、
GitHub Actions設定、依存先の文書は対象外とする。拡張子だけを変える一括変更は行わない。

## 作業内容

1. 現在のMarkdownファイルを棚卸しし、変換対象、公開経路、相互参照、GitHubからの外部リンクを一覧化する。
2. 各文書を`.adoc`へ変換する。見出し、アンカー、箇条書き、表、注意書き、コードブロック、
   Nix・Rust・shellの言語指定、相対リンク、`docs/openapi.json`への参照を保つ。
3. `README.adoc`、`CHANGELOG.adoc`、`docs/*.adoc`、`issues/*.adoc`の参照先を更新する。
   Markdownへのリンク、古いアンカー、古い画像参照は残さない。外部から参照されるGitHub URLの
   扱いも決める。
4. AdocWeave v0.6.1で安全に変換できる文書の入力規則を定める。ノート本文に必要な
   メタデータやACLはリポジトリ文書へ要求しない。include、passthrough、外部ファイルの扱いは
   CIで検証する。
5. CIに文書検証を追加する。すべての`.adoc`の解析、内部xref、相対ファイルリンク、
   ソースブロックの言語指定、文書一覧の網羅性を検査する。壊れた参照は
   リリース前の必須検証で拒否する。
6. GitHub上の表示、clone後の`README`検出、NixOS運用手順へのリンク、Issue一覧を手動で確認する。
   移行後の執筆規則は`CONTRIBUTING`に相当する文書へ記す。

## 決定事項

- MarkdownとAsciiDocを長期間併存させない。一つのコミットで文書とリンクを`.adoc`へ切り替える。
- 形式移行と仕様変更を混ぜない。仕様や手順を変更する場合は、別Issueまたは変更記録に分ける。
- 既存の外部リンクを維持するためにリダイレクトが必要な場合は、リポジトリ外のHTTP
  リダイレクトとGit URLの扱いを切替前に決める。
- Issueの番号と内容を保ち、Gitが追跡できるファイル名変更として扱う。

## 完了条件

- 対象となる人間向けMarkdown文書が残っておらず、対応する`.adoc`文書と相互参照が存在する。
- README、CHANGELOG、`docs/`、`issues/`がGitHubとlocal cloneで閲覧・追跡できる。
- AdocWeave v0.6.1による解析・変換と内部リンク検証がCIとリリース前の必須検証で成功する。
- 文書形式、ソースブロック、リンク、アンカー、画像、外部URLの執筆規則が明文化される。
- OpenAPI など対象外の機械可読仕様と、既存の NixOS・REST・MCP 手順への参照が失われない。

## 依存関係

- 開始前提: [029: AdocWeave v0.6.1への移行](029-adocweave-v0.6.1-migration.md)
- CI統合: [021: テスト構成とリリース前検証](021-test-architecture-and-release-gates.md)
- 文書の安全なrender規約: [004: 安全なHTML、数式、コード表示](004-safe-rendering-and-presentation.md)
