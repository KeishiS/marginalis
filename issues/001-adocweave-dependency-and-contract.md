# 001: AdocWeave依存固定と契約監視

## 目的

AdocWeaveを本アプリの管理下で再現可能に利用し、更新に伴うHTML、検索・グラフ投影、
WASMプレビューの互換性変更を検出できるようにする。

## 範囲

- 採用するAdocWeaveのtagとcommitを依存定義へ固定する。
- アプリ内に、AdocWeave連携とアプリ固有拡張を置くRust crate境界を作る。
- AdocWeaveの単一`CONTRACT_VERSION`を起動時またはビルド時に照合し、Core、HTML、
  Projection、ConformanceおよびWASMの成果物を同じreleaseへ揃える。
- AdocWeave更新時に、影響するHTML cache、検索・参照projection、診断およびWASM資産の
  再構築を要求する移行手順を定義する。

## 範囲外

- ノート固有の構文・属性の実装。
- 参照先のDB解決、HTML表示およびブラウザWorkerの実装。

## 完了条件

- 同じlockfileから同じAdocWeave versionをビルドできる。
- CIが採用commitと単一`CONTRACT_VERSION`を記録・検証する。
- 依存更新PRでは、契約version差分と必要な再構築対象が明示される。
- アプリ固有拡張がAdocWeaveの公開API以外へ依存しない。

## 依存関係

なし。
