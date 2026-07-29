# CodeMirrorをAsciiDoc編集基盤に採用

- 状態: 採用
- 日付: 2026-07-29

## 背景

Marginalisのノートは、題名、文書属性、本文を含む完全なAsciiDoc文書を正本とします。長文編集には
行番号、検索、編集履歴、複数行の字下げ、日本語IME、診断位置への移動が必要です。一方、ブラウザー
側でAsciiDocを別の文書モデルへ変換すると、未対応の記法や文書属性を失う可能性があります。

## 決定

Web UIのAsciiDoc入力にCodeMirror 6を採用します。依存は`@codemirror/state`、
`@codemirror/view`、`@codemirror/commands`、`@codemirror/search`の必要な部分に限定します。
CodeMirrorとの接続は`AsciiDocEditor.tsx`へ閉じ込め、Reactのフォーム状態とは完全な文字列だけを
受け渡します。

AsciiDocの解析、診断、HTML生成はサーバー側のAdocWeaveを唯一の実装とします。入力補助は
AsciiDocを構造化された別形式へ変換せず、一回の文字列置換としてCodeMirrorの編集履歴へ加えます。

## 代替案

| 選択肢 | 評価 |
| --- | --- |
| `textarea` | 配布容量は小さいものの、正確な行番号、検索、複数行編集、診断位置への移動を個別に実装する必要があるため不採用 |
| Monaco Editor | 高機能ですが、公式にモバイルブラウザーを対応対象としておらず、320px幅の要件に合わないため不採用 |
| Tiptap / ProseMirror | 構造化されたリッチテキスト編集には適していますが、任意のAsciiDocソースを無損失で往復させる保証を作りにくいため不採用 |
| CodeMirror 6 | 文字列と選択範囲を正本にしたまま、必要な長文編集機能を組み合わせられるため採用 |

比較には[CodeMirrorのシステムガイド](https://codemirror.net/docs/guide/)、
[Tiptapの文書モデル](https://tiptap.dev/docs/editor/core-concepts/schema)、
[Monaco Editorの対応ブラウザー](https://github.com/microsoft/monaco-editor#browser-support)を
使用しました。

## 結果

導入前のproduction用`editor.js`は224,269バイト、gzip圧縮後は約69.72 kBでした。導入時は
545,570バイト、gzip圧縮後は約174.34 kBとなり、それぞれ321,301バイト、約104.62 kB増えます。
現在のRustサーバーはReact画面を一つの`editor.js`として配信するため、この増加は編集画面以外にも
影響します。必要な編集機能と引き換えにこの増加を受け入れ、600,000バイトを継続検査の上限とします。

CodeMirrorの公式AsciiDoc構文拡張は採用しません。初期実装ではプレーンテキストとして扱い、
不完全な構文色分けとAdocWeaveの解析結果が食い違う状態を避けます。

## 再検討条件

- `editor.js`が600,000バイトを超える場合
- 低速な通信環境で編集画面以外の初期表示に無視できない遅延が確認された場合
- CodeMirrorの保守終了またはアクセシビリティー要件を満たせない問題が確認された場合
- AsciiDocを無損失で往復できる、保守された専用編集基盤が利用可能になった場合
