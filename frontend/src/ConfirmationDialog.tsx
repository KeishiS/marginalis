import { KeyboardEvent, ReactNode, useEffect, useRef } from "react";

export function ConfirmationDialog({
  id,
  eyebrow,
  heading,
  description,
  busy,
  problem,
  confirmLabel,
  busyLabel,
  confirmClassName = "button button-primary",
  onCancel,
  onConfirm,
}: {
  id: string;
  eyebrow: string;
  heading: string;
  description: ReactNode;
  busy: boolean;
  problem: string | null;
  confirmLabel: string;
  busyLabel: string;
  confirmClassName?: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    cancelButton.current?.focus();
  }, []);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape" && !busy) {
      event.preventDefault();
      onCancel();
      return;
    }
    if (event.key !== "Tab" || busy) return;
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

  return (
    <div className="dialog-backdrop">
      <div
        ref={dialog}
        className="confirmation-dialog surface"
        role="dialog"
        aria-modal="true"
        aria-labelledby={`${id}-heading`}
        aria-describedby={`${id}-description`}
        onKeyDown={handleKeyDown}
      >
        <p className="page-eyebrow">{eyebrow}</p>
        <h2 id={`${id}-heading`}>{heading}</h2>
        <p id={`${id}-description`}>{description}</p>
        {problem !== null && (
          <p className="problem-inline" role="alert">
            {problem}
          </p>
        )}
        <div
          className="confirmation-dialog-actions"
          aria-live="polite"
          aria-busy={busy}
        >
          <button
            ref={cancelButton}
            className="button button-secondary"
            type="button"
            disabled={busy}
            onClick={onCancel}
          >
            取り消す
          </button>
          <button
            className={confirmClassName}
            type="button"
            disabled={busy}
            onClick={onConfirm}
          >
            {busy ? busyLabel : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
