import { useCallback, useRef, useState } from "react";

import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";

import {
  ApplicationConfig,
  DeletedNoteListEntry,
  listDeletedNotes,
  restoreNote,
} from "../api";
import { ConfirmationDialog } from "../ConfirmationDialog";
import { formatDateTime } from "../formatting";
import { noteRestorationProblem } from "../noteLifecyclePresentation";
import { noteRetentionStatus } from "../noteRetention";
import { listNoticePath, listPath } from "../paths";
import { useApiResource } from "../useApiResource";

export function DeletedNotesPage({
  config,
  navigate = (path) => window.location.assign(path),
}: {
  config: ApplicationConfig;
  navigate?: (path: string) => void;
}) {
  const load = useCallback(
    (signal: AbortSignal) => listDeletedNotes(config.apiBase, signal),
    [config.apiBase],
  );
  const resource = useApiResource(load);
  const notes = resource.status === "ready" ? resource.value : null;
  const failed = resource.status === "failed";
  const [selected, setSelected] = useState<DeletedNoteListEntry | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const restoreTrigger = useRef<HTMLButtonElement | null>(null);

  function openRestoreDialog(
    note: DeletedNoteListEntry,
    trigger: HTMLButtonElement,
  ) {
    restoreTrigger.current = trigger;
    setSelected(note);
    setProblem(null);
  }

  function closeRestoreDialog() {
    setSelected(null);
    setProblem(null);
    // 確認画面のフォーカストラップが解除されてから復元ボタンへ戻す。
    setTimeout(() => restoreTrigger.current?.focus(), 0);
  }

  async function confirmRestore() {
    if (selected === null || restoring) return;
    setRestoring(true);
    setProblem(null);
    try {
      await restoreNote(config.apiBase, selected.note_id, selected.revision);
      navigate(listNoticePath(config, "note-restored"));
    } catch (error: unknown) {
      setProblem(noteRestorationProblem(error));
      setRestoring(false);
    }
  }

  return (
    <section className="grid gap-6" aria-labelledby="deleted-heading">
      <PageHeader
        eyebrow="Recently deleted"
        title="削除済みノート"
        titleId="deleted-heading"
        description="所有するノートは、削除後30日以内であれば復元できます。"
      >
        <Button variant="outline" asChild>
          <a href={listPath(config)}>ノート一覧へ戻る</a>
        </Button>
      </PageHeader>
      {failed ? (
        <ProblemAlert>
          削除済みノートを読み込めませんでした。接続を確認して画面を再読み込みしてください。
        </ProblemAlert>
      ) : notes === null ? (
        <StatusMessage>削除済みノートを読み込んでいます。</StatusMessage>
      ) : notes.length === 0 ? (
        <StatusMessage>削除済みノートはありません。</StatusMessage>
      ) : (
        <ul className="m-0 grid list-none gap-3 p-0">
          {notes.map((note) => {
            const retention = noteRetentionStatus(note.purge_at_ms);
            return (
              <li
                key={note.note_id}
                className="rounded-md border bg-card px-5 py-4 shadow-xs"
              >
                <h2 className="m-0 text-base font-bold [overflow-wrap:anywhere]">
                  {note.title}
                </h2>
                <dl className="my-2 flex flex-wrap gap-x-5 gap-y-2 text-sm">
                  <div className="flex gap-1">
                    <dt className="m-0 font-semibold text-muted-foreground">
                      削除
                    </dt>
                    <dd className="m-0">
                      <time
                        dateTime={new Date(note.deleted_at_ms).toISOString()}
                      >
                        {formatDateTime(note.deleted_at_ms)}
                      </time>
                    </dd>
                  </div>
                  <div className="flex gap-1">
                    <dt className="m-0 font-semibold text-muted-foreground">
                      完全削除予定
                    </dt>
                    <dd className="m-0">
                      <time dateTime={new Date(note.purge_at_ms).toISOString()}>
                        {formatDateTime(note.purge_at_ms)}
                      </time>
                    </dd>
                  </div>
                  <div className="flex gap-1">
                    <dt className="m-0 font-semibold text-muted-foreground">
                      revision
                    </dt>
                    <dd className="m-0">rev-{note.revision}</dd>
                  </div>
                </dl>
                <p className="mt-0 mb-3 text-sm text-muted-foreground">
                  {retention.label}
                </p>
                <Button
                  type="button"
                  onClick={(event) =>
                    openRestoreDialog(note, event.currentTarget)
                  }
                >
                  復元
                </Button>
              </li>
            );
          })}
        </ul>
      )}
      {selected !== null && (
        <ConfirmationDialog
          eyebrow="Restore note"
          heading="このノートを復元しますか？"
          description={
            <>
              「<strong>{selected.title}</strong>
              」を通常のノート一覧へ戻します。
            </>
          }
          busy={restoring}
          problem={problem}
          confirmLabel="復元する"
          busyLabel="復元しています…"
          onCancel={closeRestoreDialog}
          onConfirm={() => void confirmRestore()}
        />
      )}
    </section>
  );
}
