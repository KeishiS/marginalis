# 015: APIを中心としたアーキテクチャの再設計

状態: 再設計は完了。実OIDC/MCPサービスを用いるNixOS VM統合試験は
[Issue 030](030-end-to-end-test-automation-readiness.md)へ移管した。

RESTによるノートの作成・取得・更新・削除・検索と、MCPツールを同じ業務処理へ接続するため、
HTTP中心だった構成を再設計する。この作業では、公開HTTP API、SQLiteスキーマ、設定形式、
Rustクレートの後方互換性を維持しない。

旧環境のAsciiDoc正本とSQLiteは移行対象にしない。ただし、`dataDir`を起動時に自動削除しては
ならない。データを破棄する場合は、運用者がサービスを停止してから明示的に実行する。

## 移行前の問題

- `marginalis-web`がAxumハンドラーからSQLite、ファイル、AsciiDoc解析器、OIDCアダプター、
  `NoteWriteService`を直接呼ぶ。RESTとMCPで同じ処理を再利用できない。
- 実行バイナリの組立、設定の読込、tracingの初期化が`marginalis-web`にある。一方、
  `marginalis-server`は設定と`Clock`・`Random`だけを持ち、クレート名と責務が逆転している。
- ノート更新だけがユースケースとして分離されている。閲覧、一覧、検索、認証、セッション、
  `root`管理は、入出力層からアダプターへ直接到達する。
- OIDC登録ポリシーは`RegistrationPolicy::default()`へ固定され、設定・永続化・管理APIの境界がない。
- HTTPのエラー変換、CSRF、監査、レート制限、要求IDは個別のハンドラーに分散している。
- SQLiteに検索用データがなく、RESTとMCPが非公開情報を漏らさないことを共通の契約として
  検証できない。

## 目標構成

```text
REST API         MCP                 保守コマンド
     │                 │                     │
     └────────────── 入出力アダプター ───────┘
                           │
                   marginalis-application
                操作 / 問い合わせ / 規則 / ポート
                           │
      ┌──────────────┬─────┴─────┬──────────────┐
      SQLite          ファイル    AsciiDoc       OIDC
      アダプター      アダプター  アダプター     アダプター
                           │
                    marginalis-server
              設定 / 依存関係の組立 / HTTP待受
```

`marginalis-application`は、入出力方式、sqlx、ファイルシステム、AdocWeave、OIDCライブラリへ
依存しない。各入出力アダプターは型付きの操作だけを呼び、具体的なアダプターや
データベース接続プールを保持しない。

## 決定事項

1. **アプリケーションAPIを導入する。** `NoteCommands`、`NoteQueries`、`IdentityCommands`、
   `SessionCommands`、`AdministrationCommands`を定義する。要求・応答の型とドメインエラーを
   この層に置き、HTTPとMCPはこのAPIだけに依存する。
2. **読み取りと書き込みを分離する。** 正本を更新する操作と、SQLiteの投影を返す問い合わせを
   分ける。検索は`SearchNotes`へ集約し、ACLによる絞り込み、カーソル、順位を契約に含める。
   初期の検索結果はノートIDと題名だけとし、本文や一致箇所の抜粋は返さない。
3. **認証済み主体を統一する。** Web Cookie、MCPアクセストークン、将来のCLIトークンは、
   検証後に`Principal`へ変換する。`Principal`は内部の利用者ID、権限、認証方式、
   セッションまたはトークンの識別子を持つ。`root`はMCPの`Principal`に変換しない。
4. **設定と起動処理をサービスへ集約する。** 実行バイナリを`marginalis-service`へ移す。
   `marginalis-web`はHTTPアダプターに限定する。登録ポリシー、セッション有効期間、
   API・MCPの待受先と公開URLは`ServerConfig`で扱い、NixOSモジュールから設定できるようにする。
5. **SQLiteスキーマを検索用データ中心に再設計する。** 利用者、ACL、セッション、監査の表と、
   ノート、アンカー、参照、検索用データの表を分ける。検索用データは正本のリビジョンと
   投影の版を持ち、正本から再構築できるようにする。
6. **入出力方式に共通する処理を集約する。** エラー変換、要求ID、tracing、認証失敗時の
   秘密情報除去、CSRF、レート制限、監査をミドルウェアまたはアプリケーション層へ集める。
   CSRFはHTTPにだけ適用する。

## 実施内容

1. アプリケーション層の要求・応答、ドメインエラー、ポートtraitを定義した。HTTPの状態から
   具体的なアダプターを除き、メモリ上の代替実装でユースケースを単体試験できるようにした。
2. SQLite、ファイル、AsciiDocの各アダプターを新しいポートへ実装した。ノートの作成、取得、
   更新、削除を新しいアプリケーションAPIへ移した。操作記録と復旧はノート操作に閉じた。
3. OIDC、`root`ログイン、セッション、登録ポリシー、利用者状態を認証・セッション用APIへ移した。
   無効化した利用者のセッションとトークンを失効させ、監査を同じトランザクションへ含めた。
4. HTTPアダプターを新しいアプリケーションAPIだけで書き直し、Issue 014のREST設計へ移行した。
5. 検索用データと`SearchNotes`を導入してからMCPアダプターを接続した。
6. 実行バイナリ、NixOSモジュール、VM試験、運用コマンドを新しい構成へ移した。

## 初期化方針

- この再設計では旧`dataDir`を移行しない。新しいSQLiteスキーマ、`root`資格情報、
  OIDC利用者情報、ACL、セッション、監査、AsciiDoc正本を空の状態から作成する。
- NixOSの運用手順は、サービス停止、対象`dataDir`のバックアップまたは削除、空ディレクトリの
  所有者設定、設定の適用、`root`初期化、OIDCログイン確認の順とする。
- アプリケーションとNixOSモジュールは、通常の起動や再構築で既存の`dataDir`を削除しない。
  初期化は運用者が明示的に選んだ場合だけ実行する。
- 新しいスキーマには旧マイグレーションを引き継がない。旧データベースを指定した場合は、
  明確なバージョン不一致エラーで停止する。

## 完了条件

- HTTPとMCPがSQLite、ファイル、AsciiDoc、OIDCの具体的なアダプターを直接参照しない。
- RESTとMCPが、同じアプリケーション層の操作とACL規則を使う。
- 空の`dataDir`から、`root`初期化、OIDCログイン、REST、検索、MCPまでを一貫して検証する。
- サーバーの組立、NixOS設定、tracing、保守コマンドの責務が一箇所にある。

## 実施記録（2026-07-23）

- RESTとMCPは`NoteUseCases`を共有する。HTTPハンドラーはSQLite、ファイル、AsciiDocの
  具体的なアダプターを参照しない。
- 実行バイナリを`marginalis-service`へ分離し、設定の読込、アダプターの組立、tracingの初期化、
  HTTPの待受を集約した。
- 要求ID、CookieのCSRF対策、MCPのレート制限はHTTP層に置いた。利用者登録ポリシーは
  SQLiteへ永続化した。
- `root`管理の監査はSQLiteへ永続化した。`marginalis rebuild-projections`、
  `marginalis backup`、`marginalis restore`と対応するNixOSのoneshot unitを実装した。
  これらにより、投影の再構築、SQLiteと正本のバックアップ、検証済みの復元候補を作成できる。
- VM試験では、資格情報の注入、永続ディレクトリ、サービスの再起動、oneshot unitとの
  排他制御を確認した。
- NixOS VM上で実際のOIDCプロバイダーとMCPクライアントを使うE2E試験はIssue 030へ移管した。
