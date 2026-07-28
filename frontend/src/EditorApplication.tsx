import { FormEvent, useEffect, useMemo, useState } from "react";

import {
  ApiError,
  Note,
  Problem,
  createNote,
  readNote,
  updateNote,
} from "./api";

export interface EditorConfig {
  mode: "create" | "edit";
  noteId: string;
  apiBase: string;
  basePath: string;
}

interface FormState {
  title: string;
  body: string;
  tagsText: string;
}

const EMPTY_FORM: FormState = { title: "", body: "", tagsText: "" };

export function EditorApplication({ config }: { config: EditorConfig }) {
  const [noteId, setNoteId] = useState(config.noteId);
  const [revision, setRevision] = useState<number | null>(null);
  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [baseline, setBaseline] = useState<FormState>(EMPTY_FORM);
  const [loading, setLoading] = useState(config.mode === "edit");
  const [saving, setSaving] = useState(false);
  const [problem, setProblem] = useState<Problem | null>(null);
  const [notice, setNotice] = useState("");
  const isDirty = useMemo(
    () => JSON.stringify(form) !== JSON.stringify(baseline),
    [baseline, form],
  );

  useEffect(() => {
    if (config.mode !== "edit") {
      return;
    }
    const controller = new AbortController();
    readNote(config.apiBase, config.noteId, controller.signal)
      .then((note) => {
        applyNote(note, setForm, setBaseline, setRevision);
        setProblem(null);
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) {
          setProblem(toProblem(error));
        }
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setLoading(false);
        }
      });
    return () => controller.abort();
  }, [config.apiBase, config.mode, config.noteId]);

  useEffect(() => {
    const warnAboutUnsavedChanges = (event: BeforeUnloadEvent) => {
      if (isDirty) {
        event.preventDefault();
      }
    };
    window.addEventListener("beforeunload", warnAboutUnsavedChanges);
    return () =>
      window.removeEventListener("beforeunload", warnAboutUnsavedChanges);
  }, [isDirty]);

  async function save(event: FormEvent) {
    event.preventDefault();
    if (saving) {
      return;
    }
    setSaving(true);
    setProblem(null);
    setNotice("");
    const draft = {
      title: form.title,
      body: form.body,
      tags: parseTags(form.tagsText),
    };
    try {
      const note =
        revision === null
          ? await createNote(config.apiBase, draft)
          : await updateNote(config.apiBase, noteId, draft, revision);
      setNoteId(note.note_id);
      applyNote(note, setForm, setBaseline, setRevision);
      setNotice("保存しました。");
      if (revision === null) {
        window.history.replaceState(
          null,
          "",
          externalPath(config.basePath, `/notes/${note.note_id}/edit`),
        );
      }
    } catch (error: unknown) {
      setProblem(toProblem(error));
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return <p role="status">ノートを読み込んでいます。</p>;
  }

  if (config.mode === "edit" && revision === null) {
    return (
      <section aria-labelledby="editor-heading">
        <h1 id="editor-heading">ノートの編集</h1>
        {problem && (
          <ProblemMessage
            problem={problem}
            heading="ノートを読み込めませんでした"
          />
        )}
        <a href={externalPath(config.basePath, "/")}>一覧へ戻る</a>
      </section>
    );
  }

  return (
    <section aria-labelledby="editor-heading">
      <div className="editor-heading">
        <div>
          <h1 id="editor-heading">
            {revision === null ? "ノートの作成" : "ノートの編集"}
          </h1>
          {revision !== null && (
            <p className="metadata">更新番号: {revision}</p>
          )}
        </div>
        <a
          href={
            noteId
              ? externalPath(config.basePath, `/notes/${noteId}`)
              : externalPath(config.basePath, "/")
          }
        >
          {noteId ? "閲覧画面へ戻る" : "一覧へ戻る"}
        </a>
      </div>

      {problem && (
        <ProblemMessage problem={problem} heading="保存できませんでした" />
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      <form className="editor-form" onSubmit={save}>
        <label>
          題名
          <input
            name="title"
            value={form.title}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                title: event.target.value,
              }))
            }
            disabled={saving}
          />
        </label>
        <label>
          本文（AsciiDoc）
          <textarea
            name="body"
            rows={20}
            value={form.body}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                body: event.target.value,
              }))
            }
            disabled={saving}
          />
        </label>
        <label>
          タグ（コンマ区切り）
          <input
            name="tags"
            value={form.tagsText}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                tagsText: event.target.value,
              }))
            }
            disabled={saving}
          />
        </label>
        <div className="editor-actions">
          <button type="submit" disabled={saving || !isDirty}>
            {saving ? "保存しています…" : "保存"}
          </button>
          <span role="status">
            {isDirty ? "未保存の変更があります。" : "変更は保存されています。"}
          </span>
        </div>
      </form>
    </section>
  );
}

function ProblemMessage({
  problem,
  heading,
}: {
  problem: Problem;
  heading: string;
}) {
  return (
    <section className="problem" aria-labelledby="problem-heading" role="alert">
      <h2 id="problem-heading">{heading}</h2>
      <p>{problemMessage(problem)}</p>
      {problem.diagnostics && problem.diagnostics.length > 0 && (
        <ul>
          {problem.diagnostics.map((diagnostic, index) => (
            <li key={`${diagnostic.code}-${index}`}>
              {diagnosticMessage(diagnostic.code)}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function applyNote(
  note: Note,
  setForm: (form: FormState) => void,
  setBaseline: (form: FormState) => void,
  setRevision: (revision: number) => void,
) {
  const next = {
    title: note.title,
    body: note.body,
    tagsText: note.tags.join(", "),
  };
  setForm(next);
  setBaseline(next);
  setRevision(note.revision);
}

function parseTags(value: string): string[] {
  return value
    .split(",")
    .map((tag) => tag.trim())
    .filter((tag) => tag.length > 0);
}

function toProblem(error: unknown): Problem {
  if (error instanceof ApiError) {
    return error.problem;
  }
  return {
    code: "network_error",
    message: "通信に失敗しました。入力内容を保ったまま再試行できます。",
  };
}

function problemMessage(problem: Problem): string {
  switch (problem.code) {
    case "validation_failed":
      return "入力内容を確認してください。";
    case "conflict":
      return "ほかの操作でノートが更新されました。";
    case "authentication_required":
      return "ログインの有効期限が切れました。再度ログインしてください。";
    default:
      return problem.message;
  }
}

function diagnosticMessage(code: string): string {
  switch (code) {
    case "invalid_title":
      return "題名を入力し、改行と上限を超える文字を取り除いてください。";
    case "invalid_tag":
      return "タグの空欄、改行、重複、または長さを確認してください。";
    case "too_many_tags":
      return "タグの数が上限を超えています。";
    case "body_too_large":
      return "本文のデータ量が上限を超えています。";
    case "asciidoc_parse_failed":
      return "AsciiDoc本文を解析できませんでした。";
    case "include_directive_disabled":
      return "includeディレクティブは使用できません。";
    case "inline_passthrough_disabled":
    case "block_passthrough_disabled":
      return "未検証の内容を直接出力する記法は使用できません。";
    case "duplicate_anchor":
      return "同じアンカーが複数あります。";
    case "external_reference_disabled":
      return "外部の参照先は使用できません。";
    case "invalid_url_scheme":
      return "許可されていない形式のURLです。";
    case "resource_disabled":
      return "外部リソースは使用できません。";
    case "unsupported_math_language":
      return "対応していない数式形式です。";
    case "unsupported_source_language":
      return "対応していないソースコード言語です。";
    default:
      return "入力内容を確認してください。";
  }
}

function externalPath(basePath: string, path: string): string {
  return basePath === "/" ? path : `${basePath.replace(/\/$/, "")}${path}`;
}
