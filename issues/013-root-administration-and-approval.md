# 013: root管理・OIDCユーザー承認・最小管理UI

状態: 計画。

OIDCで認証された初回ユーザーを安全に承認できるよう、ローカル`root`アカウントの認証、管理APIおよび
最小のブラウザー管理UIを実装する。現在の公開origin上で利用可能な簡潔な初期実装とし、専用管理origin、
mTLSおよびVPNによる追加防御は後続issueで扱う。

## 背景と方針

現行実装は起動時に`root`のパスワードハッシュと内部ユーザーUUIDをSQLiteへ作成する。しかし、rootの
パスワード検証、root sessionの発行、保留OIDCユーザーの承認経路は未実装である。そのため、既定の
`approval`登録ポリシーで作成された`pending`ユーザーを正規の操作で有効化できない。

本issueでは、画面を先に作らない。認証・認可・監査をapplication portとREST APIとして固定してから、
それらを利用する最小のサーバー生成HTMLを追加する。root関連のrouteは通常ユーザーのrouteとは別の
routerへ置き、後続で専用originやmTLSを導入しても業務ロジックを変更しない構成にする。

## 範囲

- rootパスワードによるログイン、短期sessionおよびログアウト
- OIDCユーザーの一覧、承認、無効化
- 登録ポリシー（`open`、`approval`、`invite-only`）の取得・変更
- root管理操作の監査記録
- rootログインと保留ユーザー承認のための最小HTML UI
- NixOS設定・運用手順・既存`pending`ユーザーの移行手順

次は範囲外とする。

- 招待トークン・SMTP送信・メールアドレスによる招待制御
- rootパスワードの変更・復旧
- セッションの全件閲覧・一括失効
- 専用管理origin、mTLS、VPN、WebAuthn
- ノート閲覧・編集・ACL管理の一般向けWeb UI

## 設計上の決定

1. **rootはローカルアカウントとする。** OIDCのissuer/subjectと対応付けず、保存済みArgon2 hashだけで
   検証する。パスワード、password hash、session ID、CSRF tokenおよびOIDC tokenをログ・監査本文・
   エラー応答へ含めない。
2. **root sessionは通常sessionと区別する。** `is_root = true`のactorを発行し、無操作30分、絶対8時間を
   初期値とする。Cookieには`Secure`、`HttpOnly`、`SameSite=Lax`を指定する。
3. **登録ポリシーはDBを正本とする。** 新規DBの初期値だけをNixOS option
   `initialRegistrationPolicy`で指定可能にし、初期値は`approval`とする。rootによる変更は以後の新規登録に
   だけ適用し、既存ユーザーの状態を変更しない。NixOS再適用で運用中の値を上書きしない。
4. **root操作は全て監査する。** 少なくともログイン成功・失敗、ログアウト、ユーザー承認、無効化、
   登録ポリシー変更について、操作者、対象、操作種別、時刻および結果を保存する。
5. **公開originは暫定的にroot loginを提供する。** パスワードだけの漏洩対策として、一般的な失敗応答、
   レート制限、CSRF、短期sessionを適用する。後続でroot routerを専用originとmTLSへ移すことを前提にする。

## 実装順序

1. **domain/application: root認証と管理port**
   - `RootCredentialStore`へ一定時間比較を伴うpassword verificationを追加し、成功時にrootの`UserId`を返す。
   - root session lifetimeを明示的に指定できるsession発行use caseを追加する。
   - ユーザー状態、登録ポリシー、監査イベントの型とportを追加する。root以外からの状態変更を拒否する。

2. **SQLite: migrationと監査可能な永続化**
   - registration policyを一意に保存する設定table、管理監査table、ログイン試行のレート制限に必要な状態を
     migrationで追加する。
   - `pending`、`active`、`disabled`の許可された遷移をtransaction内で実装する。
   - 既存DBでは設定値がない場合にだけ`approval`を初期化し、既存の`pending`ユーザーを保持する。

3. **REST: root loginと管理API**
   - `POST /auth/root/login`、`POST /auth/logout`、`GET /api/v1/admin/users`、
     `PUT /api/v1/admin/users/{user_id}/status`、`GET`/`PUT /api/v1/admin/registration-policy`を実装する。
   - login失敗時は常に同じ認証失敗応答を返し、成功・失敗を含む監査記録を残す。
   - 状態変更には既存のCSRF tokenに加え、OriginおよびFetch Metadataを検証する。ログインにはIPおよび
     rootアカウント単位のレート制限を設ける。
   - root route群を通常routerから分離し、将来の専用listener/originに載せ替え可能にする。

4. **最小管理UI**
   - `/admin/login`、`/admin/users`および登録ポリシー変更画面をサーバー生成HTMLで提供する。
   - UIは管理APIと同じ認可・CSRF境界を通す。一般ノートUIやJavaScript SPAは導入しない。
   - `pending`ユーザーの表示名を管理画面だけに表示し、通常利用者へは公開しない。

5. **NixOS・運用・移行**
   - `initialRegistrationPolicy`をNixOS moduleとドキュメントへ追加する。
   - root passwordは初期化専用であり、DB初期化後にsecret設定から除去できることを確認する。
   - 既存の`pending`ユーザーは、root login後に管理UI/APIから承認する手順を文書化する。
   - 管理originをmTLSへ分離する将来構成と、その際に変更しないAPI/router境界を記録する。

6. **検証**
   - root passwordの成功・失敗・レート制限・短期session・logoutをテストする。
   - root以外が管理APIを操作できないこと、CSRF/Origin検証、状態遷移、監査記録をテストする。
   - approvalで初回OIDC loginが`pending`となり、root承認後の再ログインが成功する結合テストを追加する。
   - NixOS VMで初回root初期化、再起動後のroot login、OIDC承認フローを検証する。

## 完了条件

- rootがパスワードでログインし、通常ユーザーより短いsessionで管理操作できる。
- `pending`OIDCユーザーをrootが承認すると、次回OIDC loginで有効なsessionを得られる。
- root以外はユーザー状態および登録ポリシーを変更できない。
- root操作と認証失敗は監査されるが、secretおよび認証tokenは保存・出力されない。
- NixOSの初期登録ポリシーと、DB上の変更可能な運用ポリシーの役割が区別されている。
- root routerは後続で専用originとmTLSへ分離可能である。
