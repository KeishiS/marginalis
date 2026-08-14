import { FormEvent, useCallback, useEffect, useRef, useState } from "react";

import {
  addBibliographyItem,
  ApplicationConfig,
  BibliographyItem,
  deleteBibliographyItem,
  searchBibliography,
  updateBibliographyItem,
} from "../api";
import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

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
    <section className="grid gap-6">
      <PageHeader
        eyebrow="Bibliography"
        title="書誌ライブラリー"
        description="CSL-JSON形式の文献情報を、ノートとは独立して管理します。"
      />
      {/* ノート一覧の絞り込みと同じく、広い画面では入力欄の隣へ内容幅のボタンを置く。 */}
      <form
        className="grid items-stretch gap-3 min-[60rem]:flex min-[60rem]:items-end"
        onSubmit={(event) => {
          event.preventDefault();
          if (mutating) return;
          void load(query);
        }}
      >
        <label className="grid gap-2 font-semibold min-[60rem]:flex-1">
          文献を検索
          <Input
            disabled={mutating}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="citation key、題名、著者、DOI"
          />
        </label>
        <Button variant="outline" type="submit" disabled={mutating}>
          {searching ? "検索しています…" : "検索"}
        </Button>
      </form>
      <BibliographyImportPanel
        apiBase={config.apiBase}
        onApplied={() => load(query)}
      />
      <form className="grid gap-3" onSubmit={(event) => void submit(event)}>
        <label className="grid gap-2 font-semibold">
          CSL-JSON
          <textarea
            className="w-full resize-y rounded-md border bg-card p-3 font-mono text-sm"
            rows={12}
            value={input}
            onChange={(event) => setInput(event.target.value)}
            spellCheck={false}
            disabled={mutating}
          />
        </label>
        <div className="flex flex-wrap gap-2">
          <Button type="submit" disabled={mutating}>
            {mutating ? "処理しています…" : editing ? "更新" : "登録"}
          </Button>
          {editing && (
            <Button
              variant="outline"
              type="button"
              disabled={mutating}
              onClick={requestCancelEditing}
            >
              取消
            </Button>
          )}
        </div>
      </form>
      {message &&
        pendingDelete === null &&
        (message.failed ? (
          <ProblemAlert>{message.text}</ProblemAlert>
        ) : (
          <StatusMessage>{message.text}</StatusMessage>
        ))}
      {loadError && <ProblemAlert>{loadError}</ProblemAlert>}
      {items === null ? (
        <StatusMessage>書誌情報を読み込んでいます。</StatusMessage>
      ) : items.length === 0 ? (
        <StatusMessage>登録済みの書誌情報はありません。</StatusMessage>
      ) : (
        <ul className="m-0 grid list-none gap-3 p-0">
          {items.map((item) => (
            <li
              key={item.item_id}
              className="flex items-center justify-between gap-4 rounded-md border bg-card p-4"
            >
              {/* カードの情報部分そのものを、編集を始める操作にする。
                  編集中は色だけでなく左端の帯でも示す。 */}
              <button
                className="grid min-w-0 cursor-pointer gap-1 rounded-sm border-0 bg-transparent p-1 text-start hover:bg-muted aria-[current=true]:border-l-4 aria-[current=true]:border-solid aria-[current=true]:border-l-primary aria-[current=true]:pl-3"
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
              <div className="flex flex-wrap gap-2">
                <Button
                  variant="destructive"
                  type="button"
                  disabled={mutating}
                  onClick={() => {
                    setPendingDelete(item);
                    setDeleteProblem(null);
                  }}
                >
                  削除
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {pendingEdit !== null && (
        <ConfirmationDialog
          eyebrow="未保存の変更"
          heading="編集中の内容を破棄しますか"
          description="保存していないCSL-JSONは元に戻せません。"
          busy={false}
          problem={null}
          confirmLabel="変更を破棄"
          busyLabel="変更を破棄しています…"
          destructive
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
          eyebrow="書誌情報の削除"
          heading={`${pendingDelete.citation_key}を削除しますか`}
          description="書誌情報の削除は取り消せません。"
          busy={mutating}
          problem={deleteProblem}
          confirmLabel="削除する"
          busyLabel="削除しています…"
          destructive
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
