# 001: AdocWeaveの依存固定と互換性検証

位置付け: AdocWeave連携に関する恒久要件を定める。v0.6.1への具体的な移行作業は
[Issue 029](029-adocweave-v0.6.1-migration.md)で管理する。

## 概要

AdocWeaveを再現可能な形で利用し、更新に伴うHTML、検索・グラフ投影、WASMプレビューの
互換性変更を検出できるようにする。

## 対象範囲

- 採用するAdocWeaveのタグとコミットを依存定義へ固定する。
- AdocWeave連携とアプリケーション固有の拡張を、専用のRustクレートへ集約する。
- Rust、HTML、Projection、ConformanceおよびWASMの成果物が、同じAdocWeaveリリースに
  基づくことを検証する。
- AdocWeave更新時に、HTMLキャッシュ、検索・参照投影、診断およびWASM配布物のうち、
  再構築が必要な対象を明示する。

## 対象外

- ノート固有の構文と属性の実装。
- 参照先のDB照会、HTML表示およびブラウザーWorkerの実装。

## 完了条件

- 同じlockfileから同じAdocWeaveバージョンをビルドできる。
- CIが固定したコミットとパッケージ版を記録し、両者の対応を検証する。
- 依存更新時に、互換性の差分と再構築対象が明示される。
- アプリケーション固有の拡張がAdocWeaveの公開API以外へ依存しない。

## 関連Issue

- 移行作業: [Issue 029](029-adocweave-v0.6.1-migration.md)
