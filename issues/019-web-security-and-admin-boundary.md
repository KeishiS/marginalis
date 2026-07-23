# 019: Web security baselineとroot管理境界

状態: 完了。

## 目的

Cookie認証を用いるREST/root管理のsecurity最低条件を、reverse proxy構成を含めて明確かつ検証可能にする。

## 現状の問題

- CSRFはtoken照合のみで、要件にある`Origin`およびFetch Metadata検証が未実装である。
- logoutのCookie削除Pathが`/`固定で、Base URLのsubpathと一致しない。
- root login rate limitはprocess内・TCP peer単位であり、reverse proxy経由では全利用者が同一sourceとなる。
- root routeは通常routeと同じrouter/listenerにある。専用origin/mTLSは後続だが、分離可能なrouter境界も
  明示的には存在しない。

## 実装項目

1. cookie発行・削除を同じpath policyへ統一し、subpath testを追加する。
2. 状態変更requestにCSRF token、Origin allowlist、Fetch Metadata policyを適用する。
3. proxy trust policyを導入する。既定はforwarded headerを信頼せず、必要時だけ信頼proxy CIDRまたは
   proxy-provided authenticated headerを設定する。
4. root routeを独立routerへ分け、将来の別listener/origin/mTLS設定で業務ロジックを移動しない構造にする。
5. rate limitの責務を、applicationの永続的な認証試行制御とproxyの粗い接続制御へ分ける。

## 完了条件

- subpathでlogin/logout Cookieが同じPathを使う。
- cross-site state-changing requestがCSRF/Origin/Fetch Metadataのいずれかを欠けば拒否される。
- reverse proxyの有無でrate limitの意味が文書・testともに明確である。
- root routerを別listenerへ載せ替える変更がHTTP route設定だけで済む。

## 要判断事項

- root loginのclient IP単位rate limitはreverse proxy側で実施する。Marginalisはforwarded headerを信頼せず、
  TCP peer単位の補助制限だけを維持する。
- current releaseではroot routeを独立routerへ分離する。専用管理origin/mTLSは後続とする。

## 実施結果

- logoutはsessionとCSRF Cookieの双方を、発行時と同じBase URL subpathで削除する。subpath testで検証する。
- Cookie sessionを伴う変更操作はCSRF token、固定した公開origin、`Sec-Fetch-Site`をすべて検証する。
- client IPはTCP peerだけを使い、forwarded headerを信頼しない。proxy配下の利用者単位制限はproxy側の責務として
  NixOS運用文書へ明記した。
- `administration_router`はroot loginと`/api/v1/admin/*`だけを収容し、composition rootで通常routerへmergeする。
  将来の別listener/origin/mTLS化はrouterの載せ替えだけで行える。
