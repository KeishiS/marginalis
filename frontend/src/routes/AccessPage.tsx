import { useCallback, useState } from "react";

import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";

import { AccessControl } from "../AccessControl";
import { ApplicationConfig, readNote } from "../api";
import { notePath } from "../paths";
import { useApiResource } from "../useApiResource";

export function AccessPage({
  config,
  noteId,
}: {
  config: ApplicationConfig;
  noteId: string;
}) {
  const load = useCallback(
    (signal: AbortSignal) => readNote(config.apiBase, noteId, signal),
    [config.apiBase, noteId],
  );
  const resource = useApiResource(load);
  const note = resource.status === "ready" ? resource.value : null;
  const failed = resource.status === "failed";
  // ACLを更新するとノートのrevisionが進む。読み込んだ値を書き換えず、
  // どのノートに対する更新かを併せて保持する。
  const [updated, setUpdated] = useState<{
    noteId: string;
    revision: number;
  } | null>(null);
  const revision =
    updated?.noteId === noteId ? updated.revision : (note?.revision ?? 0);
  if (failed)
    return <ProblemAlert>共有設定を読み込めませんでした。</ProblemAlert>;
  if (note === null)
    return <StatusMessage>共有設定を読み込んでいます。</StatusMessage>;
  return (
    <section className="grid gap-6" aria-labelledby="access-page-heading">
      <PageHeader
        eyebrow="Access"
        title="共有設定"
        titleId="access-page-heading"
        description="このノートを閲覧または編集できる利用者を管理します。"
      />
      <nav className="flex flex-wrap gap-2" aria-label="ノート操作">
        <Button variant="outline" asChild>
          <a href={notePath(config, noteId)}>閲覧画面へ戻る</a>
        </Button>
      </nav>
      <AccessControl
        apiBase={config.apiBase}
        noteId={noteId}
        revision={revision}
        onRevision={(next) => setUpdated({ noteId, revision: next })}
      />
    </section>
  );
}
