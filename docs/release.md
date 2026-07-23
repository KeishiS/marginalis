# リリース手順

## RC.2 の範囲

`v0.1.0-rc.2` は、研究室内で REST API と OAuth 保護 MCP を運用するための候補版です。
一般利用者向け Web UI、SMTP、招待、ユーザー再有効化、グループ ACL、専用管理オリジン・mTLS は
含みません。受入確認専用の `/acceptance` は含みますが、これは製品の Web UI ではありません。

API パスは `/api/v1`、データフォーマットは v1 です。RC.2 で公開する
[OpenAPI 契約](openapi.json)は v0.1.0 の凍結候補です。RC 期間中に破壊的変更が許されるのは、
セキュリティ、データ破損、ACL 漏洩、相互運用性に関わるリリースブロッカーの修正だけです。

## 自動ゲート

リリース候補にタグを付ける直前に、Linux 上で次を実行します。

```sh
nix develop --command cargo make release-gate
```

このゲートは、Rust のフォーマット、Clippy、ワークスペース全体のテスト、依存境界の検査、
OpenAPI 契約、GitHub Actions の構文、Nix flake、NixOS VM テスト、Linux パッケージビルドを
実行します。タグの push 時と手動起動時には、GitHub Actions の `Release gate` ワークフローも
同じゲートを実行します。

## 手動受入

自動ゲートの後、[実環境での受入確認](acceptance.md)の段階 1〜3 を順に完了します。実際の
Kanidm と MCP クライアントを使うため、OIDC シークレット、Cookie、認可コード、トークンを
CI やコマンド履歴に渡してはいけません。

受入で最低限確認する項目は次のとおりです。

1. OIDC セッションでの REST CRUD、検索、`ETag` 競合、ACL 非漏洩、物理削除。
2. 実 MCP クライアントの OAuth 認可、REST / MCP 間の可視性の一致、認可取消後のトークン失効。
3. バックアップ世代の検証、非破壊の復元候補作成、投影再構築、監査タイマー、実行中バイナリの
   OpenAPI 契約。

## タグ付けと公開

手動受入に成功し、リリースブロッカーがない場合にだけ、次を行います。

1. `Cargo.toml` 群と flake のバージョンが `0.1.0-rc.2` であること、`LICENSE-MIT`・
   `LICENSE-APACHE`・`CHANGELOG.md` が確定していることを確認する。
2. 注釈付きタグ `v0.1.0-rc.2` を作成して push する。
3. GitHub Actions の `Release gate` が成功した後、公開配布が必要な場合にだけ GitHub Release
   をプレリリースとして作成する。タグだけで候補版を運用する場合は Release の作成を省略できる。
4. NixOS 利用者には、Git 参照をタグへ固定するよう案内する。

```nix
inputs.marginalis.url = "github:KeishiS/Marginalis/v0.1.0-rc.2";
```

GitHub Release の発行と実サーバーへの適用は外部の状態を変更するため、このリポジトリからは
自動実行しません。
