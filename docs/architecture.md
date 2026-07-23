# Marginalis アーキテクチャ再設計

## 目的

Marginalisは、研究ノートの正本であるAsciiDoc、SQLite投影、OIDC、Web UI、MCPおよびNixOS
運用を長期間にわたり変更できるようにする。初期実装との後方互換は要求しない。公開API、DB
schemaおよび設定形式は、この文書の構成へ破壊的に移行する。

## 再基線化前の問題

- HTTP transportがSQLite、ファイル、AsciiDocおよびsession adapterの具体型へ直接到達すると、
  RESTとMCPが同じ認可・更新手順を共有できない。
- server binary、設定読込およびtracingの責務がHTTP transportと混在すると、NixOS moduleと
  maintenance CLIの組立が不安定になる。
- 時刻、UUID生成、乱数、SQLおよびファイルI/Oの境界が明確でなく、ユースケースを単体試験
  できない。
- SQL schemaを実行時の`CREATE TABLE IF NOT EXISTS`と個別`ALTER TABLE`で更新しており、
  schema version、再現可能なmigrationおよびupgrade testがない。
- AsciiDoc正本のファイルサービスと更新ジャーナルが未実装であり、SQLite投影だけが先行している。
- NixOS moduleが必要とする型付き設定・secret入力・実行ユーザー・永続パスの契約が、サーバー
  設定としてまだ定義されていない。

## 目標構成

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
                 marginalis-server
       composition root / config / tracing / serve
```

### crateの責務

- `marginalis-domain`: ID、権限、ユーザー状態、ノートmetadata、エラー、純粋なpolicy。
- `marginalis-application`: 作成・更新・削除・認証・ACL・投影再構築のユースケースとport trait。
- `marginalis-asciidoc`: AdocWeaveを使うノートprofile、解析、投影、描画入力。DB・HTTP・認証を
  持たない。
- `marginalis-sqlite`: sqlx migration、Repository実装、操作ジャーナル、検索・グラフ投影。
- `marginalis-files`: datadirのパス規則、原子的置換、revision hash、ファイル操作の復旧補助。
- `marginalis-auth-oidc`: OIDC Discovery・code exchange・ID Token検証。Web sessionはapplication
  portを通じて発行する。
- `marginalis-server`: 設定読込、依存組立、tracing、server起動。HTTP、MCPおよびCLIを具体adapterへ
  接続するcomposition rootであり、transportの業務判断を持たない。

初期段階ではworkspace内crateに留め、外部公開するRustライブラリAPIは設けない。

## 不変条件

- AsciiDoc正本、SQLite投影および操作ジャーナルの更新は一つのapplication use caseだけが調停する。
- HTTP、MCPおよび将来のCLIは同じuse caseを呼び、SQLやファイルI/Oへ直接アクセスしない。
- Web Cookie session、root loginおよび外部OIDC callbackは`WebAuthenticationUseCases`を通す。
  HTTP transportはsession table、root credential、OIDC stateおよびidentity storeを直接参照しない。
- OIDCの`issuer`と`subject`だけが外部本人同定に使われる。email・表示名は可変属性である。
- secret、token、authorization code、state、nonceおよびPKCE verifierを監査ログ・通常ログ・
  Nix storeへ出力しない。
- MCP access tokenはcanonical resource URI・scope・有効期限を同時に照合する。refresh tokenは一回だけ
  使用でき、交換時に同一SQLite transactionで次のtoken pairへローテーションする。
- root sessionはMCP clientの認可を作れず、root actorをMCP Bearer tokenとして認証しない。
- Client ID Metadata Documentは、NixOS設定で許可されたHTTPS hostだけから取得する。取得値はclient IDの
  完全一致、サイズ上限、redirect URI policyを検証してからSQLiteへ保存する。
- 新基線のDB schemaはversion管理し、空DB作成をCIで検証する。旧dataDirは移行せず、旧versionは
  明確に拒否する。
- 時刻はUTC epoch milliseconds、IDは型付きUUIDv7、外部入力は境界で検証する。

## 設定と起動

`ServerConfig`を一箇所で検証し、環境変数、NixOS moduleおよび将来のCLIはこの型へ変換する。
設定にはBase URL、listen address、data directory、SQLite URL、OIDC公開設定、登録ポリシー、
session期限を含める。secretは別の`SecretConfig`で受け、NixOSではsystemd credentialから渡す。

起動順は、設定検証、migration、datadir検証、root初期化、未完了ジャーナル復旧、OIDC client
初期化、HTTP listenとする。OIDC一時障害時の起動可否は、root緊急ログインと通常ログインの
可用性要件を踏まえた明示設定にする。

## 段階的な再基線化

1. 新crate構成、domain型、`ServerConfig`、Clock/Random port、sqlx migration基盤を導入する。
2. `ServerConfig`が確定した時点で、最小のNix package/moduleとhealth checkを並行して実装する。
   moduleは設定型と永続パスだけを扱い、OIDC secret注入は後続の契約へ委ねる。
3. ユーザー・ACL・session・OIDC試行を新SQLite adapterへ移す。旧schemaは移行せず、deploymentの
   dataDirを明示的に初期化する。
4. ファイル正本と操作ジャーナルを実装し、ノートCRUDをapplication use caseとして成立させる。
5. Axumを新use caseへ接続し、OIDC、Cookie、CSRF、RESTをadapterへ限定する。NixOS VM testをここで
   OIDC secret contractまで拡張する。
6. MCP OAuthを同じapplication portに追加し、Web UIとWASM previewは後続にする。

### 初期公開のHTTP方針

初期公開ではREST APIとOAuth保護されたMCPを先行し、サーバー生成Web UIは提供しない。ノート一覧、閲覧、編集および
ACL管理のUI、WASM previewは後続段階とする。REST APIもHTTP adapterに留まり、同じ
application use caseを経由して正本・SQLite投影・ACLを扱う。
