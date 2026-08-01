import { useCallback, useState } from "react";

import { ApplicationConfig, NoteSummary, NoteView, readNoteView } from "../api";
import { RenderedContent } from "../RenderedContent";
import {
  accessPath,
  canonicalSearch,
  editPath,
  externalPath,
  graphPath,
  notePath,
} from "../paths";
import { useApiResource } from "../useApiResource";

export function NoteViewPage({
  config,
  noteId,
}: {
  config: ApplicationConfig;
  noteId: string;
}) {
  const [copyStatus, setCopyStatus] = useState<"idle" | "success" | "failure">(
    "idle",
  );
  const load = useCallback(
    (signal: AbortSignal) => readNoteView(config.apiBase, noteId, signal),
    [config.apiBase, noteId],
  );
  const resource = useApiResource(load);
  const view = resource.status === "ready" ? resource.value : null;
  const failed = resource.status === "failed";
  async function copyNoteId() {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("unavailable");
      await navigator.clipboard.writeText(view?.note.note_id ?? noteId);
      setCopyStatus("success");
    } catch {
      setCopyStatus("failure");
    }
  }
  if (failed)
    return (
      <p className="problem-inline" role="alert">
        ノートを読み込めませんでした。
      </p>
    );
  if (view === null)
    return (
      <p className="state-message" role="status">
        ノートを読み込んでいます。
      </p>
    );
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
              href={editPath(config, noteId)}
            >
              編集
            </a>
          )}
          {view.access === "manage" && (
            <a
              className="button button-secondary"
              href={accessPath(config, noteId)}
            >
              共有設定
            </a>
          )}
          {/* 2階層まで開くのは、参照先と参照元の一覧では見えない範囲から始めるためである。 */}
          <a
            className="button button-secondary"
            href={graphPath(config, { noteId, depth: 2 })}
          >
            周辺の関係
          </a>
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
            <p className="state-message">{label}のノートはありません。</p>
          ) : (
            <ul>
              {notes.map((note) => (
                <li key={note.note_id}>
                  <a href={notePath(config, note.note_id)}>{note.title}</a>
                </li>
              ))}
            </ul>
          )}
        </section>
      ))}
    </aside>
  );
}
