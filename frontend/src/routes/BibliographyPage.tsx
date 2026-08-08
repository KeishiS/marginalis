import { FormEvent, useCallback, useEffect, useRef, useState } from "react";

import {
  addBibliographyItem,
  ApplicationConfig,
  BibliographyItem,
  deleteBibliographyItem,
  searchBibliography,
  updateBibliographyItem,
} from "../api";
import { ConfirmationDialog } from "../ConfirmationDialog";
import { BibliographyImportPanel } from "./BibliographyImportPanel";

/** 操作の結果。失敗は利用者の対応を促すため、成功の知らせと区別して伝える。 */
interface Message {
  text: string;
  failed: boolean;
}

const notice = (text: string): Message => ({ text, failed: false });
const failure = (text: string): Message => ({ text, failed: true });

const EMPTY_INPUT =
  '{\n  "id": "smith2024",\n  "type": "article-journal",\n  "title": "Example title"\n}';

/** URLの`query`を初期の絞り込み条件として読む。関係の図から文献を選んだ場合に使う。 */
function initialQuery(search: string): string {
  return new URLSearchParams(search).get("query") ?? "";
}

export function BibliographyPage({ config }: { config: ApplicationConfig }) {
  const [items, setItems] = useState<BibliographyItem[] | null>(null);
  const [query, setQuery] = useState(() => initialQuery(config.search));
  const [input, setInput] = useState(EMPTY_INPUT);
  const [savedInput, setSavedInput] = useState(EMPTY_INPUT);
  const [message, setMessage] = useState<Message | null>(null);
  const [loadError, setLoadError] = useState("");
  const [searching, setSearching] = useState(false);
  const [mutating, setMutating] = useState(false);
  const [editing, setEditing] = useState<BibliographyItem | null>(null);
  const [pendingEdit, setPendingEdit] = useState<
    BibliographyItem | "cancel" | null
  >(null);
  const [pendingDelete, setPendingDelete] = useState<BibliographyItem | null>(
    null,
  );
  const [deleteProblem, setDeleteProblem] = useState<string | null>(null);
  const activeSearch = useRef<AbortController | null>(null);
  const dirty = input !== savedInput;

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
  useEffect(() => {
    const warnAboutUnsavedChanges = (event: BeforeUnloadEvent) => {
      if (dirty) event.preventDefault();
    };
    window.addEventListener("beforeunload", warnAboutUnsavedChanges);
    return () =>
      window.removeEventListener("beforeunload", warnAboutUnsavedChanges);
  }, [dirty]);

  function beginEditing(item: BibliographyItem) {
    const value = JSON.stringify(item.csl_json, null, 2);
    setEditing(item);
    setInput(value);
    setSavedInput(value);
    setMessage(notice(`${item.citation_key}を編集中です。`));
  }

  function cancelEditing() {
    setEditing(null);
    setInput(EMPTY_INPUT);
    setSavedInput(EMPTY_INPUT);
    setMessage(notice("編集を取り消しました。"));
  }

  function requestEditing(item: BibliographyItem) {
    if (editing?.item_id === item.item_id) return;
    if (dirty) {
      setPendingEdit(item);
    } else {
      beginEditing(item);
    }
  }

  function requestCancelEditing() {
    if (dirty) {
      setPendingEdit("cancel");
    } else {
      cancelEditing();
    }
  }

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
        setMessage(notice("書誌情報を更新しました。"));
      } else {
        await addBibliographyItem(
          config.apiBase,
          value as Record<string, unknown>,
        );
        setMessage(notice("書誌情報を登録しました。"));
      }
      setEditing(null);
      setInput(EMPTY_INPUT);
      setSavedInput(EMPTY_INPUT);
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
      if (editing?.item_id === item.item_id) {
        setEditing(null);
        setInput(EMPTY_INPUT);
        setSavedInput(EMPTY_INPUT);
      }
      setPendingDelete(null);
      setDeleteProblem(null);
      setMessage(notice("書誌情報を削除しました。"));
      await load(query);
    } catch {
      setDeleteProblem("書誌情報を削除できませんでした。");
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
      <BibliographyImportPanel
        apiBase={config.apiBase}
        onApplied={() => load(query)}
      />
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
              onClick={requestCancelEditing}
            >
              取消
            </button>
          )}
        </div>
      </form>
      {message &&
        pendingDelete === null &&
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
                onClick={() => requestEditing(item)}
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
                  onClick={() => {
                    setPendingDelete(item);
                    setDeleteProblem(null);
                  }}
                >
                  削除
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {pendingEdit !== null && (
        <ConfirmationDialog
          id="discard-bibliography-edit"
          eyebrow="未保存の変更"
          heading="編集中の内容を破棄しますか"
          description="保存していないCSL-JSONは元に戻せません。"
          busy={false}
          problem={null}
          confirmLabel="変更を破棄"
          busyLabel="変更を破棄しています…"
          confirmClassName="button button-danger"
          onCancel={() => setPendingEdit(null)}
          onConfirm={() => {
            const next = pendingEdit;
            setPendingEdit(null);
            if (next === "cancel") cancelEditing();
            else beginEditing(next);
          }}
        />
      )}
      {pendingDelete !== null && (
        <ConfirmationDialog
          id="delete-bibliography-item"
          eyebrow="書誌情報の削除"
          heading={`${pendingDelete.citation_key}を削除しますか`}
          description="書誌情報の削除は取り消せません。"
          busy={mutating}
          problem={deleteProblem}
          confirmLabel="削除する"
          busyLabel="削除しています…"
          confirmClassName="button button-danger"
          onCancel={() => {
            setPendingDelete(null);
            setDeleteProblem(null);
          }}
          onConfirm={() => void remove(pendingDelete)}
        />
      )}
    </section>
  );
}
