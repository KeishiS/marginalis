import { FormEvent, useEffect, useMemo, useState } from "react";

import { AccessControl } from "./AccessControl";
import { EditorApplication } from "./EditorApplication";
import {
  Note,
  NoteListEntry,
  NoteSummary,
  NoteView,
  listNotes,
  readNote,
  readNoteView,
} from "./api";
import { RenderedContent } from "./RenderedContent";
import {
  NoteListQuery,
  noteListSearch,
  parseNoteListQuery,
  selectNoteListPage,
} from "./noteListState";
import { externalPath } from "./paths";

export interface ApplicationConfig {
  apiBase: string;
  basePath: string;
  path: string;
  search: string;
}

type Route =
  | { kind: "list" }
  | { kind: "create" }
  | { kind: "view"; noteId: string }
  | { kind: "edit"; noteId: string }
  | { kind: "access"; noteId: string }
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
    case "not-found":
      return <p role="alert">指定された画面はありません。</p>;
  }
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
    <>
      <div className="editor-heading">
        <h1>ノート</h1>
        <a
          href={`${externalPath(config.basePath, "/notes/new")}${canonicalSearch(config.search)}`}
        >
          新規ノート
        </a>
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
    </>
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
  useEffect(() => {
    const controller = new AbortController();
    readNoteView(config.apiBase, noteId, controller.signal)
      .then(setView)
      .catch(() => !controller.signal.aborted && setFailed(true));
    return () => controller.abort();
  }, [config.apiBase, noteId]);
  if (failed) return <p role="alert">ノートを読み込めませんでした。</p>;
  if (view === null) return <p>ノートを読み込んでいます。</p>;
  return (
    <>
      <nav aria-label="ノート操作">
        <a
          href={externalPath(
            config.basePath,
            `/${canonicalSearch(config.search)}`,
          )}
        >
          一覧
        </a>{" "}
        {view.access !== "read" && (
          <a
            href={`${externalPath(config.basePath, `/notes/${noteId}/edit`)}${canonicalSearch(config.search)}`}
          >
            編集
          </a>
        )}{" "}
        {view.access === "manage" && (
          <a
            href={`${externalPath(config.basePath, `/notes/${noteId}/access`)}${canonicalSearch(config.search)}`}
          >
            共有設定
          </a>
        )}
      </nav>
      <RenderedContent html={view.html} />
      <RelatedNotes config={config} view={view} />
    </>
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
    <>
      <nav aria-label="ノート操作">
        <a
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
    </>
  );
}

function parseRoute(pathname: string): Route {
  if (pathname === "/") return { kind: "list" };
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
