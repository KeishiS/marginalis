import { FormEvent, useEffect, useState } from "react";

import {
  ApplicationConfig,
  McpClientAuthorization,
  McpScopeCeiling,
  deleteMcpClientScopeCeiling,
  listMcpAuthorizations,
  readMcpScopeCeiling,
  replaceMcpClientScopeCeiling,
  replaceMcpScopeCeiling,
  revokeMcpAuthorization,
} from "../api";
import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { Button } from "@/components/ui/button";

import { ConfirmationDialog } from "../ConfirmationDialog";
import { externalPath } from "../paths";

const SCOPE_DESCRIPTIONS: Record<string, string> = {
  "notes:read": "閲覧: list_notes、get_note、get_note_profile",
  "notes:write": "作成・更新: create_note、update_note、get_note_profile",
  "notes:delete": "削除: delete_note",
  "notes:sync": "外部検索用コピーとの継続同期: sync_notes",
  "bibliography:read": "閲覧: search_bibliography",
  "bibliography:write": "作成: add_bibliography_item",
  "bibliography:delete": "削除: delete_bibliography_item",
};

export function McpAccessSettingsPage({
  config,
}: {
  config: ApplicationConfig;
}) {
  const [settings, setSettings] = useState<McpScopeCeiling | null>(null);
  const [authorizations, setAuthorizations] = useState<
    McpClientAuthorization[] | null
  >(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [clientSelections, setClientSelections] = useState<
    Record<string, string[]>
  >({});
  const [saving, setSaving] = useState(false);
  const [busyClient, setBusyClient] = useState<string | null>(null);
  const [revokeTarget, setRevokeTarget] =
    useState<McpClientAuthorization | null>(null);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    Promise.all([
      readMcpScopeCeiling(config.apiBase, controller.signal),
      listMcpAuthorizations(config.apiBase, controller.signal),
    ])
      .then(([value, clients]) => {
        if (!controller.signal.aborted) {
          setSettings(value);
          setSelected(value.scopes);
          setAuthorizations(clients);
          setClientSelections(
            Object.fromEntries(
              clients.map((client) => [client.client_id, client.scope_ceiling]),
            ),
          );
        }
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setFailed(true);
          setMessage("MCPのアクセス設定を読み込めませんでした。");
        }
      });
    return () => controller.abort();
  }, [config.apiBase]);

  function toggle(scope: string, checked: boolean) {
    setSelected((current) =>
      checked
        ? [...current, scope]
        : current.filter((value) => value !== scope),
    );
    setMessage("");
  }

  function toggleClientScope(
    clientId: string,
    scope: string,
    checked: boolean,
  ) {
    setClientSelections((current) => {
      const selectedScopes = current[clientId] ?? [];
      return {
        ...current,
        [clientId]: checked
          ? [...selectedScopes, scope]
          : selectedScopes.filter((value) => value !== scope),
      };
    });
    setMessage("");
  }

  async function saveClient(client: McpClientAuthorization) {
    if (busyClient !== null) return;
    setBusyClient(client.client_id);
    setMessage("");
    try {
      const selectedScopes = clientSelections[client.client_id] ?? [];
      // 上限は将来の認可を制限する設定なので、これまで同意した範囲ではなく
      // サーバーが対応する全scopeから選ぶ。
      const ordered = (settings?.supported_scopes ?? []).filter((scope) =>
        selectedScopes.includes(scope),
      );
      const saved = await replaceMcpClientScopeCeiling(
        config.apiBase,
        client.client_id,
        { scopes: ordered, revision: client.scope_ceiling_revision },
      );
      setAuthorizations(
        (current) =>
          current?.map((value) =>
            value.client_id === saved.client_id ? saved : value,
          ) ?? null,
      );
      setClientSelections((current) => ({
        ...current,
        [saved.client_id]: saved.scope_ceiling,
      }));
      setFailed(false);
      setMessage(
        "クライアント別の上限を保存しました。変更前の上限を超える接続は失効し、追加した操作を使うには再認可が必要です。",
      );
    } catch {
      setFailed(true);
      setMessage(
        "クライアント別の上限を保存できませんでした。画面を再読み込みしてからお試しください。",
      );
    } finally {
      setBusyClient(null);
    }
  }

  /// 上限設定を取り除き、未設定へ戻す。既存の接続とtokenはそのまま残る。
  async function clearClientCeiling(client: McpClientAuthorization) {
    if (busyClient !== null) return;
    setBusyClient(client.client_id);
    setMessage("");
    try {
      const saved = await deleteMcpClientScopeCeiling(
        config.apiBase,
        client.client_id,
        client.scope_ceiling_revision,
      );
      setAuthorizations(
        (current) =>
          current?.map((value) =>
            value.client_id === saved.client_id ? saved : value,
          ) ?? null,
      );
      setClientSelections((current) => ({
        ...current,
        [saved.client_id]: saved.scope_ceiling,
      }));
      setFailed(false);
      setMessage(
        "クライアント別の上限を解除しました。現在はサーバーが対応する全scopeを許可できます。追加した操作を使うには再認可が必要です。",
      );
    } catch {
      setFailed(true);
      setMessage(
        "クライアント別の上限を解除できませんでした。画面を再読み込みしてからお試しください。",
      );
    } finally {
      setBusyClient(null);
    }
  }

  async function revokeClient() {
    if (revokeTarget === null || busyClient !== null) return;
    const clientId = revokeTarget.client_id;
    setBusyClient(clientId);
    try {
      await revokeMcpAuthorization(config.apiBase, clientId);
      setAuthorizations(
        (current) =>
          current?.map((client) =>
            client.client_id === clientId
              ? { ...client, active: false }
              : client,
          ) ?? null,
      );
      setFailed(false);
      setMessage("MCPクライアントの接続を取り消しました。");
      setRevokeTarget(null);
    } catch {
      setFailed(true);
      setMessage("MCPクライアントの接続を取り消せませんでした。");
    } finally {
      setBusyClient(null);
    }
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (settings === null || saving) return;
    setSaving(true);
    setMessage("");
    const narrows = settings.scopes.some((scope) => !selected.includes(scope));
    try {
      const ordered = settings.supported_scopes.filter((scope) =>
        selected.includes(scope),
      );
      const saved = await replaceMcpScopeCeiling(config.apiBase, {
        scopes: ordered,
        revision: settings.revision,
      });
      setSettings(saved);
      setSelected(saved.scopes);
      setFailed(false);
      setMessage(
        narrows
          ? "MCPのアクセス設定を保存しました。新しい上限を超える接続は再認可が必要です。"
          : "MCPのアクセス設定を保存しました。追加した権限を使うには再認可が必要です。",
      );
    } catch {
      setFailed(true);
      setMessage(
        "MCPのアクセス設定を保存できませんでした。画面を再読み込みしてからお試しください。",
      );
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="page-section mcp-access-settings">
      <div className="page-heading">
        <div>
          <p className="page-eyebrow">Settings</p>
          <h1>MCPのアクセス制御</h1>
          <p className="page-description">
            あなたのノートと書誌情報について、MCPクライアントへ許可できる操作の上限を設定します。各クライアントへ実際に与える権限は、この上限を超えません。
          </p>
        </div>
        <Button variant="outline" asChild>
          <a href={externalPath(config.basePath, "/settings")}>設定へ戻る</a>
        </Button>
      </div>
      {settings === null || authorizations === null ? (
        !message ? (
          <StatusMessage>MCPのアクセス設定を読み込んでいます。</StatusMessage>
        ) : (
          <ProblemAlert>{message}</ProblemAlert>
        )
      ) : (
        <div className="mcp-access-settings-content">
          <form className="surface" onSubmit={save}>
            <fieldset>
              <legend>すべてのクライアントに対する上限</legend>
              <p className="field-help">
                チェックを外した操作は、どのMCPクライアントにも許可できません。ノートの共有設定による閲覧範囲が広がることはありません。
              </p>
              <div className="mcp-scope-options">
                {settings.supported_scopes.map((scope) => (
                  <label key={scope}>
                    <input
                      type="checkbox"
                      checked={selected.includes(scope)}
                      onChange={(event) => toggle(scope, event.target.checked)}
                    />
                    <span>
                      <code>{scope}</code>
                      <small>{SCOPE_DESCRIPTIONS[scope] ?? scope}</small>
                    </span>
                  </label>
                ))}
              </div>
            </fieldset>
            <p className="field-help">
              上限を狭めると、新しい上限を超えるMCP接続だけが直ちに失効します。上限を広げても既存の接続へ権限は追加されません。
            </p>
            {message ? (
              failed ? (
                <ProblemAlert>{message}</ProblemAlert>
              ) : (
                <StatusMessage>{message}</StatusMessage>
              )
            ) : null}
            <Button disabled={saving}>
              {saving ? "保存しています…" : "アクセス設定を保存"}
            </Button>
          </form>
          <section aria-labelledby="mcp-client-authorizations-heading">
            <div className="section-heading">
              <h2 id="mcp-client-authorizations-heading">
                認可済みクライアント
              </h2>
              <p>
                接続時に同意した操作を確認し、クライアントごとに制限または取消できます。
              </p>
            </div>
            {authorizations.length === 0 ? (
              <StatusMessage>
                認可済みのMCPクライアントはありません。
              </StatusMessage>
            ) : (
              <div className="mcp-client-list">
                {authorizations.map((client) => (
                  <article
                    className="surface mcp-client-card"
                    key={client.client_id}
                  >
                    <div className="mcp-client-heading">
                      <div>
                        <h3>{client.display_name}</h3>
                        <code>{client.client_id}</code>
                      </div>
                      <span
                        className={
                          client.active ? "status-active" : "status-inactive"
                        }
                      >
                        {client.active ? "有効" : "無効"}
                      </span>
                    </div>
                    <dl className="oauth-detail-list">
                      <div>
                        <dt>登録方式</dt>
                        <dd>
                          {client.registration_method === "metadata_document"
                            ? "Client ID Metadata Document"
                            : "動的クライアント登録"}
                        </dd>
                      </div>
                      <div>
                        <dt>認可日時</dt>
                        <dd>{formatTimestamp(client.authorized_at_ms)}</dd>
                      </div>
                      <div>
                        <dt>最終利用日時</dt>
                        <dd>
                          {client.last_used_at_ms === null
                            ? "まだ利用されていません"
                            : formatTimestamp(client.last_used_at_ms)}
                        </dd>
                      </div>
                    </dl>
                    <fieldset disabled={busyClient === client.client_id}>
                      <legend>クライアント別の上限</legend>
                      <p className="field-help">
                        {!client.scope_ceiling_configured
                          ? "未設定です。現在はサーバーが対応する全scopeを許可できます。上限を設定すると、選んだscopeだけを許可します。"
                          : "選んだscopeだけを許可しています。上限は今後の認可を制限する設定であり、それ自体が権限を与えることはありません。範囲を広げても既存tokenへ権限は追加されないため、追加した操作にはOAuth再認可が必要です。"}
                      </p>
                      <div className="mcp-scope-options">
                        {(settings?.supported_scopes ?? []).map((scope) => (
                          <label key={scope}>
                            <input
                              type="checkbox"
                              checked={(
                                clientSelections[client.client_id] ?? []
                              ).includes(scope)}
                              onChange={(event) =>
                                toggleClientScope(
                                  client.client_id,
                                  scope,
                                  event.target.checked,
                                )
                              }
                            />
                            <span>
                              <code>{scope}</code>
                              <small>
                                {SCOPE_DESCRIPTIONS[scope] ?? scope}
                              </small>
                            </span>
                          </label>
                        ))}
                      </div>
                    </fieldset>
                    <div className="mcp-client-actions">
                      <Button
                        type="button"
                        disabled={busyClient !== null}
                        onClick={() => void saveClient(client)}
                      >
                        {!client.scope_ceiling_configured
                          ? "上限を設定"
                          : "上限を保存"}
                      </Button>
                      {client.scope_ceiling_configured ? (
                        <Button
                          variant="outline"
                          type="button"
                          disabled={busyClient !== null}
                          onClick={() => void clearClientCeiling(client)}
                        >
                          上限を解除
                        </Button>
                      ) : null}
                      <Button
                        variant="destructive"
                        type="button"
                        disabled={!client.active || busyClient !== null}
                        onClick={() => setRevokeTarget(client)}
                      >
                        接続を取り消す
                      </Button>
                    </div>
                  </article>
                ))}
              </div>
            )}
          </section>
        </div>
      )}
      {revokeTarget !== null ? (
        <ConfirmationDialog
          eyebrow="MCP access"
          heading="MCPクライアントの接続を取り消しますか？"
          description="このクライアントへ発行したaccess tokenとrefresh tokenを直ちに失効します。再び使うにはOAuth認可が必要です。"
          confirmLabel="接続を取り消す"
          busyLabel="取り消しています…"
          destructive
          busy={busyClient !== null}
          problem={failed ? message : null}
          onCancel={() => setRevokeTarget(null)}
          onConfirm={() => void revokeClient()}
        />
      ) : null}
    </section>
  );
}

function formatTimestamp(value: number): string {
  return new Intl.DateTimeFormat("ja-JP", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}
