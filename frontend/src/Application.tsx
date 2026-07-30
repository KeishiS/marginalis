import { FormEvent, useEffect, useMemo, useState } from "react";

import { AccessControl } from "./AccessControl";
import { EditorApplication } from "./EditorApplication";
import {
  addBibliographyItem,
  ApplicationConfig,
  BibliographyItem,
  deleteBibliographyItem,
  Note,
  NoteListEntry,
  NoteSummary,
  NoteView,
  listNotes,
  readNote,
  readNoteView,
  searchBibliography,
  updateBibliographyItem,
} from "./api";
import { RenderedContent } from "./RenderedContent";
import {
  NoteListQuery,
  noteListSearch,
  parseNoteListQuery,
  selectNoteListPage,
} from "./noteListState";
import { externalPath } from "./paths";

// 起動設定の形は`marginalis-contract`が定めます。ここでは再定義せず再公開します。
export type { ApplicationConfig };

type Route =
  | { kind: "list" }
  | { kind: "create" }
  | { kind: "view"; noteId: string }
  | { kind: "edit"; noteId: string }
  | { kind: "access"; noteId: string }
  | { kind: "bibliography" }
  | { kind: "not-found" };

export function Application({ config }: { config: ApplicationConfig }) {
  const route = parseRoute(config.path);
  switch (route.kind) {
    case "list":
      return <NoteList config={config} />;
    case "create":
      return (
        <EditorApplication config={{ ...config, mode: "create", noteId: "" }} />
      );
    case "view":
      return <NoteViewer config={config} noteId={route.noteId} />;
    case "edit":
      return (
        <EditorApplication
          config={{ ...config, mode: "edit", noteId: route.noteId }}
        />
      );
    case "access":
      return <AccessPage config={config} noteId={route.noteId} />;
    case "bibliography":
      return <BibliographyLibrary config={config} />;
    case "not-found":
      return <p role="alert">指定された画面はありません。</p>;
  }
}

function BibliographyLibrary({ config }: { config: ApplicationConfig }) {
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
              <div>
                <strong>{item.citation_key}</strong>
                <span>
                  {typeof item.csl_json.title === "string"
                    ? item.csl_json.title
                    : "題名なし"}
                </span>
              </div>
              <button
                className="button button-secondary"
                type="button"
                onClick={() => {
                  setEditing(item);
                  setInput(JSON.stringify(item.csl_json, null, 2));
                  setMessage(`${item.citation_key}を編集中です。`);
                }}
              >
                編集
              </button>
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void remove(item)}
              >
                削除
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function NoteList({ config }: { config: ApplicationConfig }) {
  const [notes, setNotes] = useState<NoteListEntry[] | null>(null);
  const [failed, setFailed] = useState(false);
  const query = useMemo(
    () => parseNoteListQuery(config.search),
    [config.search],
  );
  useEffect(() => {
    let current = true;
    listNotes(config.apiBase)
      .then((value) => current && setNotes(value))
      .catch(() => current && setFailed(true));
    return () => {
      current = false;
    };
  }, [config.apiBase]);
  const page = notes === null ? null : selectNoteListPage(notes, query);
  return (
    <section
      className="note-index page-section"
      aria-labelledby="note-index-heading"
    >
      <div className="page-heading">
        <div>
          <p className="page-eyebrow">Library</p>
          <h1 id="note-index-heading">ノート</h1>
          <p className="page-description">
            記録した知識を、更新日やタグから見つけられます。
          </p>
        </div>
      </div>
      <NoteListFilters config={config} query={query} />
      {failed ? (
        <p role="alert">ノート一覧を読み込めませんでした。</p>
      ) : notes === null ? (
        <p role="status">ノート一覧を読み込んでいます。</p>
      ) : page?.total === 0 ? (
        <p>
          {notes.length === 0
            ? "閲覧できるノートはありません。"
            : "条件に一致するノートはありません。"}
        </p>
      ) : (
        <>
          <p className="list-result-count" role="status">
            {page?.total}件のノート
          </p>
          <ul className="note-list">
            {page?.notes.map((note) => (
              <li key={note.note_id}>
                <a
                  href={`${externalPath(config.basePath, `/notes/${note.note_id}`)}${canonicalSearch(config.search)}`}
                >
                  {note.title}
                </a>
                <dl>
                  <div>
                    <dt>更新</dt>
                    <dd>
                      <time
                        dateTime={new Date(note.updated_at_ms).toISOString()}
                      >
                        {formatDateTime(note.updated_at_ms)}
                      </time>
                    </dd>
                  </div>
                  <div>
                    <dt>アクセス</dt>
                    <dd>{accessLabel(note.access)}</dd>
                  </div>
                </dl>
                {note.tags.length > 0 && (
                  <ul className="tag-list" aria-label="タグ">
                    {note.tags.map((tag) => (
                      <li key={tag}>{tag}</li>
                    ))}
                  </ul>
                )}
              </li>
            ))}
          </ul>
          {page && page.pageCount > 1 && (
            <nav className="pagination" aria-label="ノート一覧のページ">
              {page.page > 1 && (
                <a href={listPath(config, query, page.page - 1)}>前へ</a>
              )}
              <span>
                {page.page} / {page.pageCount}
              </span>
              {page.page < page.pageCount && (
                <a href={listPath(config, query, page.page + 1)}>次へ</a>
              )}
            </nav>
          )}
        </>
      )}
    </section>
  );
}

function NoteListFilters({
  config,
  query,
}: {
  config: ApplicationConfig;
  query: NoteListQuery;
}) {
  function resetPage(event: FormEvent<HTMLFormElement>) {
    const page = event.currentTarget.elements.namedItem("page");
    if (page instanceof HTMLInputElement) page.value = "1";
  }
  return (
    <form
      className="note-list-filters"
      action={externalPath(config.basePath, "/")}
      method="get"
      onSubmit={resetPage}
    >
      <label>
        タグ
        <input
          name="tag"
          type="text"
          defaultValue={query.tags.join(", ")}
          placeholder="research, rust"
        />
      </label>
      <label>
        この日以降に更新
        <input
          name="updated_after"
          type="date"
          defaultValue={query.updatedAfter}
        />
      </label>
      <input name="page" type="hidden" value="1" readOnly />
      <button type="submit">絞り込む</button>
      {(query.tags.length > 0 || query.updatedAfter) && (
        <a href={externalPath(config.basePath, "/")}>条件を解除</a>
      )}
    </form>
  );
}

function NoteViewer({
  config,
  noteId,
}: {
  config: ApplicationConfig;
  noteId: string;
}) {
  const [view, setView] = useState<NoteView | null>(null);
  const [failed, setFailed] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"idle" | "success" | "failure">(
    "idle",
  );
  useEffect(() => {
    const controller = new AbortController();
    readNoteView(config.apiBase, noteId, controller.signal)
      .then(setView)
      .catch(() => !controller.signal.aborted && setFailed(true));
    return () => controller.abort();
  }, [config.apiBase, noteId]);
  async function copyNoteId() {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("unavailable");
      await navigator.clipboard.writeText(view?.note.note_id ?? noteId);
      setCopyStatus("success");
    } catch {
      setCopyStatus("failure");
    }
  }
  if (failed) return <p role="alert">ノートを読み込めませんでした。</p>;
  if (view === null) return <p>ノートを読み込んでいます。</p>;
  return (
    <section className="note-viewer" aria-label="ノートの閲覧">
      <div className="note-view-toolbar surface">
        <div className="note-identity">
          <span className="note-identity-label">note ID</span>
          <div className="note-identity-value">
            <code>{view.note.note_id}</code>
            <button
              className="button button-secondary button-small"
              type="button"
              aria-label="note IDをコピー"
              onClick={() => void copyNoteId()}
            >
              コピー
            </button>
            {view.note.tags.length > 0 && (
              <ul className="tag-list note-view-tags" aria-label="ノートのタグ">
                {view.note.tags.map((tag) => (
                  <li key={tag}>{tag}</li>
                ))}
              </ul>
            )}
          </div>
          {copyStatus !== "idle" && (
            <p
              className={`copy-feedback copy-feedback-${copyStatus}`}
              role={copyStatus === "failure" ? "alert" : "status"}
            >
              {copyStatus === "success"
                ? "note IDをコピーしました。"
                : "note IDをコピーできませんでした。"}
            </p>
          )}
        </div>
        <nav className="page-actions" aria-label="ノート操作">
          <a
            className="button button-secondary"
            href={externalPath(
              config.basePath,
              `/${canonicalSearch(config.search)}`,
            )}
          >
            一覧
          </a>
          {view.access !== "read" && (
            <a
              className="button button-primary"
              href={`${externalPath(config.basePath, `/notes/${noteId}/edit`)}${canonicalSearch(config.search)}`}
            >
              編集
            </a>
          )}
          {view.access === "manage" && (
            <a
              className="button button-secondary"
              href={`${externalPath(config.basePath, `/notes/${noteId}/access`)}${canonicalSearch(config.search)}`}
            >
              共有設定
            </a>
          )}
        </nav>
      </div>
      <div className="document-surface">
        <RenderedContent html={view.html} styleNonce={config.styleNonce} />
      </div>
      <RelatedNotes config={config} view={view} />
    </section>
  );
}

function RelatedNotes({
  config,
  view,
}: {
  config: ApplicationConfig;
  view: NoteView;
}) {
  const groups: [string, NoteSummary[]][] = [
    ["参照先", view.related.outgoing],
    ["参照元", view.related.incoming],
  ];
  return (
    <aside className="related-notes" aria-label="関連ノート">
      {groups.map(([label, notes]) => (
        <section key={label}>
          <h2>{label}</h2>
          {notes.length === 0 ? (
            <p>ありません。</p>
          ) : (
            <ul>
              {notes.map((note) => (
                <li key={note.note_id}>
                  <a
                    href={`${externalPath(config.basePath, `/notes/${note.note_id}`)}${canonicalSearch(config.search)}`}
                  >
                    {note.title}
                  </a>
                </li>
              ))}
            </ul>
          )}
        </section>
      ))}
    </aside>
  );
}

function AccessPage({
  config,
  noteId,
}: {
  config: ApplicationConfig;
  noteId: string;
}) {
  const [note, setNote] = useState<Note | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    readNote(config.apiBase, noteId)
      .then(setNote)
      .catch(() => setFailed(true));
  }, [config.apiBase, noteId]);
  if (failed) return <p role="alert">共有設定を読み込めませんでした。</p>;
  if (note === null) return <p>共有設定を読み込んでいます。</p>;
  return (
    <section
      className="access-page page-section"
      aria-labelledby="access-page-heading"
    >
      <div className="page-heading">
        <div>
          <p className="page-eyebrow">Access</p>
          <h1 id="access-page-heading">共有設定</h1>
          <p className="page-description">
            このノートを閲覧または編集できる利用者を管理します。
          </p>
        </div>
      </div>
      <nav className="page-actions" aria-label="ノート操作">
        <a
          className="button button-secondary"
          href={`${externalPath(config.basePath, `/notes/${noteId}`)}${canonicalSearch(config.search)}`}
        >
          閲覧画面へ戻る
        </a>
      </nav>
      <AccessControl
        apiBase={config.apiBase}
        noteId={noteId}
        revision={note.revision}
        onRevision={(revision) => setNote({ ...note, revision })}
      />
    </section>
  );
}

function parseRoute(pathname: string): Route {
  if (pathname === "/") return { kind: "list" };
  if (pathname === "/bibliography") return { kind: "bibliography" };
  if (pathname === "/notes/new") return { kind: "create" };
  const match = pathname.match(/^\/notes\/([^/]+)(?:\/(edit|access))?$/);
  if (!match) return { kind: "not-found" };
  const noteId = decodeURIComponent(match[1]);
  if (match[2] === "edit") return { kind: "edit", noteId };
  if (match[2] === "access") return { kind: "access", noteId };
  return { kind: "view", noteId };
}

function listPath(
  config: ApplicationConfig,
  query: NoteListQuery,
  page: number,
): string {
  return externalPath(config.basePath, `/${noteListSearch(query, page)}`);
}

function canonicalSearch(search: string): string {
  return noteListSearch(parseNoteListQuery(search));
}

function accessLabel(access: NoteListEntry["access"]): string {
  switch (access) {
    case "read":
      return "閲覧";
    case "edit":
      return "編集";
    case "manage":
      return "所有";
  }
}

function formatDateTime(timestamp: number): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(timestamp));
}
