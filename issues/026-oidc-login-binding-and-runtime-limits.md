# 026: OIDCログイン開始要求のブラウザ結合と実行時制限

状態: RC.1のログインCSRF対策は完了。期限の設定可能化とtrusted proxy/専用管理originの検討は後続作業。

## 問題

OIDCの`state`はDB内の一回限りtokenとして検証されるが、ログイン開始したブラウザとの結合がない。
攻撃者が取得した自身のcode/stateを別のブラウザに踏ませるログインCSRFを防げない。

一般sessionのidle timeoutは8時間に固定され、要件の24時間および設定可能な期限と一致しない。OIDC login
attempt、削除確認、認可code、失効済みsession/tokenには、期限切れ行を一括削除するmaintenance経路がない。
reverse proxy背後のroot login rate limitもTCP peer単位であるため、全利用者が同一バケツになる。

## 実装項目

1. OIDC開始時に、HttpOnlyかつSameSiteなbrowser-bound nonce cookieを発行し、stateと対応するhashを保存する。
   callbackでは両者を一回限り照合し、成功・失敗ともにcookieを消去する。
2. 完了: session、認可code、OIDC attempt、削除確認、失効済みtokenを日次maintenanceで削除し、
   一般sessionの既定idle timeoutを24時間とする。保持期限の設定可能化は後続とする。
3. root loginの利用者単位rate limitは、forwarded headerを信頼しない現在方針を維持したまま、trusted proxy
   または専用管理origin/mTLSへ移行するまでのDoS影響を運用文書と受入試験に明記する。
4. login CSRF、cookie欠落・不一致、expiry cleanupおよびproxy構成でのrate limitを試験する。

### 実行時上限（v0.1.0 freeze後・roadmap段階6）

5. 明示的なresource上限を導入し、契約として文書化する。少なくともノートサイズ、タイトル長、
   タグ長・タグ数、request body上限を対象とし、既定値と設定項目を定める（ヒアリング時の目安:
   ノート1 MiB、タイトル200文字、タグ64文字、ノートあたり50タグ、request 2 MiB）。超過は
   既存の`validation-failed`系エラーで機械判定可能に返す。`/api/v1`内の後方互換な追加として
   OpenAPIへ反映する。
6. 未認証で書き込みを発生させる経路（`/auth/oidc/login`と未認証の`/oauth/authorize`が
   `oidc_login_attempts`へINSERTする）に、application側の粗い接続元rate limitまたは
   保留行数の上限を設ける。proxy側制限（docs/nixos.md）は緩和せず、多層防御として扱う。
7. in-memory rate limiterのwindow map（root login・MCP）に、期限切れエントリの削除を入れて
   無制限な成長を防ぐ。

## 完了条件

- callbackは開始した同一ブラウザだけで完了できる。
- 期限切れの認証関連データが定期的に削除される。保持期間の設定可能化は後続とする。
- root login rate limitの信頼境界と残余リスクが明文化される。
- resource上限が実装・文書化され、要件定義の「上限は未導入」の記述が更新される。
- 未認証経路のDB書き込みが上限またはrate limitで抑制される。
