import { FormEvent, useCallback, useEffect, useRef, useState } from "react";

import {
  addBibliographyItem,
  ApplicationConfig,
  BibliographyItem,
  deleteBibliographyItem,
  searchBibliography,
  updateBibliographyItem,
} from "../api";

/** 操作の結果。失敗は利用者の対応を促すため、成功の知らせと区別して伝える。 */
interface Message {
  text: string;
  failed: boolean;
}

const notice = (text: string): Message => ({ text, failed: false });
const failure = (text: string): Message => ({ text, failed: true });

/** URLの`query`を初期の絞り込み条件として読む。関係の図から文献を選んだ場合に使う。 */
function initialQuery(search: string): string {
  return new URLSearchParams(search).get("query") ?? "";
}

export function BibliographyPage({ config }: { config: ApplicationConfig }) {
  const [items, setItems] = useState<BibliographyItem[] | null>(null);
  const [query, setQuery] = useState(() => initialQuery(config.search));
  const [input, setInput] = useState(
    '{\n  "id": "smith2024",\n  "type": "article-journal",\n  "title": "Example title"\n}',
  );
  const [message, setMessage] = useState<Message | null>(null);
  const [loadError, setLoadError] = useState("");
  const [searching, setSearching] = useState(false);
  const [mutating, setMutating] = useState(false);
  const [editing, setEditing] = useState<BibliographyItem | null>(null);
  const activeSearch = useRef<AbortController | null>(null);

  const load = useCallback(
    async (search: string) => {
      activeSearch.current?.abort();
      const controller = new AbortController();
      activeSearch.current = controller;
      setSearching(true);
      try {
        const loaded = await searchBibliography(
          config.apiBase,
          search,
          controller.signal,
        );
        if (!controller.signal.aborted) {
          setItems(loaded);
          setLoadError("");
        }
      } catch {
        if (!controller.signal.aborted) {
          setLoadError("書誌ライブラリーを読み込めませんでした。");
        }
      } finally {
        if (!controller.signal.aborted && activeSearch.current === controller) {
          setSearching(false);
        }
      }
    },
    [config.apiBase],
  );
  const initial = initialQuery(config.search);
  useEffect(() => {
    const controller = new AbortController();
    activeSearch.current = controller;
    searchBibliography(config.apiBase, initial, controller.signal)
      .then((loaded) => {
        if (!controller.signal.aborted) {
          setItems(loaded);
          setLoadError("");
        }
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setLoadError("書誌ライブラリーを読み込めませんでした。");
        }
      });
    return () => controller.abort();
  }, [config.apiBase, initial]);
  useEffect(
    () => () => {
      activeSearch.current?.abort();
    },
    [],
  );

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (mutating) return;
    setMutating(true);
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
        setMessage(notice("書誌情報を更新しました。"));
      } else {
        await addBibliographyItem(
          config.apiBase,
          value as Record<string, unknown>,
        );
        setMessage(notice("書誌情報を登録しました。"));
      }
      await load(query);
    } catch {
      setMessage(
        failure(
          "登録できませんでした。CSL-JSONのid、type、JSON構文を確認してください。",
        ),
      );
    } finally {
      setMutating(false);
    }
  }

  async function remove(item: BibliographyItem) {
    if (mutating) return;
    setMutating(true);
    try {
      await deleteBibliographyItem(config.apiBase, item.item_id, item.revision);
      setMessage(notice("書誌情報を削除しました。"));
      await load(query);
    } catch {
      setMessage(failure("書誌情報を削除できませんでした。"));
    } finally {
      setMutating(false);
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
          if (mutating) return;
          void load(query);
        }}
      >
        <label>
          文献を検索
          <input
            disabled={mutating}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="citation key、題名、著者、DOI"
          />
        </label>
        <button type="submit" disabled={mutating}>
          {searching ? "検索しています…" : "検索"}
        </button>
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
            disabled={mutating}
          />
        </label>
        <div className="bibliography-actions">
          <button
            className="button button-primary"
            type="submit"
            disabled={mutating}
          >
            {mutating ? "処理しています…" : editing ? "更新" : "登録"}
          </button>
          {editing && (
            <button
              className="button button-secondary"
              type="button"
              disabled={mutating}
              onClick={() => {
                setEditing(null);
                setMessage(notice("編集を取り消しました。"));
              }}
            >
              取消
            </button>
          )}
        </div>
      </form>
      {message &&
        (message.failed ? (
          <p className="problem-inline" role="alert">
            {message.text}
          </p>
        ) : (
          <p className="state-message" role="status">
            {message.text}
          </p>
        ))}
      {loadError && (
        <p className="problem-inline" role="alert">
          {loadError}
        </p>
      )}
      {items === null ? (
        <p className="state-message" role="status">
          書誌情報を読み込んでいます。
        </p>
      ) : items.length === 0 ? (
        <p className="state-message">登録済みの書誌情報はありません。</p>
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
                  setMessage(notice(`${item.citation_key}を編集中です。`));
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
                  className="button button-danger"
                  type="button"
                  disabled={mutating}
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
