# AdocWeave統合・拡張の実装計画

このディレクトリは、本アプリのAsciiDoc機能プロファイルをAdocWeaveで実現するための
実装Issueを管理する。各Issueは、アプリ固有の要件をAdocWeaveの標準コア、ホスト拡張、
Web UIのいずれへ実装するかを明確にする。

## 実装順序

1. [001: 依存固定と契約監視](001-adocweave-dependency-and-contract.md)
2. [002: ノート用プロファイルと属性検証](002-note-profile-and-metadata.md)
3. [003: ノート参照とResolver](003-note-references-and-resolver.md)
4. [004: 安全なHTML、数式、コード表示](004-safe-rendering-and-presentation.md)
5. [005: 検索・グラフ投影と再構築](005-projections-and-rebuild.md)
6. [006: ブラウザ編集プレビュー](006-browser-preview.md)
7. [007: 結合試験とリリース検証](007-integration-testing-and-release.md)

`001`から`005`は保存・閲覧・検索・グラフのサーバ側機能を成立させる最小経路である。
`006`は編集体験を改善するが、サーバ側の検証を置き換えない。`007`は全Issueの完了条件を
継続的に検証する。

## 実装原則

- AdocWeaveコアはファイル、DB、ネットワーク、時刻および認証情報へアクセスさせない。
- ノートID、ACL、参照先の実在確認およびURL生成はホスト拡張で行う。
- HTMLレンダラーには、同一revisionの解析結果から作った`RenderInputs`だけを渡す。
- 保存時はstrict、編集中はpermissiveとし、どちらも生HTMLを出力しない。
- AdocWeaveのCore、HTML、ProjectionおよびWASM契約versionを、キャッシュと投影の
  再構築判断に使用する。
