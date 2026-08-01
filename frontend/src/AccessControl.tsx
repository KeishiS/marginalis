import {
  FormEvent,
  useEffect,
  useLayoutEffect,
  useReducer,
  useRef,
} from "react";

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
    return error ? (
      <p className="problem-inline" role="alert">
        {error}
      </p>
    ) : null;
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
      className="access-control surface"
      aria-labelledby="access-control-heading"
    >
      <h2 id="access-control-heading">共有設定</h2>
      <p className="section-description">
        同じ認証基盤の利用者subjectを正確に入力してください。
      </p>
      <ul className="access-list">
        {currentEntries.map((entry) => (
          <li key={entry.subject}>
            <span>
              <code>{entry.subject}</code>
              <span className="access-permission">
                {entry.permission === "edit" ? "閲覧・編集" : "閲覧"}
              </span>
            </span>
            <button
              type="button"
              className="button button-danger button-small"
              disabled={status === "saving"}
              onClick={() =>
                dispatch({ type: "remove", subject: entry.subject })
              }
            >
              削除
            </button>
          </li>
        ))}
      </ul>
      {currentEntries.length === 0 && (
        <p className="state-message">追加の共有先はありません。</p>
      )}
      <form className="access-form" onSubmit={add}>
        <label>
          利用者subject
          <input
            disabled={status === "saving"}
            value={subject}
            onChange={(event) =>
              dispatch({ type: "subject", value: event.target.value })
            }
          />
        </label>
        <label>
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
        <button
          className="button button-secondary"
          type="submit"
          disabled={status === "saving"}
        >
          共有先を追加
        </button>
      </form>
      <div className="form-actions">
        <button
          className="button button-primary"
          type="button"
          onClick={save}
          disabled={status === "saving"}
        >
          {status === "saving" ? "保存しています…" : "共有設定を保存"}
        </button>
        {notice && (
          <p className="notice-inline" role="status">
            {notice}
          </p>
        )}
      </div>
      {error && (
        <p className="problem-inline" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
