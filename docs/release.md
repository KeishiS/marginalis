# リリース手順

`v0.1.0`（2026-07-23）で `/api/v1` の OpenAPI 契約とデータフォーマット v1 を凍結しました。
以後のリリースはこの手順に従います。過去のリリース内容は [CHANGELOG](../CHANGELOG.md) を
参照してください。

## 互換性の方針

- フィールド追加などの後方互換な変更は `/api/v1` 内で行います。
- 破壊的変更は新しいバージョンパスとして追加し、既存バージョンには少なくとも 1 リリース
  周期の非推奨告知と移行手順を用意します。
- 正本の解釈を変える変更は、SQLite マイグレーションだけでなくデータフォーマットの
  バージョンも上げます。

## 自動ゲート

タグを付ける直前に、Linux 上で次を実行します。

```sh
nix develop --command cargo make release-gate
```

このゲートは、Rust のフォーマット、Clippy、ワークスペース全体のテスト、依存境界の検査、
OpenAPI 契約、GitHub Actions の構文、Nix flake、NixOS VM テスト、Linux パッケージビルドを
実行します。タグの push 時と手動起動時には、GitHub Actions の `Release gate` ワークフローも
同じゲートを実行します。

## 手動受入

自動ゲートの後、[実環境での受入確認](acceptance.md)のうちリリース内容に該当する段階を完了
します。実際の Kanidm と MCP クライアントを使うため、OIDC シークレット、Cookie、認可コード、
トークンを CI やコマンド履歴に渡してはいけません。

公開 API・認証・データフォーマットに触れるリリースでは、最低限次を確認します。

1. OIDC セッションでの REST CRUD、検索、`ETag` 競合、ACL 非漏洩、物理削除。
2. 実 MCP クライアントの OAuth 認可、REST / MCP 間の可視性の一致、認可取消後のトークン失効。
3. バックアップ世代の検証、非破壊の復元候補作成、投影再構築、監査タイマー、実行中バイナリの
   OpenAPI 契約。

## タグ付けと公開

手動受入に成功し、既知の障害がない場合にだけ、次を行います。

1. `Cargo.toml` 群と flake のバージョンがリリース対象のバージョンと一致していること、
   `LICENSE-MIT`・`LICENSE-APACHE`・`CHANGELOG.md` が確定していることを確認する。
2. 注釈付きタグ `v<version>` を作成して push する。
3. GitHub Actions の `Release gate` が成功した後、公開配布が必要な場合にだけ GitHub Release
   を作成する。タグだけで運用する場合は Release の作成を省略できる。
4. NixOS 利用者には、Git 参照をタグへ固定するよう案内する。

```nix
inputs.marginalis.url = "github:KeishiS/Marginalis/v0.1.0";
```

GitHub Release の発行と実サーバーへの適用は外部の状態を変更するため、このリポジトリからは
自動実行しません。
