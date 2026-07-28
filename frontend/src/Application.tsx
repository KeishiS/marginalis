import { useEffect, useState } from "react";

import { AccessControl } from "./AccessControl";
import { EditorApplication } from "./EditorApplication";
import {
  Note,
  NoteSummary,
  NoteView,
  listNotes,
  readNote,
  readNoteView,
} from "./api";

export interface ApplicationConfig {
  apiBase: string;
  basePath: string;
  path: string;
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
  const [notes, setNotes] = useState<NoteSummary[] | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let current = true;
    listNotes(config.apiBase)
      .then((value) => current && setNotes(value))
      .catch(() => current && setFailed(true));
    return () => {
      current = false;
    };
  }, [config.apiBase]);
  return (
    <>
      <div className="editor-heading">
        <h1>ノート</h1>
        <a href={path(config.basePath, "/notes/new")}>新規ノート</a>
      </div>
      {failed ? (
        <p role="alert">ノート一覧を読み込めませんでした。</p>
      ) : notes === null ? (
        <p>ノート一覧を読み込んでいます。</p>
      ) : notes.length === 0 ? (
        <p>閲覧できるノートはありません。</p>
      ) : (
        <ul>
          {notes.map((note) => (
            <li key={note.note_id}>
              <a href={path(config.basePath, `/notes/${note.note_id}`)}>
                {note.title}
              </a>
            </li>
          ))}
        </ul>
      )}
    </>
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
        <a href={path(config.basePath, "/")}>一覧</a>{" "}
        {view.access !== "read" && (
          <a href={path(config.basePath, `/notes/${noteId}/edit`)}>編集</a>
        )}{" "}
        {view.access === "manage" && (
          <a href={path(config.basePath, `/notes/${noteId}/access`)}>
            共有設定
          </a>
        )}
      </nav>
      <div dangerouslySetInnerHTML={{ __html: view.html }} />
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
                  <a href={path(config.basePath, `/notes/${note.note_id}`)}>
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
        <a href={path(config.basePath, `/notes/${noteId}`)}>閲覧画面へ戻る</a>
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

function path(basePath: string, suffix: string): string {
  const base = basePath === "/" ? "" : basePath.replace(/\/$/, "");
  return `${base}${suffix}`;
}
