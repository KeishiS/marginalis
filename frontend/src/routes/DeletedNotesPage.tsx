import { useCallback, useRef, useState } from "react";

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
    restoreTrigger.current?.focus();
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
    <section
      className="note-index page-section"
      aria-labelledby="deleted-heading"
    >
      <div className="page-heading">
        <div>
          <p className="page-eyebrow">Recently deleted</p>
          <h1 id="deleted-heading">削除済みノート</h1>
          <p className="page-description">
            所有するノートは、削除後30日以内であれば復元できます。
          </p>
        </div>
        <a className="button button-secondary" href={listPath(config)}>
          ノート一覧へ戻る
        </a>
      </div>
      {failed ? (
        <p className="problem-inline" role="alert">
          削除済みノートを読み込めませんでした。接続を確認して画面を再読み込みしてください。
        </p>
      ) : notes === null ? (
        <p className="state-message" role="status">
          削除済みノートを読み込んでいます。
        </p>
      ) : notes.length === 0 ? (
        <p className="state-message">削除済みノートはありません。</p>
      ) : (
        <ul className="note-list deleted-note-list">
          {notes.map((note) => {
            const retention = noteRetentionStatus(note.purge_at_ms);
            return (
              <li key={note.note_id}>
                <h2>{note.title}</h2>
                <dl>
                  <div>
                    <dt>削除</dt>
                    <dd>
                      <time
                        dateTime={new Date(note.deleted_at_ms).toISOString()}
                      >
                        {formatDateTime(note.deleted_at_ms)}
                      </time>
                    </dd>
                  </div>
                  <div>
                    <dt>完全削除予定</dt>
                    <dd>
                      <time dateTime={new Date(note.purge_at_ms).toISOString()}>
                        {formatDateTime(note.purge_at_ms)}
                      </time>
                    </dd>
                  </div>
                  <div>
                    <dt>revision</dt>
                    <dd>rev-{note.revision}</dd>
                  </div>
                </dl>
                <p className="deleted-note-retention">{retention.label}</p>
                <button
                  className="button button-primary"
                  type="button"
                  onClick={(event) =>
                    openRestoreDialog(note, event.currentTarget)
                  }
                >
                  復元
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {selected !== null && (
        <ConfirmationDialog
          id="note-restore"
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
