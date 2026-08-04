import { KeyboardEvent, useEffect, useRef } from "react";

export function NoteDeletionDialog({
  title,
  deleting,
  problem,
  onCancel,
  onConfirm,
}: {
  title: string;
  deleting: boolean;
  problem: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    cancelButton.current?.focus();
  }, []);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape" && !deleting) {
      event.preventDefault();
      onCancel();
      return;
    }
    if (event.key === "Tab" && !deleting) {
      const buttons = Array.from(
        dialog.current?.querySelectorAll<HTMLButtonElement>(
          "button:not(:disabled)",
        ) ?? [],
      );
      const first = buttons[0];
      const last = buttons.at(-1);
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    }
  }

  return (
    <div className="dialog-backdrop">
      <div
        ref={dialog}
        className="confirmation-dialog surface"
        role="dialog"
        aria-modal="true"
        aria-labelledby="note-delete-heading"
        aria-describedby="note-delete-description"
        onKeyDown={handleKeyDown}
      >
        <p className="page-eyebrow">Delete note</p>
        <h2 id="note-delete-heading">このノートを削除しますか？</h2>
        <p id="note-delete-description">
          「<strong>{title}</strong>」を削除します。削除後30日以内であれば、
          削除済みノートの画面から復元できます。
        </p>
        {problem !== null && (
          <p className="problem-inline" role="alert">
            {problem}
          </p>
        )}
        <div
          className="confirmation-dialog-actions"
          aria-live="polite"
          aria-busy={deleting}
        >
          <button
            ref={cancelButton}
            className="button button-secondary"
            type="button"
            disabled={deleting}
            onClick={onCancel}
          >
            取り消す
          </button>
          <button
            className="button button-danger"
            type="button"
            disabled={deleting}
            onClick={onConfirm}
          >
            {deleting ? "削除しています…" : "削除する"}
          </button>
        </div>
      </div>
    </div>
  );
}
