import { ReactNode } from "react";

/**
 * 画面上部の見出し。小さな分類、題名、説明を左へ、主要な操作を右へ置く。
 * 幅が足りない場合は縦へ折り返す。
 */
export function PageHeader({
  eyebrow,
  title,
  titleId,
  description,
  children,
}: {
  eyebrow: string;
  title: string;
  titleId?: string;
  description?: string;
  children?: ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-4">
      <div>
        <p className="m-0 text-xs font-bold tracking-[0.12em] text-primary uppercase">
          {eyebrow}
        </p>
        <h1
          id={titleId}
          className="m-0 text-(length:--text-note-title) leading-tight tracking-[-0.035em]"
        >
          {title}
        </h1>
        {description && (
          <p className="mt-2 mb-0 max-w-2xl text-muted-foreground">
            {description}
          </p>
        )}
      </div>
      {children}
    </div>
  );
}
