# 019: Web security baselineとroot管理境界

状態: 提案。

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

- 現行proxyを信頼し、client IPを受け入れるためのCIDR/header設定を提供するか。それともroot rate limitは
  proxy側でのみ実施し、MarginalisはTCP peer固定のままとするか。
- 専用管理origin/mTLSをnext releaseの必須にするか、route分離だけをcurrent releaseの到達点にするか。

