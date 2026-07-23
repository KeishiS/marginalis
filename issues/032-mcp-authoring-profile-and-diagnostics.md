# 032: MCP向けノートprofile公開と検証診断の充実

状態: RC.1 リリース後に着手。RC.1のOpenAPI/MCP契約freezeを変更しない。

MCPクライアント（AIエージェント）が、本文profileの制約を事前に知り、検証失敗時には位置付きの
診断を受け取れるようにする。現在は`tools/list`の一行説明しか手掛かりがなく、profile違反は
`-32602 "request is invalid"`という粗い失敗になるため、推測による再試行のループが発生する。

## 前提

- ノートヘッダ（`note-id`、`creator-id`、`created-at`、`updated-at`）はサーバが生成・保護し、
  MCPの`create_note`/`update_note`は`title`・`body`・`tags`の構造化入力のみを受ける。
  ヘッダ込みの雛形をMCPへ公開する経路は追加しない。RESTの生source経路と二重の意味論を作らない。
- 本文profileの正は`marginalis-asciidoc`である。許可source言語、禁止構文（include、passthrough、
  resourceマクロ、危険URL scheme）、`stem: latexmath`、`xref:note:<UUID>[label]`、タグ正規化規則を
  すべて同crateの定数・検証器が決めている。
- AdocWeaveは位置付き診断を公開APIとして提供する。`marginalis-asciidoc`の検証エラーは既に
  種別を持つ（`include-directive-disabled`等）。
- 診断は利用者自身が送信したsourceだけを対象とし、ACL・他ノート・秘密情報を含まない。
- 公開契約の変更はRC.1 freeze後とし、`/api/v1`内では後方互換なフィールド追加のみを行う。

## 設計方針

### 1. `get_note_profile` tool（read-only、scope `notes:read`）

- 機械可読JSONと短い人間可読説明を併記して返す。少なくとも次を含める。
  - 許可するAsciiDoc構文の一覧と、禁止構文の一覧（禁止理由の一行説明つき）。
  - `xref:note:<UUID>[label]`の書式、anchor付き参照の書式、および例。
  - `stem`の設定（`latexmath`）とインライン・ブロック数式の例。
  - 許可source block言語の一覧（言語未指定はプレーンテキストとして許可されること）。
  - タグの正規化規則（空白除去、カンマ・改行禁止、大文字小文字の同一視、ソート）。
  - タイトルの制約（空・改行禁止）。
- 出力値は`marginalis-asciidoc`の定数（`allowed_source_languages`等）から生成する。
  手書きの説明文を別に保持しない。profile変更時にtool出力・`docs/mcp.md`・検証器が
  同じ正から更新されることを試験で保証する。
- `tools/list`のtool説明にも「本文profileは`get_note_profile`で取得できる」旨を記載する。

### 2. 検証失敗時の位置付き診断

- application層の`NoteUseCaseError::Validation`へ構造化診断を追加する。transport非依存の型
  （例: `SourceDiagnostic { code, message, start, end }`のリスト）とし、`marginalis-asciidoc`の
  検証エラー種別とAdocWeave診断の原文範囲から構成する。
- MCPはJSON-RPC error objectの`data` fieldへ診断リストを載せる。`code`・`message`は現行の
  安定値を維持する（後方互換な追加）。
- RESTは共通エラーJSONへ後方互換な`details`配列を追加する（`code`と`message`は不変）。
  `/api/v1`内のフィールド追加として扱い、OpenAPI documentを同時に更新する。
- 診断へ含めるのは送信されたsourceに関する情報だけとする。DBキー、ACL状態、他ノートの存在、
  内部例外を含めない（既存のerror方針を維持）。
- `create_note`/`update_note`の構造化経路では、bodyの位置が文書全体の位置とずれる。診断の
  位置はクライアントが送った`body`基準へ変換するか、変換できない場合は位置なし診断として返す。
  どちらを採るかを実装時に決めて`docs/mcp.md`へ明記する。

### 3. 優先順位

診断の充実（2）を先に実装する。静的なprofile提示（1）より、失敗時に原因が特定できることの
ほうが再試行効率への効果が大きい。1は2で導入する診断種別の語彙を再利用して実装する。

## 対象外

- ヘッダ込みAsciiDoc雛形のMCP公開。
- MCP resources機能への移行（現在のcapabilitiesは`tools`のみであり、read-only toolとして提供する）。
- 検証の意味論そのものの変更。profileの許可・禁止範囲はこのissueでは変えない。

## 完了条件

- `get_note_profile`が`marginalis-asciidoc`の定数から生成した機械可読profileを返し、
  `docs/mcp.md`のtool表・説明と一致している。
- profile違反を含む`create_note`/`update_note`が、違反種別と（可能な場合）位置を含む診断を
  JSON-RPC `error.data`で返す。RESTの同経路は共通エラーJSONの`details`で同じ診断を返す。
- 診断に秘密情報・ACL情報・内部例外が含まれないことを試験で確認している。
- OpenAPI documentとMCP契約の変更が後方互換であることを契約試験で確認している。
