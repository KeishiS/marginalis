# 039: Kanidmグループ認可とMCP OAuth

## 状態

実装中。[037](037-v0.3.0-architecture-rebaseline.md)の認証・認可決定を実装する。
検証済み ID token からの構成可能な group claim の fail-closed な読取りを実装した。MCP は Authorization Code + PKCE、refresh
rotation、Dynamic Client Registration、認可取消、OAuth/Protected Resource metadata、Streamable HTTP
のノート操作を提供する。v0.3 の本番用 router、SQLite、OIDC・session・MCP OAuth serviceを結合した
HTTP試験で、`server-users`拒否、`server-admins`可視性、MCP作成、REST可視性、認可取消後の
access/refresh token失効を確認している。対象clientの実環境E2Eは後続作業とする。

## 目的

利用者管理を Kanidm 1.10 へ完全に委譲しつつ、ChatGPT、Claude Code、Codex CLI が利用できる
標準 MCP OAuth を Marginalis から提供する。

## 作業内容

1. OIDC login 時に Kanidm の `server-users` と `server-admins` を検証する。`server-users` に
   属さない利用者は session を発行しない。`server-admins` は全ノートの管理を許可する。
2. 検証済み ID token の `groups` claim を login 時の権限スナップショットとする。group 変更は次回
   login から反映し、既存 session と MCP token は有効期限または認可取消まで発行時の権限を保持する。
3. ローカル `root`、登録ポリシー、保留利用者、招待、SMTP、root-only 管理 API と関連 schema を
   削除する。緊急管理は Kanidm の break-glass 運用へ委ねる。
4. Marginalis の MCP OAuth Authorization Server を新 API に合わせて実装する。OAuth discovery、
   Protected Resource Metadata、Authorization Code + PKCE、refresh token rotation、認可取消、
   Dynamic Client Registration / Client Metadata を提供する。
5. ChatGPT、Claude Code、Codex CLI の各 remote MCP client で OAuth login、read/write、認可取消を
   確認する。資格情報と token を CI のログ・artifact へ残さない。

## 完了条件

- `server-users` と `server-admins` に基づく認可が REST、MCP、Web UI で一致する。
- `groups` claim を持たない、または `server-users` 非所属の主体を拒否し、`server-admins` を session と MCP
  authorization に一貫して反映する。
- ローカル root と独自の利用者ライフサイクルを参照する実装・設定・運用手順が残らない。
- 三つの対象 MCP client で OAuth 認可と認可取消を確認できる。
