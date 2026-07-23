# 実環境受入確認

この手順は、NixOS VM・unit testとは別に、実際のreverse proxy、KanidmおよびMCP clientを通す確認である。
password、Cookie、OIDC code、MCP access/refresh tokenをコマンド履歴、issue、ログまたは共有画面に
貼り付けない。

## 事前条件

1. `GET /api/v1/health`が`200`、`GET /api/v1/readiness`がOIDC `available`として`200`を返す。
2. `services.marginalis.mcp.enable = true`とする場合、reverse proxyが`/mcp`、`/oauth/`および
   `/.well-known/`を同じoriginへ転送する。
3. `approval`登録policyの場合、rootが対象OIDC userを有効化済みである。root操作は
   [REST API](rest-api.md#root管理)のCSRF要件に従う。

## 段階1: OIDC利用者によるREST

1. ブラウザで`/auth/oidc/login`へ移動し、Kanidm login後に`GET /api/v1/session`が`200`かつ
   `is_root: false`を返すことを確認する。
2. Cookie jarとCSRF cookieを管理できるAPI clientで、`POST /api/v1/notes`へ有効なAsciiDoc正本を送る。
   必須header属性は含めるが、`note-id`、`creator-id`、`created-at`、`updated-at`はserverが置換する。
   `201`とsource URLを記録し、取得したsourceがserver値になっていることを確認する。
3. `GET` sourceの`ETag`を取得し、その値を`If-Match`と`X-CSRF-Token`に付けて`PUT`する。`204`後に
   `GET /api/v1/search?q=<固有語>`が作成ノートだけを返すことを確認する。
4. 同じ`ETag`で二度更新して`409`となることを確認する。更新後の`ETag`で削除準備を行い、返された
   confirmation tokenで削除を確定して`204`となることを確認する。削除後はsource取得が`404`であり、
   一覧・検索の`200`応答には対象ノートが含まれないことを確認する。

`marginalis_session`や`marginalis_csrf`をshellの引数・履歴へ貼り付けない。API clientのsecret storeまたは
一時的なCookie jarを用い、確認後に削除する。

### browser開発者ツールによるREST確認例（strict CSPでは使用しない）

OIDC login済みの同一originでbrowser開発者ツールのConsoleを開く方法は、`connect-src`を許可するCSPを
明示的に設定した開発環境だけで使う。current production deploymentの`Content-Security-Policy: default-src 'none'`
では、Consoleの`fetch`も遮断されるため使用してはならない。CSPをこの受入確認のために緩めない。

productionではCookie jarとCSRF Cookieを保持できる外部API clientを使う。そのclient自身のbrowser loginで
OIDC sessionを確立し、同じcookie jarで以下のrequestを実行する。clientが`marginalis_csrf` Cookieを
`X-CSRF-Token`へ展開できない場合は、REST受入の自動化対象から外し、Issue 030のE2E基盤で扱う。

| 順序 | request | 必須設定 | 期待結果 |
| --- | --- | --- | --- |
| 1 | `POST /api/v1/notes` | UTF-8 AsciiDoc body、`Content-Type: text/plain; charset=utf-8`、`X-CSRF-Token` | `201`、`Location` |
| 2 | `GET {Location}` | Cookie jar | `200`、`ETag` |
| 3 | `PUT {Location}` | 更新済みbody、`If-Match: <ETag>`、`X-CSRF-Token` | `204` |
| 4 | 同じ`If-Match`で再度`PUT` | 同上 | `409` |
| 5 | `GET /api/v1/search?q=<固有語>` | Cookie jar | `200`、作成ノートだけを含む |
| 6 | `POST /api/v1/notes/{note_id}/delete-preparations` | `If-Match: <最新ETag>`、`X-CSRF-Token` | `200`、`confirmation_token` |
| 7 | `POST /api/v1/notes/delete-confirmations` | JSON body `{"confirmation_token":"..."}`、`X-CSRF-Token` | `204` |

次のConsoleコードは、CSPを許可したローカル開発環境でだけ利用する。`unique_phrase`は他のノートに含まれない
値へ変更する。`Origin`と`Sec-Fetch-Site`はbrowserが付与するため、JavaScriptから設定しない。

```js
const csrf = document.cookie
  .split('; ')
  .find((part) => part.startsWith('marginalis_csrf='))
  ?.split('=')[1];
if (!csrf) throw new Error('marginalis_csrf cookie is unavailable');

const request = (path, options = {}) => fetch(path, {
  credentials: 'same-origin',
  ...options,
  headers: { 'X-CSRF-Token': csrf, ...(options.headers ?? {}) },
});

const unique_phrase = 'acceptance-unique-phrase-2026-07-23';
const source = `= 受入確認ノート
:note-id: 01800000-0000-7000-8000-000000000001
:creator-id: 01800000-0000-7000-8000-000000000002
:created-at: 2026-07-23T00:00:00.000Z
:updated-at: 2026-07-23T00:00:00.000Z
:tags: acceptance

${unique_phrase}
`;

const created = await request('/api/v1/notes', {
  method: 'POST',
  headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  body: source,
});
if (created.status !== 201) throw new Error(`create: ${created.status}`);
const noteUrl = created.headers.get('Location');
if (!noteUrl) throw new Error('create response has no Location header');

const firstSource = await request(noteUrl);
const firstEtag = firstSource.headers.get('ETag');
const savedSource = await firstSource.text();
if (!firstEtag || !savedSource.includes(unique_phrase)) throw new Error('source or ETag is invalid');
console.log({ noteUrl, firstEtag, savedSource });

const updatedSource = savedSource.replace(unique_phrase, `${unique_phrase} updated`);
const updated = await request(noteUrl, {
  method: 'PUT',
  headers: { 'Content-Type': 'text/plain; charset=utf-8', 'If-Match': firstEtag },
  body: updatedSource,
});
if (updated.status !== 204) throw new Error(`update: ${updated.status}`);

const searched = await request(`/api/v1/search?q=${encodeURIComponent(unique_phrase)}`);
const searchPage = await searched.json();
if (searched.status !== 200 || !searchPage.notes.some((note) => note.note_id === noteUrl.split('/').at(-2))) {
  throw new Error(`search: ${searched.status}`);
}
console.log(searchPage);

const stale = await request(noteUrl, {
  method: 'PUT',
  headers: { 'Content-Type': 'text/plain; charset=utf-8', 'If-Match': firstEtag },
  body: updatedSource,
});
if (stale.status !== 409) throw new Error(`stale update: ${stale.status}`);

const currentSource = await request(noteUrl);
const currentEtag = currentSource.headers.get('ETag');
const preparation = await request(`${noteUrl.replace('/source', '')}/delete-preparations`, {
  method: 'POST',
  headers: { 'If-Match': currentEtag },
});
if (preparation.status !== 200) throw new Error(`delete preparation: ${preparation.status}`);
const confirmation = await preparation.json();

const deleted = await request('/api/v1/notes/delete-confirmations', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ confirmation_token: confirmation.confirmation_token }),
});
if (deleted.status !== 204) throw new Error(`delete confirmation: ${deleted.status}`);

for (const path of [noteUrl, `/api/v1/search?q=${encodeURIComponent(unique_phrase)}`, '/api/v1/notes']) {
  const response = await request(path);
  console.log(path, response.status); // sourceは404、検索・一覧では対象ノートが含まれないことを確認する。
}
```

この例の`note-id`、`creator-id`、日時は形式を満たすダミー値であり、作成時にserver値へ置換される。
Console出力を共有する場合も、Cookie、CSRF token、削除confirmation tokenを含めない。

## 段階2: 実MCP client

1. clientのStreamable HTTP endpointを`https://marginalis.sandi05.com/mcp`に設定する。
2. Protected Resource MetadataからOAuth authorization endpointへ進み、Kanidm login後、通常userとして
   scopeを確認して許可する。root sessionはMCP clientを認可できない。
3. `search_notes`、`create_note`、`get_note`、`update_note`、`prepare_delete_note`、`delete_note`を順に実行する。
   RESTで作ったノートはMCP検索でも、MCPで作ったノートはREST検索でも同じACL結果になることを確認する。
4. RESTの`DELETE /api/v1/mcp-authorizations?client_id=...`で認可を取消し、既存access tokenによる`/mcp`が
   `401`になることを確認する。

unknown clientを使う場合はClient ID Metadata Documentのhostを`clientMetadataAllowedHosts`へ追加する。
metadataを持たないclientは、rootが事前登録してから試す。

## 段階3: 運用確認

1. root監査を読み取り専用で確認する。

   ```sh
   sudo -u marginalis sqlite3 /var/lib/marginalis/marginalis.sqlite \
     'SELECT action, occurred_at_ms FROM root_audit_log ORDER BY audit_id DESC LIMIT 100;'
   ```

2. 永続backup storageと保持世代を決め、`backupDirectory`を設定する。`marginalis-backup.service`を起動後、
   新しいgenerationに`FORMAT`、`MANIFEST`、`COMPLETE`、`marginalis.sqlite`、`notes/`があることを確認する。
3. 本番dataDirを切り替えず、`marginalis restore --input <generation> --output <新しい絶対path>`を実行する。
   `RESTORED` marker、SQLiteおよび正本が作られることを確認する。実際のdataDir切替は、旧データを
   保持する場所とrollback手順を決めてから行う。
4. `marginalis-rebuild-projections.service`を実行し、main serviceを再起動してhealth/readinessを再確認する。
5. `systemctl status marginalis-prune-audit.timer`で、root監査の365日保持と期限切れ認証補助データのcleanupを
   行うtimerが有効であることを確認する。
6. `curl -fsS https://marginalis.sandi05.com/api/v1/openapi.json | jq -e '.openapi == "3.1.0"'`で、実行中binaryが
   公開contractを返すことを確認する。RC.1では、このdocumentとの差分がrelease blocker修正だけであることを確認する。

v0.1.0でAPI versionをfreezeした後に破壊的変更が必要になった場合は、新しいversion pathを追加し、既存versionに
deprecation告知・移行手順・少なくとも一つのrelease周期を設ける。

各段階で、失敗時は`X-Request-Id`と`journalctl -u marginalis.service -b --no-pager`を対応付ける。
