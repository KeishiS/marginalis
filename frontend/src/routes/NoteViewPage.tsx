import { useCallback, useRef, useState } from "react";

import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

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
  historyPath,
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
    // 確認画面のフォーカストラップが解除されてから削除ボタンへ戻す。
    setTimeout(() => deleteButton.current?.focus(), 0);
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
    return <ProblemAlert>ノートを読み込めませんでした。</ProblemAlert>;
  if (view === null)
    return <StatusMessage>ノートを読み込んでいます。</StatusMessage>;
  return (
    <section className="grid min-w-0 gap-5" aria-label="ノートの閲覧">
      <div className="flex w-full min-w-0 flex-wrap items-stretch justify-between gap-4 rounded-md border bg-card px-4 py-3 shadow-xs min-[60rem]:items-start">
        <div className="grid min-w-0 gap-1">
          <span className="text-xs font-bold tracking-wide text-muted-foreground">
            note ID
          </span>
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <code className="max-w-full rounded-sm bg-muted p-2 font-mono text-sm [overflow-wrap:anywhere] select-all">
              {view.note.note_id}
            </code>
            <Button
              variant="outline"
              size="sm"
              type="button"
              aria-label="note IDをコピー"
              onClick={() => void copyNoteId()}
            >
              コピー
            </Button>
            {view.note.tags.length > 0 && (
              <ul
                className="m-0 flex list-none flex-wrap gap-2 p-0"
                aria-label="ノートのタグ"
              >
                {view.note.tags.map((tag) => (
                  <li key={tag}>
                    <Badge variant="secondary">{tag}</Badge>
                  </li>
                ))}
              </ul>
            )}
          </div>
          {copyStatus !== "idle" && (
            <p
              className={`m-0 text-xs ${
                copyStatus === "success" ? "text-success" : "text-destructive"
              }`}
              role={copyStatus === "failure" ? "alert" : "status"}
            >
              {copyStatus === "success"
                ? "note IDをコピーしました。"
                : "note IDをコピーできませんでした。"}
            </p>
          )}
          <dl className="m-0 flex flex-wrap gap-x-5 gap-y-1 text-sm">
            <div className="flex gap-1">
              <dt className="m-0 font-semibold text-muted-foreground">
                作成経路
              </dt>
              <dd className="m-0">
                {creationSourceLabel(view.note.created_via)}
              </dd>
            </div>
            <div className="flex gap-1">
              <dt className="m-0 font-semibold text-muted-foreground">
                人手確認
              </dt>
              <dd className="m-0">
                {reviewStatusLabel(review?.status ?? view.note.review_status)}
              </dd>
            </div>
          </dl>
          {reviewProblem && <ProblemAlert>{reviewProblem}</ProblemAlert>}
        </div>
        <nav
          className="flex w-full flex-wrap items-start gap-2 min-[60rem]:w-auto"
          aria-label="ノート操作"
        >
          <Button variant="outline" asChild>
            <a
              href={externalPath(
                config.basePath,
                `/${canonicalSearch(config.search)}`,
              )}
            >
              一覧
            </a>
          </Button>
          {view.access !== "read" && (
            <Button asChild>
              <a href={editPath(config, noteId)}>編集</a>
            </Button>
          )}
          <Button variant="outline" asChild>
            <a href={historyPath(config, noteId)}>版履歴</a>
          </Button>
          {view.access === "manage" && (
            <>
              {(review?.status ?? view.note.review_status) !== "reviewed" && (
                <Button
                  variant="outline"
                  type="button"
                  disabled={reviewing}
                  onClick={() => void confirmReview()}
                >
                  {reviewing ? "確認を記録しています" : "確認済みにする"}
                </Button>
              )}
              <Button variant="outline" asChild>
                <a href={accessPath(config, noteId)}>共有設定</a>
              </Button>
              <Button
                ref={deleteButton}
                variant="destructive"
                type="button"
                onClick={openDeleteDialog}
              >
                削除
              </Button>
            </>
          )}
          {/* 2階層まで開くのは、参照先と参照元の一覧では見えない範囲から始めるためである。 */}
          <Button variant="outline" asChild>
            <a href={graphPath(config, { noteId, depth: 2 })}>周辺の関係</a>
          </Button>
        </nav>
      </div>
      <div className="w-full min-w-0 rounded-lg border bg-card p-[clamp(var(--space-5),4vw,var(--space-12))] shadow-xs">
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
    <aside
      className="mt-8 grid w-full min-w-0 grid-cols-1 gap-4 min-[60rem]:grid-cols-2"
      aria-label="関連ノート"
    >
      {groups.map(([label, notes]) => (
        <section key={label} className="rounded-md border bg-card p-4">
          <h2 className="mt-0 mb-3 text-base font-bold">{label}</h2>
          {notes.length === 0 ? (
            <StatusMessage>{label}のノートはありません。</StatusMessage>
          ) : (
            <ul className="m-0 grid list-none gap-2 p-0">
              {notes.map((note) => (
                <li key={note.note_id} className="rounded-sm bg-muted p-3">
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
