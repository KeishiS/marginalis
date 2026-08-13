import { ReactNode, useEffect, useRef } from "react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Alert, AlertDescription } from "@/components/ui/alert";

export function ConfirmationDialog({
  eyebrow,
  heading,
  description,
  busy,
  problem,
  confirmLabel,
  busyLabel,
  destructive = false,
  onCancel,
  onConfirm,
}: {
  eyebrow: string;
  heading: string;
  description: ReactNode;
  busy: boolean;
  problem: string | null;
  confirmLabel: string;
  busyLabel: string;
  destructive?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const content = useRef<HTMLDivElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);

  // 実行中は確認画面自体へフォーカスを保ち、二重送信と背景の操作を防ぐ。
  // 失敗して実行中が解けたときは「取り消す」へフォーカスを戻す。
  useEffect(() => {
    if (busy) {
      content.current?.focus();
    } else {
      cancelButton.current?.focus();
    }
  }, [busy]);

  return (
    <AlertDialog
      open
      onOpenChange={(open) => {
        if (!open && !busy) {
          onCancel();
        }
      }}
    >
      <AlertDialogContent
        ref={content}
        onEscapeKeyDown={(event) => {
          if (busy) {
            event.preventDefault();
          }
        }}
        onCloseAutoFocus={(event) => {
          // 閉じたあとの戻り先は呼び出し側が管理する(削除ボタンへ戻すなど)。
          event.preventDefault();
        }}
      >
        <AlertDialogHeader>
          <p className="text-xs font-bold tracking-[0.12em] text-primary uppercase">
            {eyebrow}
          </p>
          <AlertDialogTitle>{heading}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        {problem !== null && (
          <Alert variant="destructive">
            <AlertDescription>{problem}</AlertDescription>
          </Alert>
        )}
        <AlertDialogFooter aria-live="polite" aria-busy={busy}>
          <AlertDialogCancel
            ref={cancelButton}
            disabled={busy}
            onClick={onCancel}
          >
            取り消す
          </AlertDialogCancel>
          <AlertDialogAction
            variant={destructive ? "destructive" : "default"}
            disabled={busy}
            onClick={(event) => {
              // 成否の判断は呼び出し側が持つため、押しただけでは閉じない。
              event.preventDefault();
              onConfirm();
            }}
          >
            {busy ? busyLabel : confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
