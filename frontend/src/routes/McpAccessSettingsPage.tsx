import { FormEvent, useEffect, useState } from "react";

import {
  ApplicationConfig,
  McpScopeCeiling,
  readMcpScopeCeiling,
  replaceMcpScopeCeiling,
} from "../api";
import { externalPath } from "../paths";

const SCOPE_DESCRIPTIONS: Record<string, string> = {
  "notes:read": "ノートの一覧と本文の読み取り",
  "notes:write": "ノートの作成と更新",
  "notes:delete": "ノートの削除",
  "bibliography:read": "書誌情報の検索",
  "bibliography:write": "書誌情報の追加",
  "bibliography:delete": "書誌情報の削除",
};

export function McpAccessSettingsPage({
  config,
}: {
  config: ApplicationConfig;
}) {
  const [settings, setSettings] = useState<McpScopeCeiling | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    readMcpScopeCeiling(config.apiBase, controller.signal)
      .then((value) => {
        if (!controller.signal.aborted) {
          setSettings(value);
          setSelected(value.scopes);
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

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (settings === null || saving) return;
    setSaving(true);
    setMessage("");
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
        "MCPのアクセス設定を保存しました。既存の接続は再認可が必要です。",
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
        <a
          className="button button-secondary"
          href={externalPath(config.basePath, "/settings")}
        >
          設定へ戻る
        </a>
      </div>
      {settings === null && !message ? (
        <p className="state-message" role="status">
          MCPのアクセス設定を読み込んでいます。
        </p>
      ) : settings === null ? (
        <p className="problem-inline" role="alert">
          {message}
        </p>
      ) : (
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
            保存すると、権限の変更を直ちに反映するため、現在のMCP接続は失効します。必要なクライアントを改めて認可してください。
          </p>
          {message ? (
            <p
              className={failed ? "problem-inline" : "success-inline"}
              role={failed ? "alert" : "status"}
            >
              {message}
            </p>
          ) : null}
          <button className="button button-primary" disabled={saving}>
            {saving ? "保存しています…" : "アクセス設定を保存"}
          </button>
        </form>
      )}
    </section>
  );
}
