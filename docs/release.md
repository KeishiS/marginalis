# リリース手順

## RC.1 の範囲

`v0.1.0-rc.1`は、研究室内で REST API と OAuth 保護 MCP を運用する最初の候補版である。
通常利用者向けWeb UI、SMTP、招待、ユーザー再有効化、グループ ACL、専用管理 origin・mTLSは含めない。
実環境受入専用の`/acceptance`は含むが、製品のWeb UIではない。

API pathは`/api/v1`、data formatはv1である。RC.1で公開する[OpenAPI contract](openapi.json)は
v0.1.0のfreeze候補とする。RC期間中は、security、データ破損、ACL漏洩、相互運用性のrelease blockerだけを
破壊的変更の理由として扱う。

## 自動 gate

release candidateをtag付けする直前に、Linux上で次を実行する。

```sh
nix develop --command cargo make release-gate
```

このgateはRust format、Clippy、workspace test、依存境界、OpenAPI contract、GitHub Actions構文、Nix flake、
NixOS VM testおよびLinux package buildを実行する。tag push時または手動起動時には、GitHub Actionsの
`Release gate` workflowも同じgateを実行する。

## 手動受入

自動gateの後、[実環境受入確認](acceptance.md)の段階1から3を順に完了する。実 Kanidm と実 MCP clientを
用いるため、OIDC secret、Cookie、authorization codeおよびtokenをCIやコマンド履歴へ渡してはならない。

受入で確認する最低条件は次である。

1. OIDC sessionでのREST CRUD、検索、ETag競合、ACL非漏洩および物理削除。
2. 実MCP clientのOAuth認可、REST/MCP間の可視性一致、認可取消後のtoken失効。
3. backup generationの検証、非破壊restore候補、投影再構築、監査timerおよび実行中OpenAPI contract。

## tagと公開

手動受入が成功し、release blockerがない場合だけ次を行う。

1. `Cargo.toml`群とflakeのversionが`0.1.0-rc.1`であること、`LICENSE-MIT`、`LICENSE-APACHE`および
   `CHANGELOG.md`が確定していることを確認する。
2. annotated tag `v0.1.0-rc.1`を作成してpushする。
3. GitHub Actionsの`Release gate`成功後、GitHub Releaseをprereleaseとして作成する。
4. NixOS利用者にはGit refをtagへ固定するよう案内する。

```nix
inputs.marginalis.url = "github:KeishiS/Marginalis/v0.1.0-rc.1";
```

GitHub Releaseの発行と実サーバへの適用は外部状態を変更するため、このrepositoryから自動実行しない。
