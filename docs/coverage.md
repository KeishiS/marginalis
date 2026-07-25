# 本番到達性とカバレッジ

## 実行

Nix開発環境から次の一commandで計測します。

```sh
nix develop --command cargo make coverage
```

`target/coverage/`へ次の二組を生成し、同じsummaryを標準出力にも表示します。

- `workspace.{json,summary.txt}`: workspaceのunit・integration testを実行した結果
- `v0.3-integration-path.{json,summary.txt}`: 実行バイナリと現行HTTP・OIDC・MCP結合試験の結果

`tests/`以下の外部test sourceと`marginalis-integration-tests` crateはreportの分母から除外します。
`#[cfg(test)]`をproduction module末尾へ置くRustのunit testは同じsource fileとしてLLVMに
instrumentされるため、file全体の数値にはtest bodyも含まれます。この数値を「未使用コード率」や
厳密な「production code coverage」とは呼びません。未実行箇所をHTMLまたはJSONで確認し、
本番到達性、試験不足、意図的なerror pathの順に分類します。

## 本番到達性

coverageは、本番から到達不能なコードの検出器ではありません。次の静的検査を別に実行します。

```sh
nix develop --command cargo make production-reachability
```

この検査は`marginalis-service`が依存するworkspace crateをallowlistと照合し、旧公開API、
ローカル管理者、ファイル正本、所属定期監視に由来する禁止symbolの復帰を拒否します。
`cargo make verify`にも含まれます。

## CI方針

coverageは通常の`verify`と独立した`coverage` jobで実行します。初期baselineでは一律の
合格率を設定せず、summaryはCI logだけへ出力しartifactとして保存しません。秘密値、token、
Cookie、実利用者データを試験fixtureやreportへ含めてはいけません。
