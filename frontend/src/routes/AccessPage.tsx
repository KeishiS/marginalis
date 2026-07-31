import { useCallback, useState } from "react";

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
  if (failed) return <p role="alert">共有設定を読み込めませんでした。</p>;
  if (note === null) return <p>共有設定を読み込んでいます。</p>;
  return (
    <section
      className="access-page page-section"
      aria-labelledby="access-page-heading"
    >
      <div className="page-heading">
        <div>
          <p className="page-eyebrow">Access</p>
          <h1 id="access-page-heading">共有設定</h1>
          <p className="page-description">
            このノートを閲覧または編集できる利用者を管理します。
          </p>
        </div>
      </div>
      <nav className="page-actions" aria-label="ノート操作">
        <a className="button button-secondary" href={notePath(config, noteId)}>
          閲覧画面へ戻る
        </a>
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
