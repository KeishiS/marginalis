import { useCallback, useRef, useState } from "react";

import {
  ApplicationConfig,
  deleteNote,
  markNoteReviewed,
  NoteReview,
  NoteSummary,
  NoteView,
  readNoteView,
} from "../api";
import { NoteDeletionDialog } from "../NoteDeletionDialog";
import { noteDeletionProblem } from "../noteLifecyclePresentation";
import { RenderedContent } from "../RenderedContent";
import {
  accessPath,
  canonicalSearch,
  editPath,
  externalPath,
  graphPath,
  listNoticePath,
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
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteProblem, setDeleteProblem] = useState<string | null>(null);
  const [review, setReview] = useState<NoteReview | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [reviewProblem, setReviewProblem] = useState<string | null>(null);
  const deleteButton = useRef<HTMLButtonElement>(null);
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
  function openDeleteDialog() {
    setDeleteProblem(null);
    setDeleteOpen(true);
  }
  function closeDeleteDialog() {
    setDeleteOpen(false);
    setDeleteProblem(null);
    deleteButton.current?.focus();
  }
  async function confirmDelete() {
    if (view === null || deleting) return;
    setDeleting(true);
    setDeleteProblem(null);
    try {
      await deleteNote(
        config.apiBase,
        view.note.note_id,
        review?.current_revision ?? view.note.revision,
      );
      window.location.assign(listNoticePath(config, "note-deleted"));
    } catch (error: unknown) {
      setDeleteProblem(noteDeletionProblem(error));
      setDeleting(false);
    }
  }
  async function confirmReview() {
    if (view === null || reviewing) return;
    setReviewing(true);
    setReviewProblem(null);
    try {
      setReview(
        await markNoteReviewed(
          config.apiBase,
          view.note.note_id,
          review?.current_revision ?? view.note.revision,
        ),
      );
    } catch {
      setReviewProblem(
        "確認済みにできませんでした。ノートを再読み込みしてからお試しください。",
      );
    } finally {
      setReviewing(false);
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
          <dl className="note-provenance">
            <div>
              <dt>作成経路</dt>
              <dd>{creationSourceLabel(view.note.created_via)}</dd>
            </div>
            <div>
              <dt>人手確認</dt>
              <dd>
                {reviewStatusLabel(review?.status ?? view.note.review_status)}
              </dd>
            </div>
          </dl>
          {reviewProblem && (
            <p className="problem-inline" role="alert">
              {reviewProblem}
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
            <>
              {(review?.status ?? view.note.review_status) !== "reviewed" && (
                <button
                  className="button button-secondary"
                  type="button"
                  disabled={reviewing}
                  onClick={() => void confirmReview()}
                >
                  {reviewing ? "確認を記録しています" : "確認済みにする"}
                </button>
              )}
              <a
                className="button button-secondary"
                href={accessPath(config, noteId)}
              >
                共有設定
              </a>
              <button
                ref={deleteButton}
                className="button button-danger"
                type="button"
                onClick={openDeleteDialog}
              >
                削除
              </button>
            </>
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
        <RenderedContent
          html={view.html}
          mathMacros={view.math_macros}
          styleNonce={config.styleNonce}
        />
      </div>
      <RelatedNotes config={config} view={view} />
      {deleteOpen && (
        <NoteDeletionDialog
          title={view.note.title}
          deleting={deleting}
          problem={deleteProblem}
          onCancel={closeDeleteDialog}
          onConfirm={() => void confirmDelete()}
        />
      )}
    </section>
  );
}

function creationSourceLabel(source: NoteView["note"]["created_via"]): string {
  switch (source) {
    case "web":
      return "Web UI";
    case "rest":
      return "REST API";
    case "mcp":
      return "MCP";
    case "unknown":
      return "旧形式（不明）";
  }
}

function reviewStatusLabel(status: NoteView["note"]["review_status"]): string {
  switch (status) {
    case "reviewed":
      return "現在の版を確認済み";
    case "pending":
      return "確認待ち";
    case "unknown":
      return "旧形式（不明）";
  }
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
