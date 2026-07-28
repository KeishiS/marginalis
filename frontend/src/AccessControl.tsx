import { FormEvent, useEffect, useReducer } from "react";

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
  const {
    entries,
    subject,
    permission,
    revision: currentRevision,
    notice,
    error,
  } = state;

  useEffect(() => {
    readNoteAcl(apiBase, noteId)
      .then(({ entries, revision }) =>
        dispatch({ type: "loaded", entries, revision }),
      )
      .catch((reason: unknown) => {
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
  }, [apiBase, noteId]);

  if (entries === null) {
    return error ? <p role="alert">{error}</p> : null;
  }
  const currentEntries = entries;

  function add(event: FormEvent) {
    event.preventDefault();
    dispatch({ type: "add" });
  }

  async function save() {
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
      dispatch({ type: "saved", revision: note.revision });
      onRevision(note.revision);
    } catch (reason: unknown) {
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
    <section aria-labelledby="access-control-heading">
      <h2 id="access-control-heading">共有設定</h2>
      <p>同じ認証基盤の利用者subjectを正確に入力してください。</p>
      <ul>
        {currentEntries.map((entry) => (
          <li key={entry.subject}>
            <code>{entry.subject}</code>（
            {entry.permission === "edit" ? "閲覧・編集" : "閲覧"}）
            <button
              type="button"
              onClick={() =>
                dispatch({ type: "remove", subject: entry.subject })
              }
            >
              削除
            </button>
          </li>
        ))}
      </ul>
      <form onSubmit={add}>
        <label>
          利用者subject
          <input
            value={subject}
            onChange={(event) =>
              dispatch({ type: "subject", value: event.target.value })
            }
          />
        </label>
        <label>
          権限
          <select
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
        <button type="submit">共有先を追加</button>
      </form>
      <button type="button" onClick={save}>
        共有設定を保存
      </button>
      {notice && <p role="status">{notice}</p>}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
