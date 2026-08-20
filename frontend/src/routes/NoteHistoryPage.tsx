import { useCallback, useEffect, useState } from "react";

import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { Button } from "@/components/ui/button";

import {
  ApplicationConfig,
  compareNoteRevisions,
  listNoteRevisions,
  NoteRevision,
  NoteRevisionDiff,
  NoteRevisionSummary,
  readNoteRevision,
  restoreNoteRevision,
} from "../api";
import { notePath } from "../paths";
import { useApiResource } from "../useApiResource";

interface HistoryResource {
  revisions: NoteRevisionSummary[];
  current: NoteRevision;
}

export function NoteHistoryPage({
  config,
  noteId,
}: {
  config: ApplicationConfig;
  noteId: string;
}) {
  const load = useCallback(
    async (signal: AbortSignal): Promise<HistoryResource> => {
      const revisions = await listNoteRevisions(config.apiBase, noteId, signal);
      if (revisions.length === 0) throw new Error("history is empty");
      const current = await readNoteRevision(
        config.apiBase,
        noteId,
        revisions[0].revision,
        signal,
      );
      return { revisions, current };
    },
    [config.apiBase, noteId],
  );
  const resource = useApiResource(load);
  const [selected, setSelected] = useState<number | null>(null);
  const [snapshot, setSnapshot] = useState<NoteRevision | null>(null);
  const [fromRevision, setFromRevision] = useState<number | null>(null);
  const [toRevision, setToRevision] = useState<number | null>(null);
  const [diff, setDiff] = useState<NoteRevisionDiff | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const data = resource.status === "ready" ? resource.value : null;
  const latestRevision = data?.revisions[0]?.revision ?? null;
  const defaultSelectedRevision =
    data?.revisions[1]?.revision ?? latestRevision;
  const selectedRevision = selected ?? defaultSelectedRevision;
  const effectiveFromRevision = fromRevision ?? defaultSelectedRevision;
  const effectiveToRevision = toRevision ?? latestRevision;
  const visibleSnapshot =
    snapshot?.note.note_id === noteId &&
    snapshot.note.revision === selectedRevision
      ? snapshot
      : null;

  useEffect(() => {
    if (selectedRevision === null) return;
    const controller = new AbortController();
    readNoteRevision(
      config.apiBase,
      noteId,
      selectedRevision,
      controller.signal,
    )
      .then(setSnapshot)
      .catch(() => {
        if (!controller.signal.aborted)
          setProblem("選択した版を読み込めませんでした。");
      });
    return () => controller.abort();
  }, [config.apiBase, noteId, selectedRevision]);

  async function showDiff() {
    if (
      effectiveFromRevision === null ||
      effectiveToRevision === null ||
      working
    )
      return;
    setWorking(true);
    setProblem(null);
    try {
      setDiff(
        await compareNoteRevisions(
          config.apiBase,
          noteId,
          effectiveFromRevision,
          effectiveToRevision,
        ),
      );
    } catch {
      setProblem("版の差分を読み込めませんでした。");
    } finally {
      setWorking(false);
    }
  }

  async function restoreSelected() {
    if (data === null || selectedRevision === null || working) return;
    if (
      !window.confirm(
        `rev-${selectedRevision}の本文を新しい版として復元しますか？`,
      )
    )
      return;
    setWorking(true);
    setProblem(null);
    try {
      await restoreNoteRevision(
        config.apiBase,
        noteId,
        selectedRevision,
        data.current.note.revision,
      );
      window.location.assign(notePath(config, noteId));
    } catch {
      setProblem(
        "復元できませんでした。現在の版が変わっていないか確認してから、もう一度お試しください。",
      );
      setWorking(false);
    }
  }

  if (resource.status === "failed")
    return <ProblemAlert>版履歴を読み込めませんでした。</ProblemAlert>;
  if (data === null)
    return <StatusMessage>版履歴を読み込んでいます。</StatusMessage>;

  return (
    <section className="grid gap-5" aria-labelledby="history-heading">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p className="m-0 text-sm text-muted-foreground">
            {data.current.note.title}
          </p>
          <h1 id="history-heading" className="m-0 text-2xl font-bold">
            版履歴
          </h1>
        </div>
        <Button variant="outline" asChild>
          <a href={notePath(config, noteId)}>ノートへ戻る</a>
        </Button>
      </header>
      {problem && <ProblemAlert>{problem}</ProblemAlert>}
      <div className="grid gap-5 min-[60rem]:grid-cols-[minmax(16rem,22rem)_1fr]">
        <section
          className="rounded-md border bg-card p-4"
          aria-labelledby="revision-list-heading"
        >
          <h2 id="revision-list-heading" className="mt-0 text-lg font-bold">
            保存された版
          </h2>
          <ol className="m-0 grid list-none gap-2 p-0">
            {data.revisions.map((revision) => (
              <li key={revision.revision}>
                <button
                  type="button"
                  className={`w-full rounded-md border p-3 text-left ${selectedRevision === revision.revision ? "border-primary bg-muted" : "bg-background"}`}
                  aria-pressed={selectedRevision === revision.revision}
                  onClick={() => {
                    setSelected(revision.revision);
                    setProblem(null);
                  }}
                >
                  <span className="block font-bold">
                    rev-{revision.revision}
                  </span>
                  <span className="block text-sm">
                    {revisionKindLabel(revision.kind)}
                  </span>
                  <time
                    className="block text-xs text-muted-foreground"
                    dateTime={new Date(revision.changed_at_ms).toISOString()}
                  >
                    {new Date(revision.changed_at_ms).toLocaleString("ja-JP")}
                  </time>
                  <span className="block text-xs text-muted-foreground">
                    {revision.changed_by_subject}
                  </span>
                  <span className="block truncate text-xs text-muted-foreground">
                    {revision.changed_by_issuer}
                  </span>
                </button>
              </li>
            ))}
          </ol>
        </section>
        <div className="grid min-w-0 gap-5">
          <section
            className="rounded-md border bg-card p-4"
            aria-labelledby="revision-source-heading"
          >
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h2
                id="revision-source-heading"
                className="m-0 text-lg font-bold"
              >
                {selectedRevision === null
                  ? "版の内容"
                  : `rev-${selectedRevision}の原文`}
              </h2>
              {selectedRevision !== null &&
                selectedRevision !== data.current.note.revision &&
                data.current.access !== "read" &&
                data.current.deleted_at_ms === null && (
                  <Button
                    type="button"
                    disabled={working}
                    onClick={() => void restoreSelected()}
                  >
                    この版を復元
                  </Button>
                )}
            </div>
            {visibleSnapshot === null ? (
              <StatusMessage>版の内容を読み込んでいます。</StatusMessage>
            ) : (
              <pre className="max-h-[36rem] overflow-auto whitespace-pre-wrap rounded-md bg-muted p-4 font-mono text-sm">
                {visibleSnapshot.note.source}
              </pre>
            )}
          </section>
          <section
            className="rounded-md border bg-card p-4"
            aria-labelledby="revision-diff-heading"
          >
            <h2 id="revision-diff-heading" className="mt-0 text-lg font-bold">
              版を比較
            </h2>
            <div className="flex flex-wrap items-end gap-3">
              <RevisionSelect
                label="比較元"
                value={effectiveFromRevision}
                revisions={data.revisions}
                onChange={(revision) => {
                  setFromRevision(revision);
                  setDiff(null);
                }}
              />
              <RevisionSelect
                label="比較先"
                value={effectiveToRevision}
                revisions={data.revisions}
                onChange={(revision) => {
                  setToRevision(revision);
                  setDiff(null);
                }}
              />
              <Button
                type="button"
                variant="outline"
                disabled={working}
                onClick={() => void showDiff()}
              >
                差分を表示
              </Button>
            </div>
            {diff && (
              <pre
                className="mt-4 max-h-[36rem] overflow-auto whitespace-pre rounded-md bg-muted p-4 font-mono text-sm"
                aria-label="行単位の差分"
              >
                {diff.unified_diff || "本文に差はありません。"}
              </pre>
            )}
          </section>
        </div>
      </div>
    </section>
  );
}

function RevisionSelect({
  label,
  value,
  revisions,
  onChange,
}: {
  label: string;
  value: number | null;
  revisions: NoteRevisionSummary[];
  onChange: (revision: number) => void;
}) {
  return (
    <label className="grid gap-1 text-sm font-semibold">
      {label}
      <select
        className="rounded-md border bg-background px-3 py-2"
        value={value ?? ""}
        onChange={(event) => onChange(Number(event.target.value))}
      >
        {revisions.map((revision) => (
          <option key={revision.revision} value={revision.revision}>
            rev-{revision.revision}
          </option>
        ))}
      </select>
    </label>
  );
}

function revisionKindLabel(kind: NoteRevisionSummary["kind"]): string {
  switch (kind) {
    case "created":
      return "作成";
    case "content_updated":
      return "本文の更新";
    case "acl_updated":
      return "共有設定の更新";
    case "reviewed":
      return "人手確認";
    case "deleted":
      return "削除";
    case "restored":
      return "削除から復元";
    case "history_restored":
      return "過去版から復元";
    case "imported":
      return "履歴導入時の版";
  }
}
