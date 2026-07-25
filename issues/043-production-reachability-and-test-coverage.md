# 043: 本番到達性とテストカバレッジの可視化

## 状態

未着手。[037](037-v0.3.0-architecture-rebaseline.md)の破壊的切替後に残る旧実装を除去し、
現行v0.3経路の試験不足を継続的に把握するための横断作業とする。

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
2. `cargo make coverage`で再現可能なtext summaryと機械可読JSONまたはLCOVを生成する。
   vendor、生成物、test bodyはproduction coverageの分母に含めない。
3. 次のreportを分離する。
   - v0.3 unit・integration test全体のcrate別line/region coverage。
   - `marginalis-service`の現行composition rootとv0.3 HTTP/OIDC/MCP結合経路に限定したcoverage。
4. `cargo tree`、`cargo metadata`、明示的な禁止symbol・route検査を組み合わせ、本番到達性をrelease
   gateで検証する。coverageだけを旧実装削除の証明にしない。
5. 最初のbaselineでは一律の合格率を設けない。未実行箇所を分類し、不要コードを削除した後、
   security境界と業務規則からcrate別の下限を決める。下限は低下を防ぐ方向にだけ更新する。
6. reportへ秘密値、OIDC token、Cookie、実利用者データを含めない。CI artifactを保存する場合は
   保存期間と公開範囲を明示する。

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
