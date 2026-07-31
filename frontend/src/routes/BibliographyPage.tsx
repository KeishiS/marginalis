import { FormEvent, useEffect, useState } from "react";

import {
  addBibliographyItem,
  ApplicationConfig,
  BibliographyItem,
  deleteBibliographyItem,
  searchBibliography,
  updateBibliographyItem,
} from "../api";

export function BibliographyPage({ config }: { config: ApplicationConfig }) {
  const [items, setItems] = useState<BibliographyItem[] | null>(null);
  const [query, setQuery] = useState("");
  const [input, setInput] = useState(
    '{\n  "id": "smith2024",\n  "type": "article-journal",\n  "title": "Example title"\n}',
  );
  const [message, setMessage] = useState("");
  const [editing, setEditing] = useState<BibliographyItem | null>(null);

  async function load(search = query) {
    try {
      setItems(await searchBibliography(config.apiBase, search));
      setMessage("");
    } catch {
      setMessage("書誌ライブラリーを読み込めませんでした。");
    }
  }
  useEffect(() => {
    let current = true;
    searchBibliography(config.apiBase, "")
      .then((value) => current && setItems(value))
      .catch(
        () => current && setMessage("書誌ライブラリーを読み込めませんでした。"),
      );
    return () => {
      current = false;
    };
  }, [config.apiBase]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      const value: unknown = JSON.parse(input);
      if (typeof value !== "object" || value === null || Array.isArray(value)) {
        throw new Error("invalid");
      }
      if (editing) {
        await updateBibliographyItem(
          config.apiBase,
          editing.item_id,
          value as Record<string, unknown>,
          editing.revision,
        );
        setEditing(null);
        setMessage("書誌情報を更新しました。");
      } else {
        await addBibliographyItem(
          config.apiBase,
          value as Record<string, unknown>,
        );
        setMessage("書誌情報を登録しました。");
      }
      await load();
    } catch {
      setMessage(
        "登録できませんでした。CSL-JSONのid、type、JSON構文を確認してください。",
      );
    }
  }

  async function remove(item: BibliographyItem) {
    try {
      await deleteBibliographyItem(config.apiBase, item.item_id, item.revision);
      setMessage("書誌情報を削除しました。");
      await load();
    } catch {
      setMessage("書誌情報を削除できませんでした。");
    }
  }

  return (
    <section className="page-section bibliography-library">
      <div className="page-heading">
        <div>
          <p className="page-eyebrow">Bibliography</p>
          <h1>書誌ライブラリー</h1>
          <p className="page-description">
            CSL-JSON形式の文献情報を、ノートとは独立して管理します。
          </p>
        </div>
      </div>
      <form
        className="bibliography-search"
        onSubmit={(event) => {
          event.preventDefault();
          void load();
        }}
      >
        <label>
          文献を検索
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="citation key、題名、著者、DOI"
          />
        </label>
        <button type="submit">検索</button>
      </form>
      <form
        className="bibliography-input"
        onSubmit={(event) => void submit(event)}
      >
        <label>
          CSL-JSON
          <textarea
            rows={12}
            value={input}
            onChange={(event) => setInput(event.target.value)}
            spellCheck={false}
          />
        </label>
        <div className="bibliography-actions">
          <button type="submit">{editing ? "更新" : "登録"}</button>
          {editing && (
            <button
              className="button button-secondary"
              type="button"
              onClick={() => {
                setEditing(null);
                setMessage("編集を取り消しました。");
              }}
            >
              取消
            </button>
          )}
        </div>
      </form>
      {message && <p role="status">{message}</p>}
      {items === null ? (
        <p>書誌情報を読み込んでいます。</p>
      ) : items.length === 0 ? (
        <p>登録済みの書誌情報はありません。</p>
      ) : (
        <ul className="bibliography-list">
          {items.map((item) => (
            <li key={item.item_id}>
              <button
                className="bibliography-item"
                type="button"
                aria-current={editing?.item_id === item.item_id}
                onClick={() => {
                  // 選び直しでは読み込み直さない。押し間違いで編集中の内容が消えるため。
                  if (editing?.item_id === item.item_id) {
                    return;
                  }
                  setEditing(item);
                  setInput(JSON.stringify(item.csl_json, null, 2));
                  setMessage(`${item.citation_key}を編集中です。`);
                }}
              >
                <strong>{item.citation_key}</strong>
                <span>
                  {typeof item.csl_json.title === "string"
                    ? item.csl_json.title
                    : "題名なし"}
                </span>
              </button>
              <div className="bibliography-item-actions">
                <button
                  className="button button-secondary"
                  type="button"
                  onClick={() => void remove(item)}
                >
                  削除
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
