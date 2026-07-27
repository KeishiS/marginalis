# 047: 実行時の運用診断

## 状態

実装完了。読み取り専用診断、安定event名、NixOS運用手順、障害注入と回復試験を追加した。

## 目的

利用者10名・約1,000ノートの単一NixOS hostを、追加の監視基盤なしで診断できるようにする。
正常性と失敗原因をsystemd、終了status、journald、request IDから確認できる境界を整える。

## 作業内容

1. 公開liveness、SQLiteの利用可否、schema、OIDC discovery、保守jobをどの境界で診断するかを
   定義する。外部IdPの一時障害だけでHTTP service全体を停止扱いにしない。
2. 運用者がlocal commandからschema、SQLite整合性、設定の非秘密部分を確認できる診断経路を追加する。
3. backup、復元検証、purge、OIDC discovery、MCP OAuthの成功・失敗logに安定したevent名と
   必要最小限のfieldを付ける。
4. systemd unitの失敗、timerの最終実行結果、backupの最終成功世代を確認する手順を
   `docs/nixos.md`へ追加する。
5. database破損、schema不一致、OIDC到達不能、backup保存先不足、期限切れ認証状態のpurge失敗を
   NixOS VMまたは結合試験で再現する。

## 非目標

- Prometheus、OpenTelemetry collector、外部監視サービスを必須依存にすること。
- raw検索語、ノート本文、OAuth parameter、token、Cookieをlogまたはmetricsへ出すこと。
- 利用者行動の分析基盤を追加すること。

## 完了条件

- service停止、部分的なOIDC障害、保守job失敗を区別して診断できる。
- 主要な失敗logに秘密情報を含まず、request IDまたはsystemd invocationから追跡できる。
- NixOS運用手順だけでtimer、backup世代、database、OIDC状態を確認できる。
- 新しい常駐serviceを追加せず、通常運用の確認手順が自動試験で保護されている。
