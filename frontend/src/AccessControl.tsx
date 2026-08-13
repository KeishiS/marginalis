import {
  FormEvent,
  useEffect,
  useLayoutEffect,
  useReducer,
  useRef,
} from "react";

import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

import { ApiError, readNoteAcl, replaceNoteAcl } from "./api";
import {
  accessControlReducer,
  initialAccessControlState,
} from "./accessControlState";

interface Props {
  apiBase: string;
  noteId: string;
  revision: number;
  onRevision: (revision: number) => void;
}

export function AccessControl({
  apiBase,
  noteId,
  revision,
  onRevision,
}: Props) {
  const [state, dispatch] = useReducer(
    accessControlReducer,
    revision,
    initialAccessControlState,
  );
  const activeNoteId = useRef(noteId);
  useLayoutEffect(() => {
    activeNoteId.current = noteId;
  }, [noteId]);
  const {
    status,
    entries,
    subject,
    permission,
    revision: currentRevision,
    notice,
    error,
  } = state;

  useEffect(() => {
    const controller = new AbortController();
    dispatch({ type: "loading" });
    readNoteAcl(apiBase, noteId, controller.signal)
      .then(({ entries, revision }) =>
        dispatch({ type: "loaded", entries, revision }),
      )
      .catch((reason: unknown) => {
        if (controller.signal.aborted) return;
        if (
          reason instanceof ApiError &&
          (reason.status === 403 || reason.status === 404)
        ) {
          dispatch({
            type: "error",
            message: "共有設定を利用できません。",
          });
          return;
        }
        dispatch({
          type: "error",
          message: "共有設定を読み込めませんでした。",
        });
      });
    return () => controller.abort();
  }, [apiBase, noteId]);

  if (entries === null) {
    return error ? <ProblemAlert>{error}</ProblemAlert> : null;
  }
  const currentEntries = entries;

  function add(event: FormEvent) {
    event.preventDefault();
    dispatch({ type: "add" });
  }

  async function save() {
    if (status !== "ready") return;
    const savedNoteId = noteId;
    dispatch({ type: "save-started" });
    try {
      const note = await replaceNoteAcl(
        apiBase,
        noteId,
        currentEntries.map(({ subject, permission }) => ({
          subject,
          permission,
        })),
        currentRevision,
      );
      if (activeNoteId.current !== savedNoteId) return;
      dispatch({ type: "saved", revision: note.revision });
      onRevision(note.revision);
    } catch (reason: unknown) {
      if (activeNoteId.current !== savedNoteId) return;
      dispatch({
        type: "error",
        message:
          reason instanceof ApiError && reason.status === 409
            ? "別の操作で更新されています。画面を再読み込みしてください。"
            : "共有設定を保存できませんでした。",
      });
    }
  }

  return (
    <section
      className="grid max-w-4xl gap-4 rounded-md border bg-card p-[clamp(var(--space-4),3vw,var(--space-6))] shadow-xs"
      aria-labelledby="access-control-heading"
    >
      <h2 className="m-0" id="access-control-heading">
        共有設定
      </h2>
      <p className="m-0 max-w-2xl text-muted-foreground">
        同じ認証基盤の利用者subjectを正確に入力してください。
      </p>
      <ul className="m-0 grid list-none gap-3 p-0">
        {currentEntries.map((entry) => (
          <li
            key={entry.subject}
            className="flex flex-col items-stretch justify-between gap-4 rounded-sm bg-muted p-3 min-[60rem]:flex-row min-[60rem]:items-center"
          >
            <span className="flex min-w-0 flex-wrap items-center gap-3">
              <code className="[overflow-wrap:anywhere]">{entry.subject}</code>
              <Badge variant="secondary">
                {entry.permission === "edit" ? "閲覧・編集" : "閲覧"}
              </Badge>
            </span>
            <Button
              type="button"
              variant="destructive"
              size="sm"
              disabled={status === "saving"}
              onClick={() =>
                dispatch({ type: "remove", subject: entry.subject })
              }
            >
              削除
            </Button>
          </li>
        ))}
      </ul>
      {currentEntries.length === 0 && (
        <StatusMessage>追加の共有先はありません。</StatusMessage>
      )}
      <form
        className="flex flex-wrap items-end gap-3 border-t pt-4"
        onSubmit={add}
      >
        <label className="grid flex-[1_1_20rem] gap-1 text-sm font-semibold">
          利用者subject
          <Input
            disabled={status === "saving"}
            value={subject}
            onChange={(event) =>
              dispatch({ type: "subject", value: event.target.value })
            }
          />
        </label>
        <label className="grid gap-1 text-sm font-semibold">
          権限
          <select
            disabled={status === "saving"}
            value={permission}
            onChange={(event) =>
              dispatch({
                type: "permission",
                value: event.target.value === "edit" ? "edit" : "read",
              })
            }
          >
            <option value="read">閲覧</option>
            <option value="edit">閲覧・編集</option>
          </select>
        </label>
        <Button variant="outline" type="submit" disabled={status === "saving"}>
          共有先を追加
        </Button>
      </form>
      <div className="flex flex-wrap items-center gap-4">
        <Button type="button" onClick={save} disabled={status === "saving"}>
          {status === "saving" ? "保存しています…" : "共有設定を保存"}
        </Button>
        {notice && (
          <p className="m-0 text-sm text-success" role="status">
            {notice}
          </p>
        )}
      </div>
      {error && <ProblemAlert>{error}</ProblemAlert>}
    </section>
  );
}
