# リポジトリ文書のAsciiDoc化評価

## 目的と結論

この文書は、[Issue #21](https://github.com/KeishiS/marginalis/issues/21)に基づき、リポジトリで
保守する人間向け文書をMarkdownからAsciiDocへ変換する便益と費用を評価します。ノート本文に
AsciiDocを採用する理由は[要件定義](requirements.md)で扱うため、ここでは対象にしません。

2026-07-28時点では、リポジトリ文書をAsciiDocへ変換しません。現在使っている表、リンク、
ソースコード例はMarkdownで表現できており、形式変更によって新しく検証できる要件や再利用できる
内容を確認できませんでした。一方、変換すると既存のリンク検査を作り直し、30ファイルと
171個の文書間リンクを同時に変更する必要があります。

## 前提

評価対象は、Gitが追跡するルートと`docs/`のMarkdown文書です。GitHub Issue、OpenAPI、
プログラム、設定、ノートの`.adoc`ファイルは含みません。

2026-07-28の`main`では、対象は30ファイル、3,280行です。文書には表の行が194行、
ソースコードブロックの開始・終了行が50行あります。文書間の`.md`リンクは171個です。
AsciiDocの`include`や文書間`xref`を使うリポジトリ文書はありません。

## 具体例による比較

| 例 | Markdownでの現状 | AsciiDoc化で期待できる便益 | 評価 |
| --- | --- | --- | --- |
| `docs/traceability.md`の対応表 | 通常の表として記述し、検査用スクリプトが要件IDを確認 | 表の一部を再利用できる可能性 | 再利用先がなく、現行検査も形式に依存するため便益なし |
| `docs/development.md`のコマンド例 | 言語を指定したコードブロックとして表示 | ソースブロックの属性を細かく指定可能 | 現在必要な表示はMarkdownで充足 |
| 文書間リンク | 相対パスを使用し、`docs-check`が対象ファイルの存在を確認 | AsciiDocの`xref`による参照 | アンカーまで検査する実装が別途必要で、現行より安全にならない |
| 図 | 現在は図を使用していない | AsciiDocの図表機能 | GitHubはMermaidをMarkdownで表示できるため、形式変更の理由にならない |

この比較では、AsciiDocでなければ表現または検証できない現行文書を確認できませんでした。
将来の抽象的な再利用可能性は、30ファイルを変換する根拠に含めません。

## GitHub上の表示

GitHubはAsciiDocを文章として表示し、変更差分にも整形後の表示を提供します。したがって、
`.adoc`ファイル自体が閲覧不能になる問題はありません。根拠はGitHub公式資料の
[文章ファイルの表示方法](https://docs.github.com/en/repositories/working-with-files/using-files/working-with-non-code-files)
です。

ただし、GitHubが案内する相対リンク、見出しへのリンク、Mermaid図はMarkdownを基準に説明されて
います。特にMermaidの表示対象はMarkdownファイルと明記されています。詳しくは
[基本的な記述と書式](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax)と
[図の作成](https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/creating-diagrams)
を参照してください。GitHub上で表示できることだけでは、Markdownより保守しやすいとは判断できません。

## 検査と移行費用

現在の`cargo make docs-check`は、Gitが追跡するMarkdown文書について次を確認します。

- 行末の不要な空白を検出します。
- `.md`への相対リンクが既存ファイルを指すことを確認します。
- v0.5.0の旧Issueと移行対応表の対応を確認します。

AsciiDocへ変換する場合は、少なくとも同じ検査を`.adoc`へ実装し直す必要があります。
AdocWeaveのノート用プロファイルは、ノートの題名、属性、安全な表示規則を検証するものであり、
リポジトリ文書の相対リンクやGitHub上の見出しIDを検査する代替にはなりません。ノートと同じ
拡張子を使うだけでは検査を共通化できません。

また、一括変換では30ファイルの名前と171個のリンクを同時に変更します。内容変更と形式変換の
差分を分けて確認しにくくなり、過去のGitHub URLを参照する利用者にも影響します。段階的に変換すると、
MarkdownとAsciiDocの両方に対応する執筆規則と検査を移行期間中に維持する必要があります。

## 寄稿への影響

現在のPull Request、Issue、GitHub公式の執筆案内はMarkdownと同じ記法を使用できます。
リポジトリ文書だけをAsciiDocへ変えると、参加者は文書の配置によって二つの記法を使い分けます。
AsciiDoc固有機能を実際に使う文書がない現状では、この追加負担を上回る便益はありません。

## 決定と再評価条件

リポジトリ文書はMarkdownのまま保ち、一括変換も一部変換も行いません。新しい文書も、次の
再評価条件を満たすまではMarkdownで作成します。

次のいずれかが具体例とともに確認された場合は、影響する文書だけを対象に再評価します。

- 同じ節や表を複数の公開文書へ安全に埋め込み、重複を除く必要性
- Markdownでは表現できず、GitHubとローカル生成物の両方で必要な文書機能
- AsciiDocの構造を使うことで、現行の検査では確認できない要件を自動検証できること
- 変換対象、文書間リンク、外部の参照URLを保った移行手順と、Markdown以上の検査

再評価では、形式を先に選ばず、解決する具体的な文書上の問題と受入例を最初に固定します。
