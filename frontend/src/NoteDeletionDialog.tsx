import { ConfirmationDialog } from "./ConfirmationDialog";

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
  return (
    <ConfirmationDialog
      eyebrow="Delete note"
      heading="このノートを削除しますか？"
      description={
        <>
          「<strong>{title}</strong>」を削除します。削除後30日以内であれば、
          削除済みノートの画面から復元できます。
        </>
      }
      busy={deleting}
      problem={problem}
      confirmLabel="削除する"
      busyLabel="削除しています…"
      destructive
      onCancel={onCancel}
      onConfirm={onConfirm}
    />
  );
}
