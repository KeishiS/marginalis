# 033: リポジトリ文書のAsciiDoc移行

状態: AdocWeave v0.5.0移行（Issue 029）完了後に着手。

## 目的

リポジトリ内で保守する人間向け文書をMarkdownからAsciiDoc（`.adoc`）へ移行し、ノート正本と同じ
文書形式・検証基盤へ揃える。形式変換だけでなく、リンク、anchor、コード例、表、NixOS運用手順および
GitHub上での閲覧性を保持する。

AdocWeave v0.5.0の契約とURL・semantic blockの挙動を先に固定しなければ、移行した文書の検証・render結果を
安定した基盤で評価できない。このためIssue 029の完了を開始条件とする。

## 対象と対象外

対象はrepositoryが保守する次のMarkdown文書である。

1. rootの`README.md`、`CHANGELOG.md`。
2. `docs/`配下の運用・仕様・受入・リリース・ロードマップ文書。
3. `issues/`および`issues/upstream/`配下のIssue・計画文書。

`docs/openapi.json`、Nix/Rust/JSON/YAML等の機械可読ファイル、license本文、GitHub Actions設定、依存先の
third-party文書は対象外とする。拡張子だけを変更する機械的な一括renameは行わない。

## 実装項目

1. 現在のMarkdownファイルを棚卸しし、変換対象、公開経路、相互参照、GitHubからの外部リンクを一覧化する。
2. 各文書を`.adoc`へ変換する。見出し、anchor、箇条書き、表、admonition、コードblock、Nix/Rust/shellの
   source language、相対リンク、`docs/openapi.json`への参照を意味を保って移す。
3. `README.adoc`、`CHANGELOG.adoc`、`docs/*.adoc`、`issues/*.adoc`へ参照先を更新し、Markdownへのリンク・
   anchor・画像参照を残さない。GitHubのblob URL、raw URLおよび外部から参照されるURLの互換性方針を決める。
4. AdocWeave v0.5.0で安全にrenderできる文書profileを定める。ノート正本に必要なmetadataやACLはrepository
   文書へ要求せず、include・passthrough・外部resourceの扱いはCIで明示的に検証する。
5. CIに文書検証を追加する。すべての`.adoc`のparse、内部xrefと相対file link、source block language、
   `README`/`CHANGELOG`/`docs`/`issues`の網羅性を検査し、壊れた参照をrelease gateで拒否する。
6. GitHub上の閲覧、clone後の`README`発見、NixOS運用手順へのリンク、Issue一覧の可読性を手動確認し、
   migration後の執筆規約を`CONTRIBUTING`相当の文書へ記す。

## 移行方針

- 原子的に切替える。MarkdownとAsciiDocを長期併存させず、切替commitでリンクをすべて`.adoc`へ更新する。
- 文書の意味を変える機会にしない。仕様・手順の変更は別Issueまたは明示的な変更記録に分離する。
- GitHubの表示互換性または既存外部リンクの維持にredirectが必要なら、repository外のHTTP redirect責務と
  Git URLの互換性を先に決定する。
- issue file名の連番と内容は保持し、履歴追跡できるrenameとして扱う。

## 完了条件

- 対象となる人間向けMarkdown文書が残っておらず、対応する`.adoc`文書と相互参照が存在する。
- README、CHANGELOG、`docs/`、`issues/`がGitHubとlocal cloneで閲覧・追跡できる。
- AdocWeave v0.5.0によるparse/renderと内部リンク検証がCIおよびrelease gateで成功する。
- 文書形式、source block、リンク、anchor、画像・外部URLの執筆規約が明文化される。
- OpenAPIなど対象外の機械可読契約と、既存のNixOS・REST・MCP手順への参照が失われない。

## 依存関係

- 開始前提: [029: AdocWeave v0.5.0への移行](029-adocweave-v0.5.0-migration.md)
- CI統合: [021: 試験アーキテクチャとrelease gate](021-test-architecture-and-release-gates.md)
- 文書の安全なrender規約: [004: 安全なHTML、数式、コード表示](004-safe-rendering-and-presentation.md)
