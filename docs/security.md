# セキュリティ

本書を依存脆弱性と暗号方式の運用判断の正本とします。認証・認可の業務上の不変条件は
[アーキテクチャ](architecture.md)、NixOSの秘密情報とservice hardeningは
[NixOSでの運用](nixos.md)を参照してください。

`cargo make verify`は最新のRustSec advisory databaseに対して`Cargo.lock`を検査します。
例外はadvisory IDを明示し、到達不能である根拠と解除条件を本書へ記録します。

## RUSTSEC-2023-0071

`openidconnect 4.0.1`は`rsa 0.9.10`へ推移依存し、RSA秘密鍵演算のタイミング漏洩
[RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html)が報告されています。
修正版はありません。

MarginalisはOIDC providerの秘密鍵を保持せず、ID tokenの公開鍵検証だけを行います。さらに
provider discovery後、ID token署名方式をKanidm 1.10と結合試験が使う`ES256`だけに制限し、
RSA署名経路を受け付けません。このため、advisoryが対象とするRSA秘密鍵演算は実行経路に
ありません。`cargo audit`ではこのIDだけを例外にし、他のadvisoryは失敗させます。

`openidconnect`から`rsa`依存が除去される、修正版へ更新できる、またはadvisoryの影響範囲が変わった
時点で例外を削除します。Kanidmの署名方式を変更する場合は、許可方式を広げる前にこの判断を
再評価します。
