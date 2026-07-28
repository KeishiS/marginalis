import { FormEvent, useEffect, useState } from "react";

import {
  ApiError,
  NoteAclEntry,
  NotePermission,
  readNoteAcl,
  replaceNoteAcl,
} from "./api";

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
  const [entries, setEntries] = useState<NoteAclEntry[] | null>(null);
  const [subject, setSubject] = useState("");
  const [permission, setPermission] = useState<NotePermission>("read");
  const [currentRevision, setCurrentRevision] = useState(revision);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    readNoteAcl(apiBase, noteId)
      .then(({ entries }) => {
        if (Array.isArray(entries)) {
          setEntries(entries);
        }
      })
      .catch((reason: unknown) => {
        if (
          reason instanceof ApiError &&
          (reason.status === 403 || reason.status === 404)
        ) {
          return;
        }
        setError("共有設定を読み込めませんでした。");
      });
  }, [apiBase, noteId]);

  if (entries === null) {
    return error ? <p role="alert">{error}</p> : null;
  }
  const currentEntries = entries;

  function add(event: FormEvent) {
    event.preventDefault();
    const normalized = subject.trim();
    if (!normalized) {
      setError("共有する利用者のsubjectを入力してください。");
      return;
    }
    setEntries((current) => [
      ...(current ?? []).filter((entry) => entry.subject !== normalized),
      { subject: normalized, permission },
    ]);
    setSubject("");
    setError("");
    setNotice("未保存の共有設定があります。");
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
      setCurrentRevision(note.revision);
      onRevision(note.revision);
      setNotice("共有設定を保存しました。");
      setError("");
    } catch (reason: unknown) {
      setError(
        reason instanceof ApiError && reason.status === 409
          ? "別の操作で更新されています。画面を再読み込みしてください。"
          : "共有設定を保存できませんでした。",
      );
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
              onClick={() => {
                setEntries(currentEntries.filter((item) => item !== entry));
                setNotice("未保存の共有設定があります。");
              }}
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
            onChange={(event) => setSubject(event.target.value)}
          />
        </label>
        <label>
          権限
          <select
            value={permission}
            onChange={(event) =>
              setPermission(event.target.value as NotePermission)
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
