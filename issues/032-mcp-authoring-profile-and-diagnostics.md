# 032: MCP向けの入力規則と検証結果

## 状態

未着手。Issue 030、029の後に実装する。公開済みのOpenAPIとMCPの互換性を保ち、
既存フィールドの意味は変更しない。

## 背景

MCPクライアントは、現在の`tools/list`にある一行の説明だけでは本文の入力規則を判断できない。
規則に違反すると`-32602 "request is invalid"`だけが返り、違反箇所も分からない。そのため、
AIエージェントが原因を推測して再試行する必要がある。

## 目的

MCPクライアントが入力前にノート本文の規則を取得できるようにする。検証に失敗した場合は、
違反の種類と、取得できる場合は本文中の位置を返す。

## 決定事項

- ノートヘッダ（`note-id`、`creator-id`、`created-at`、`updated-at`）はサーバが生成・保護し、
  MCPの`create_note`/`update_note`は`title`・`body`・`tags`の構造化入力のみを受ける。
  ヘッダ込みの雛形をMCPへ公開する経路は追加しない。RESTの生source経路と二重の意味論を作らない。
- 本文の入力規則は`marginalis-asciidoc`で定義する。許可するソース言語、禁止構文（include、passthrough、
  外部ファイルを参照するマクロ、危険なURL scheme）、`stem: latexmath`、
  `xref:note:<UUID>[label]`、タグ正規化規則を
  すべて同crateの定数・検証器が決めている。
- AdocWeaveは位置付き診断を公開APIとして提供する。`marginalis-asciidoc`の検証エラーは既に
  種別を持つ（`include-directive-disabled`等）。
- 検証結果は利用者自身が送信した本文だけを対象とする。ACL、他のノート、秘密情報は含めない。
- `/api/v1`には後方互換なフィールドだけを追加する。

## 作業内容

### 1. `get_note_profile`を追加する

`get_note_profile`は読み取り専用とし、`notes:read` scopeを要求する。

- 機械可読なJSONと短い説明を返す。少なくとも次を含める。
  - 許可するAsciiDoc構文の一覧と、禁止構文の一覧（禁止理由の一行説明つき）。
  - `xref:note:<UUID>[label]`の書式、anchor付き参照の書式、および例。
  - `stem`の設定（`latexmath`）とインライン・ブロック数式の例。
  - 許可するソースブロック言語の一覧（言語未指定はプレーンテキストとして許可されること）。
  - タグの正規化規則（空白除去、カンマ・改行禁止、大文字小文字の同一視、ソート）。
  - タイトルの制約（空・改行禁止）。
- 出力値は`marginalis-asciidoc`の定数（`allowed_source_languages`等）から生成する。
  同じ情報を別の手書きデータとして保持しない。入力規則を変更した場合に、
  ツール出力、`docs/mcp.md`、検証処理が一致することを試験する。
- `tools/list`には、本文の入力規則を`get_note_profile`で取得できることを記載する。

### 2. 検証結果に位置情報を加える

- application層の`NoteUseCaseError::Validation`へ構造化した検証結果を追加する。通信方式に依存しない型
  （例: `SourceDiagnostic { code, message, start, end }`のリスト）とし、`marginalis-asciidoc`の
  検証エラー種別とAdocWeave診断の原文範囲から構成する。
- MCPはJSON-RPC error objectの`data`フィールドへ検証結果の一覧を載せる。`code`と`message`は現行の
  安定値を維持する（後方互換な追加）。
- RESTは共通エラーJSONへ後方互換な`details`配列を追加する。`code`と`message`は変更しない。
  OpenAPI文書も同時に更新する。
- 検証結果には送信された本文に関する情報だけを含める。DBキー、ACL状態、他ノートの存在、
  内部例外を含めない（既存のerror方針を維持）。
- `create_note`/`update_note`の構造化経路では、bodyの位置が文書全体の位置とずれる。診断の
  位置はクライアントが送った`body`基準へ変換するか、変換できない場合は位置なし診断として返す。
  どちらを採るかを実装時に決めて`docs/mcp.md`へ明記する。

### 3. 実装順

位置付きの検証結果（2）を先に実装する。失敗原因を特定できることは、入力規則を静的に
表示することよりも再試行の削減に直結する。`get_note_profile`（1）では、2で導入する
検証コードと用語を再利用する。

## 対象外

- ヘッダ込みAsciiDoc雛形のMCP公開。
- MCP resources機能への移行。現在は`tools`だけを提供するため、読み取り専用ツールとして追加する。
- 検証規則そのものの変更。許可・禁止する構文はこのIssueでは変えない。

## 完了条件

- `get_note_profile`が`marginalis-asciidoc`の定数から生成した機械可読な入力規則を返し、
  `docs/mcp.md`のtool表・説明と一致している。
- 入力規則への違反を含む`create_note`または`update_note`が、違反種別と可能な場合は位置を含む診断を
  JSON-RPC `error.data`で返す。RESTの同経路は共通エラーJSONの`details`で同じ診断を返す。
- 診断に秘密情報・ACL情報・内部例外が含まれないことを試験で確認している。
- OpenAPI documentとMCP契約の変更が後方互換であることを契約試験で確認している。
