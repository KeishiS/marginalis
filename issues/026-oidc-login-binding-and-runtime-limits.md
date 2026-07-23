# 026: OIDCログイン開始要求のブラウザ結合と実行時制限

状態: RC.1 release blocker（ログインCSRF）。その他はcurrent releaseの運用改善。

## 問題

OIDCの`state`はDB内の一回限りtokenとして検証されるが、ログイン開始したブラウザとの結合がない。
攻撃者が取得した自身のcode/stateを別のブラウザに踏ませるログインCSRFを防げない。

一般sessionのidle timeoutは8時間に固定され、要件の24時間および設定可能な期限と一致しない。OIDC login
attempt、削除確認、認可code、失効済みsession/tokenには、期限切れ行を一括削除するmaintenance経路がない。
reverse proxy背後のroot login rate limitもTCP peer単位であるため、全利用者が同一バケツになる。

## 実装項目

1. OIDC開始時に、HttpOnlyかつSameSiteなbrowser-bound nonce cookieを発行し、stateと対応するhashを保存する。
   callbackでは両者を一回限り照合し、成功・失敗ともにcookieを消去する。
2. session、認可code、OIDC attempt、削除確認、失効済みtokenの保持・cleanup policyを設定化し、定期
   maintenanceとして実行する。一般sessionの既定idle timeoutは24時間へ合わせる。
3. root loginの利用者単位rate limitは、forwarded headerを信頼しない現在方針を維持したまま、trusted proxy
   または専用管理origin/mTLSへ移行するまでのDoS影響を運用文書と受入試験に明記する。
4. login CSRF、cookie欠落・不一致、expiry cleanupおよびproxy構成でのrate limitを試験する。

## 完了条件

- callbackは開始した同一ブラウザだけで完了できる。
- 期限切れの認証関連データが定期的に削除され、保持期間は設定で変更できる。
- root login rate limitの信頼境界と残余リスクが明文化される。

