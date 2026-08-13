import { ReactNode } from "react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { cn } from "@/lib/utils";

/**
 * 待つか眺めるだけでよい状態(読み込み中、該当なし、成功の知らせ)を控えめに伝える。
 * 支援技術へはrole="status"で穏やかに知らせ、内容は文言で区別する。
 */
export function StatusMessage({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <p
      data-slot="status-message"
      role="status"
      className={cn(
        "rounded-md bg-muted p-4 text-center text-muted-foreground",
        className,
      )}
    >
      {children}
    </p>
  );
}

/**
 * 利用者の対応が要る失敗を割り込んで伝える。原因と次の操作を画面内に残す。
 */
export function ProblemAlert({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <Alert
      variant="destructive"
      className={cn("border-destructive", className)}
    >
      <AlertDescription>{children}</AlertDescription>
    </Alert>
  );
}
