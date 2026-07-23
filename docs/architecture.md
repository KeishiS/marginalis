# Marginalis アーキテクチャ

## 目的と現状

Marginalis は、研究ノートの正本である AsciiDoc、SQLite 投影、OIDC 認証、MCP、NixOS 運用を、
長期にわたって変更し続けられる構成を目標とする。API-first の再基線化は完了しており、REST と
MCP は同じアプリケーション層のユースケースに到達する。HTTP ハンドラーが SQLite・ファイル
正本・AsciiDoc 解析・OIDC 実装を直接操作することはない。

旧実装との後方互換は持たない。新しいデプロイは空のデータディレクトリから開始する。

## 全体構成

```text
HTTP REST       MCP transport       maintenance CLI
     │                 │                    │
     └────────── transport adapters ───────┘
                          │
                 marginalis-application
          commands / queries / policy / ports
                          │
 ┌────────────┬───────────┼─────────────┬───────────┐
 SQLite adapter  filesystem adapter  AsciiDoc adapter  OIDC adapter
                          │
                marginalis-service
       composition root / config / tracing / serve
```

### クレートの責務

| クレート | 責務 |
| --- | --- |
| `marginalis-domain` | ID、権限、ユーザー状態、ノートのメタデータ、エラー、純粋なポリシー |
| `marginalis-application` | 作成・更新・削除・認証・ACL・投影再構築のユースケースとポート trait |
| `marginalis-asciidoc` | AdocWeave を使うノートプロファイル、解析、投影、描画入力。DB・HTTP・認証へ依存しない |
| `marginalis-sqlite` | sqlx マイグレーション、リポジトリ実装、操作ジャーナル、検索・グラフ投影 |
| `marginalis-files` | データフォーマット v1 のマーカー、パス規則、原子的置換、リビジョンハッシュ、復旧補助 |
| `marginalis-auth-oidc` | OIDC Discovery、コード交換、ID トークン検証。セッション発行はアプリケーション層のポートを通す |
| `marginalis-server` | 設定型と Clock / Random、各アダプターをアプリケーション層のポートへ接続するサーバーアダプター。トランスポートの業務判断を持たない |
| `marginalis-service` | 実行バイナリ。設定読込、依存組立、tracing 初期化、HTTP 待受を一箇所に集める composition root |

各クレートはワークスペース内部の実装境界であり、外部公開する Rust ライブラリ API は設けない。

## 不変条件

### 責務境界

- AsciiDoc 正本、SQLite 投影、操作ジャーナルの更新は、一つのアプリケーションユースケースが
  調停する。HTTP、MCP、CLI は同じユースケースを呼び、SQL やファイル I/O へ直接アクセス
  しない。
- 外部 OIDC コールバックは `OidcAuthenticationUseCases`、Cookie セッションと root ログインは
  `WebSessionUseCases`、root 管理は `UserAdministrationUseCases` を通す。HTTP トランス
  ポートは、セッションテーブル・root 資格情報・OIDC 状態・identity ストアを直接参照しない。
- REST の JSON 境界は `marginalis-web::contract` に閉じる。OpenAPI 3.1 ドキュメントを
  `/api/v1/openapi.json` とリリース成果物に同一内容で公開する。MCP トークン、Cookie、CSRF、
  アダプター内部型をこの契約に含めない。
- root ログインと root 管理エンドポイントは `administration_router` に隔離する。現在は通常の
  ルーターへ合流させるが、専用管理オリジンや mTLS はこのルーターだけを別リスナーへ載せ替えて
  実現する。

### 認証と秘密情報

- 外部の本人同定には OIDC の `issuer` と `subject` だけを使う。メールアドレスと表示名は
  可変の属性である。
- シークレット、トークン、認可コード、`state`、`nonce`、PKCE verifier を、監査ログ・通常
  ログ・Nix ストアへ出力しない。
- root の認証成功・失敗と root 管理操作は、秘密値を含まない構造化行として SQLite に保存
  する。監査の保持期間は 365 日である。HTTP では公開せず、サーバー上で直接確認する。
- Cookie セッションを伴う変更操作では、CSRF トークン、起動時に固定した公開オリジン、
  `Sec-Fetch-Site` を同時に検証する。`X-Forwarded-*` は、この判定にも root ログインの補助
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
- root セッションは MCP クライアントの認可を作成できず、root を MCP の Bearer トークンとして
  認証しない。
- Client ID Metadata Document は、NixOS 設定で許可した HTTPS ホストからだけ取得する。取得値は
  クライアント ID の完全一致、サイズ上限、リダイレクト URI ポリシーを検証してから SQLite に
  保存する。

### データフォーマットと保守

- データフォーマットは v1 である。空ディレクトリは `FORMAT` マーカーとともに初期化し、
  マーカーのない非空ディレクトリと未知のマーカーは、起動・保守・復元入力のいずれでも明確に
  拒否する。SQLite マイグレーションは v1 内部のスキーマ改訂であり、マーカーのない旧デプロイを
  暗黙に移行しない。
- v1 は `marginalis-asciidoc` が固定する AdocWeave の公開契約とノートプロファイルを前提と
  する。正本の意味を変える契約変更はデータフォーマットのバージョンを上げ、SQLite だけの内部
  変更は v1 内のマイグレーションとして扱う。
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
では systemd の credential として渡す。

運用中の登録ポリシーは SQLite を正本とし、root API で変更する。NixOS 設定の値を既存 DB へ
再適用することはない。`backup`・`rebuild-projections`・`prune-audit` は `StorageConfig`
だけを読み、ベース URL、OIDC issuer、クライアントシークレットを必要としない。

起動順序は、設定検証、データフォーマット検証、マイグレーション、root 初期化、未完了
ジャーナルの復旧、OIDC クライアント初期化、HTTP 待受である。OIDC Discovery が一時的に失敗
しても、サービスは root 緊急ログインだけを有効にして起動する。この間の OIDC ログインは
`503` 相当の安全な失敗となり、IdP 復旧後にサービスを再起動して Discovery をやり直す。

## 初期公開の HTTP 方針

初期公開では REST API と OAuth 保護 MCP を先行させ、一般利用者向けのサーバー生成 Web UI は
提供しない。ノート一覧・閲覧・編集・ACL 管理の UI と WASM プレビューは後続段階とする。

`/acceptance` は実環境の受入確認のためだけの同一オリジン・サーバー描画フォームであり、製品
UI の公開や新しいユースケースの追加を意味しない。REST API も HTTP アダプターに留まり、同じ
アプリケーションユースケースを経由して正本・SQLite 投影・ACL を扱う。
