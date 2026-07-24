# Marginalis アーキテクチャ

## 目的

Marginalis は、AsciiDoc の正本、SQLite の投影、OIDC 認証、MCP、NixOS による運用を、それぞれ
独立して変更できる構成を採用する。REST と MCP は同じアプリケーション層の操作を呼び出す。
保守コマンドは、投影再構築では同じ操作を再利用し、バックアップと復元では検証済みの
ストレージアダプターを組み立てる。HTTP ハンドラーは、SQLite、ファイル、AsciiDoc の解析器、
OIDC クライアントを直接操作しない。

この文書は現在の責務分担と、変更時にも維持する不変条件を示す。旧実装からの移行経緯は
[変更履歴](../CHANGELOG.md)、今後の変更は[ロードマップ](roadmap.md)を参照する。

## 全体構成

```text
REST API        MCP             保守コマンド
     │                 │                    │
     └────────── 入出力アダプター ─────────┘
                          │
                 marginalis-application
                 操作 / 問い合わせ / ポリシー
                          │
 ┌────────────┬───────────┼─────────────┬───────────┐
 SQLite         ファイル           AsciiDoc        OIDC
 アダプター     アダプター         アダプター      アダプター
                          │
                marginalis-service
             設定 / 依存関係の組立 / HTTP 待受
```

### クレートの責務

| クレート | 責務 |
| --- | --- |
| `marginalis-domain` | ID、権限、ユーザー状態、ノートのメタデータ、エラー、純粋なポリシー |
| `marginalis-application` | 作成・更新・削除・認証・ACL・投影再構築の操作と、外部機能を抽象化するポート trait |
| `marginalis-asciidoc` | AdocWeave を使うノートプロファイル、解析、投影、描画入力。DB・HTTP・認証へ依存しない |
| `marginalis-sqlite` | sqlx マイグレーション、リポジトリ実装、操作ジャーナル、検索・グラフ投影 |
| `marginalis-files` | データフォーマット v1 のマーカー、パス規則、原子的置換、リビジョンハッシュ、復旧補助 |
| `marginalis-auth-oidc` | OIDC Discovery、コード交換、ID トークン検証。セッション発行はアプリケーション層のポートを通す |
| `marginalis-mcp` | MCP ツールの入出力契約と、アプリケーション層への接続 |
| `marginalis-server` | 設定型と `Clock`・`Random`、各アダプターをアプリケーション層のポートへ接続するサーバーアダプター。HTTP 固有でない業務判断を持たない |
| `marginalis-web` | REST、認証、OAuth、受入確認画面の HTTP ルーターと OpenAPI 契約 |
| `marginalis-service` | 実行バイナリ。設定の読込、依存関係の組立、tracing の初期化、HTTP の待受と保守コマンドを一箇所で行う |
| `marginalis-integration-tests` | 実アダプターを組み合わせた認証、REST、MCP の結合テスト |

各クレートはワークスペース内部の実装境界であり、外部公開する Rust ライブラリ API は設けない。

## 不変条件

### 責務境界

- 通常のノート操作における AsciiDoc 正本、SQLite 投影、操作ジャーナルの更新は、一つの
  アプリケーションユースケースが調停する。REST、MCP、投影再構築は同じ規則を再利用し、
  通信方式ごとの層へ業務判断を複製しない。バックアップと復元はサービス層からストレージ
  アダプターを直接組み立てるが、正本プロファイルと保存形式の検証を省略しない。
- 外部 OIDC コールバックは `OidcAuthenticationUseCases`、Cookie セッションと root ログインは
  `WebSessionUseCases`、`root` 管理は `UserAdministrationUseCases` を通す。HTTP
  アダプターは、セッションテーブル、`root` 資格情報、OIDC 状態、利用者情報の保存先を直接
  参照しない。
- REST の JSON 境界は `marginalis-web::contract` に閉じる。OpenAPI 3.1 ドキュメントを
  `/api/v1/openapi.json` とリリース成果物に同一内容で公開する。Cookie セッションと CSRF
  ヘッダーの security scheme は契約に含めるが、Cookie・トークンの内部表現とアダプター内部型は
  含めない。
- `root` ログインと `root` 管理エンドポイントは `administration_router` に隔離する。現在は通常の
  ルーターへ合流させるが、専用管理オリジンや mTLS はこのルーターだけを別リスナーへ載せ替えて
  実現する。

### 認証と秘密情報

- 外部の本人同定には OIDC の `issuer` と `subject` だけを使う。メールアドレスと表示名は
  可変の属性である。
- シークレット、トークン、認可コード、`state`、`nonce`、PKCE verifier を、監査ログ・通常
  ログ・Nix ストアへ出力しない。
- `root` の認証成功・失敗と `root` 管理操作は、秘密値を含まない構造化データとして SQLite に保存
  する。監査の保持期間は 365 日である。HTTP では公開せず、サーバー上で直接確認する。
- Cookie セッションを伴う変更操作では、CSRF トークン、起動時に固定した公開オリジン、
  `Sec-Fetch-Site` を同時に検証する。`X-Forwarded-*` は、この判定にも `root` ログインの補助
  レート制限にも使わない。
- HTTP リクエストごとにサーバー生成の UUIDv7 を `X-Request-Id` として返し、同じ値を tracing
  スパンに記録する。クライアントが送った相関 ID は採用しない。

### MCP と OAuth

- MCP アクセストークンは、正規のリソース URI・スコープ・有効期限を同時に照合する。利用時刻
  だけを記録し、トークン値やハッシュを API・ログへ出さない。
- リフレッシュトークンは一回だけ使用でき、交換時に同一 SQLite トランザクションで次の
  トークンペアへローテーションする。
- MCP の参照一覧は、参照元と参照先の両方を閲覧できる場合にだけ返す。閲覧できない参照先の
  タイトル・アンカー・投影上の存在を返さない。
- `root` セッションは MCP クライアントの認可を作成できず、`root` を MCP の Bearer トークンとして
  認証しない。
- Client ID Metadata Document は、NixOS 設定で許可した HTTPS ホストからだけ取得する。取得値は
  クライアント ID の完全一致、サイズ上限、リダイレクト URI ポリシーを検証してから SQLite に
  保存する。

### データフォーマットと保守

- データフォーマットの識別子は v1 であり、AdocWeave v0.6.1 と現在のノートプロファイルを
  前提として破壊的に再定義した。空ディレクトリは `FORMAT` マーカーとともに初期化し、
  マーカーのない非空ディレクトリと未知のマーカーは、起動・保守・復元入力のいずれでも明確に
  拒否する。移行前の v1 は互換対象ではなく、移行・復元・切戻しの経路を提供しない。
  運用者はサービス停止後に対象 `dataDir` と、そこから分離して設定した SQLite を完全に削除し、
  空の v1 として初期化する。
- アプリケーションと NixOS モジュールは既存 `dataDir` を自動削除しない。将来、正本の意味を
  再び変える場合に版を上げるか破壊的に再定義するかは、その変更の Issue で明示する。
- `rebuild-projections` は、全 AsciiDoc 正本を検証してから、一つの SQLite トランザクションで
  検索・アンカー・参照投影を置き換える。検証に失敗した場合は最後に成功した投影を保持し、
  既存の ACL を維持する。
- `backup` は、HTTP サービス停止中に SQLite をチェックポイントしてバックアップファイルへ
  出力し、検証済みの AsciiDoc 正本だけを同じ出力ディレクトリへ複製する。`FORMAT`・
  `MANIFEST`・`COMPLETE` が揃い、マニフェストの SHA-256 が SQLite と各正本に一致する一組
  だけを復元候補とする。既存のバックアップは上書きしない。
- `restore` は、フォーマットマーカー、マニフェストのハッシュ、バックアップ SQLite の
  `integrity_check`、全正本を検証してから、既存のデータディレクトリを変更せずに新しい候補を
  作る。実際の切り替えと旧データの削除は運用者が明示して行う。
- 時刻は UTC のエポックミリ秒、ID は型付き UUIDv7 とし、外部入力は境界で検証する。

## 設定と起動

HTTP サービスは `ServerConfig` を一箇所で検証し、`HttpConfig`（ベース URL・待受アドレス）、
`StorageConfig`（データディレクトリ・SQLite URL・新規 DB の登録ポリシー）、`OidcConfig`
（issuer・クライアント ID）へ分離する。シークレットは別の `SecretConfig` で受け取り、NixOS
では systemd credential として渡す。

運用中の登録ポリシーは SQLite を正本とし、`root` API で変更する。NixOS 設定の値を既存 DB へ
再適用することはない。`backup`・`rebuild-projections`・`prune-audit` は `StorageConfig`
だけを読み、ベース URL、OIDC issuer、クライアントシークレットを必要としない。`restore` は
復元元と新しい出力先をコマンド引数で受け取り、稼働中の設定を読み込まない。

起動順序は、設定検証、データフォーマット検証、マイグレーション、未完了ジャーナルの復旧、
`root` 初期化、OIDC クライアント初期化、HTTP 待受である。OIDC Discovery が一時的に失敗
しても、サービスは `root` の緊急ログインだけを有効にして起動する。この間の OIDC ログインは
`503` で拒否する。IdP の復旧後にサービスを再起動し、Discovery をやり直す。

## 公開インターフェース

現在は REST API と OAuth で保護した MCP を提供し、一般利用者向け Web UI は提供しない。
ノート一覧、閲覧、編集、ACL 管理の UI と WASM プレビューは将来の機能である。

`/acceptance` は実環境の受入確認のためだけの同一オリジン・サーバー描画フォームであり、製品
UI の公開や新しいアプリケーション操作の追加を意味しない。REST API も HTTP アダプターに
留まり、同じアプリケーション層を経由して正本、SQLite 投影、ACL を扱う。
