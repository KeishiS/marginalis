# 043: 本番到達性とテストカバレッジの可視化

## 状態

実装完了。Nixで固定した`cargo-llvm-cov`からworkspaceと現行結合経路を別々に出力し、
production dependency graphと禁止symbolは`cargo make verify`で静的に検査する。
初回計測で判明したREST noteと閲覧UIの試験不足は、OIDC loginからMCP作成、REST更新・
削除・復元、HTML閲覧までの結合試験を追加して解消した。

## 背景

RustはLLVMのsource-based code coverageにより、実行されたline、region、functionを測定できる。
しかし、coverageが示すのは「計測対象の実行で通らなかったコード」であり、「本番から到達不能な
コード」ではない。特に旧v0.2実装を旧テストが実行すると、workspace全体のcoverageは高くても、
本番composition rootが使わないコードが大量に残り得る。

したがって、次の二つを別の証拠として管理する。

1. **本番到達性**: `marginalis-service`から依存する現行v0.3の型、route、adapterを静的に棚卸しし、
   `/api/v1`、ローカル`root`、ファイル正本、旧SQLite adapterなどの禁止した実装がproduction
   dependency graphへ残らないことを確認する。
2. **テストカバレッジ**: v0.3のunit・integration testで実行したsource regionを測り、未実行箇所を
   試験追加または不要コード削除の調査対象にする。

coverageの未実行率を、そのまま未使用コード率とは呼ばない。必要なら
「v0.3試験で未実行のproduction region率」と明記する。

## 実装方針

1. Rust toolchainと互換の`cargo-llvm-cov`および`llvm-tools`をNix開発環境へversion固定して追加する。
   CI中に未固定の最新版をdownloadしない。
2. `cargo make coverage`で再現可能なtext summaryと機械可読JSONを生成する。vendor、生成物、
   外部test sourceと結合試験専用crateはreportから除外する。同一source fileの
   `#[cfg(test)]` unit testはLLVM report上で分離できないため、数値を厳密なproduction code
   coverageとは呼ばず、配置を計測都合で崩さない。
3. 次のreportを分離する。
   - v0.3 unit・integration test全体のcrate別line/region coverage。
   - `marginalis-service`の現行composition rootとv0.3 HTTP/OIDC/MCP結合経路に限定したcoverage。
4. `cargo tree`、`cargo metadata`、明示的な禁止symbol・route検査を組み合わせ、本番到達性をrelease
   gateで検証する。coverageだけを旧実装削除の証明にしない。
5. 最初のbaselineでは一律の合格率を設けない。未実行箇所を分類し、不要コードを削除した後、
   security境界と業務規則からcrate別の下限を決める。下限は低下を防ぐ方向にだけ更新する。
6. reportへ秘密値、OIDC token、Cookie、実利用者データを含めない。CI artifactを保存する場合は
   保存期間と公開範囲を明示する。

## 初回baseline

2026-07-25に`nix develop --command cargo make coverage`を11.36秒で実行した。数値は
line / region coverage（%）であり、`#[cfg(test)]`を同じsource fileに置くunit test本体を含む。

| crate | workspace | 現行結合経路 |
| --- | ---: | ---: |
| `marginalis-application` | 0.0 / 0.0 | 対象外 |
| `marginalis-asciidoc` | 70.1 / 74.3 | 40.1 / 44.2 |
| `marginalis-auth-oidc` | 88.2 / 84.8 | 86.8 / 81.8 |
| `marginalis-domain` | 78.7 / 80.3 | 67.5 / 68.1 |
| `marginalis-server` | 84.1 / 79.1 | 64.2 / 54.6 |
| `marginalis-service` | 23.1 / 21.1 | 23.1 / 21.1 |
| `marginalis-sqlite` | 95.7 / 89.7 | 68.2 / 59.9 |
| `marginalis-web` | 80.1 / 81.4 | 67.4 / 63.7 |
| **全体** | **82.0 / 78.8** | **63.2 / 56.9** |

未実行箇所は次のように分類した。

- **試験不足**: REST note handlerと閲覧UIは初回の現行結合経路でそれぞれline 16.3%と0%だった。
  結合試験追加後は83.0%と91.7%になった。残る認証失敗・競合分岐はunit testとSQLite testで
  業務規則を確認し、意味のない網羅率目的の試験は追加しない。
- **本番到達不能**: 旧公開API、ローカル管理者、ファイル正本、所属定期監視の実装は検出されなかった。
  `marginalis-service`から推移的に到達するworkspace crateをallowlistと照合する。
- **意図的な未実行経路**: `marginalis-service`のlisten、実Kanidm discovery、保守command、
  filesystem失敗はprocess・実環境境界である。NixOS VM試験と公開前の手動受入で確認し、
  unit coverageの閾値には使わない。`marginalis-application`の0%はport宣言だけを持つためである。

coverageは独立したCI jobで約11秒を要する。summaryはCI logへ表示するだけでartifactは保存せず、
初回baselineには合格率を設定しない。

## 完了条件

- Nix環境内の単一commandで、同じrevisionから同じ対象範囲のcoverage reportを生成できる。
- 全workspace coverageとv0.3 production-path coverageを混同しない名称・文書・CI表示になっている。
- crate別のline/region baselineと、未実行箇所の「試験不足」「本番到達不能」「意図的なerror path」
  の分類結果を記録する。
- `/api/v1`、ローカル`root`、ファイル正本、旧SQLite adapterが現行production graphに復帰すると
  自動検査が失敗する。
- coverage率だけを理由に意味のないtest追加、到達不能コードの温存、測定除外を行わない。
- 実行時間を測定し、通常CIへ含めるか独立jobにするかを決定する。release gateを不安定にしない。

## 参考

- [rustc: Instrumentation-based Code Coverage](https://doc.rust-lang.org/rustc/instrument-coverage.html)
- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov)
