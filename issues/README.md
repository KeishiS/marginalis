# AsciiDocライブラリ統合アダプタの実装計画

このディレクトリは、本アプリのAsciiDoc機能プロファイルを一般的なAsciiDocライブラリへ
適用するための実装Issueを管理する。現在の実装候補はAdocWeaveだが、各Issueは標準構文と
公開APIに対するホスト側アダプタとして記述する。ライブラリ固有の内部実装やアプリ固有forkを
前提にしない。

## 実装順序

1. [008: 一般AsciiDocライブラリへの適用アダプタ](008-asciidoc-library-adaptation-boundary.md)
2. [001: 依存固定と契約監視](001-adocweave-dependency-and-contract.md)
3. [002: ノート用プロファイルと属性検証](002-note-profile-and-metadata.md)
4. [003: ノート参照とResolver](003-note-references-and-resolver.md)
5. [004: 安全なHTML、数式、コード表示](004-safe-rendering-and-presentation.md)
6. [005: 検索・グラフ投影と再構築](005-projections-and-rebuild.md)
7. [006: ブラウザ編集プレビュー](006-browser-preview.md)
8. [007: 結合試験とリリース検証](007-integration-testing-and-release.md)

`001`から`005`は保存・閲覧・検索・グラフのサーバ側機能を成立させる最小経路である。
`006`は編集体験を改善するが、サーバ側の検証を置き換えない。`007`は全Issueの完了条件を
継続的に検証する。

上流AsciiDocライブラリに提案する汎用APIのIssue本文は、[upstream](upstream/README.md)に分けて
管理する。アプリ固有のUUID、ACL、SQLiteおよびBase URL決定は、上流提案へ含めない。

アプリケーション全体の認証・運用上の前提は[009: OIDCプロバイダ登録と実環境結合試験](009-oidc-provider-registration.md)
で管理する。

## 実装原則

- AdocWeaveコアはファイル、DB、ネットワーク、時刻および認証情報へアクセスさせない。
- ノートID、ACL、参照先の実在確認およびURL生成はホスト拡張で行う。
- HTMLレンダラーには、同一revisionの解析結果から作った`RenderInputs`だけを渡す。
- 保存時はstrict、編集中はpermissiveとし、どちらも生HTMLを出力しない。
- AdocWeaveのCore、HTML、ProjectionおよびWASM契約versionを、キャッシュと投影の
  再構築判断に使用する。
- `xref:note:`、文書属性、STEMおよびsource blockは標準AsciiDoc構文として扱う。アプリの
  アダプタはAST検証、Resolver、描画入力および投影を担い、新しいパーサー文法を追加しない。
