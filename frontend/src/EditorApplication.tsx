import {
  FormEvent,
  UIEvent,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";

import {
  ApiError,
  Problem,
  ValidationDiagnostic,
  createNote,
  previewNote,
  readNote,
  updateNote,
} from "./api";
import { utf8ByteOffsetToLineColumn } from "./textPosition";
import {
  EditorForm as FormState,
  editorReducer,
  initialEditorState,
  noteToForm,
} from "./editorState";

export interface EditorConfig {
  mode: "create" | "edit";
  noteId: string;
  apiBase: string;
  basePath: string;
}

export function EditorApplication({ config }: { config: EditorConfig }) {
  const [editor, dispatch] = useReducer(
    editorReducer,
    config.noteId,
    initialEditorState,
  );
  const { noteId, revision, form, baseline, conflict } = editor;
  const [loading, setLoading] = useState(config.mode === "edit");
  const [saving, setSaving] = useState(false);
  const [problem, setProblem] = useState<Problem | null>(null);
  const [notice, setNotice] = useState("");
  const [previewHtml, setPreviewHtml] = useState("");
  const [previewProblem, setPreviewProblem] = useState<Problem | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const titleInput = useRef<HTMLInputElement>(null);
  const initialFocusApplied = useRef(false);
  const isDirty = useMemo(
    () => JSON.stringify(form) !== JSON.stringify(baseline),
    [baseline, form],
  );
  const draft = useMemo(
    () => ({
      title: form.title,
      body: form.body,
      tags: parseTags(form.tagsText),
    }),
    [form],
  );

  useEffect(() => {
    if (config.mode !== "edit") {
      return;
    }
    const controller = new AbortController();
    readNote(config.apiBase, config.noteId, controller.signal)
      .then((note) => {
        dispatch({ type: "accept-note", note });
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
    if (!loading && !initialFocusApplied.current) {
      initialFocusApplied.current = true;
      titleInput.current?.focus();
    }
  }, [loading]);

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

  useEffect(() => {
    if (loading || (config.mode === "edit" && revision === null)) {
      return;
    }
    const controller = new AbortController();
    let current = true;
    const timer = window.setTimeout(() => {
      setPreviewLoading(true);
      previewNote(config.apiBase, draft, controller.signal)
        .then((preview) => {
          if (current) {
            setPreviewHtml(preview.html);
            setPreviewProblem(null);
          }
        })
        .catch((error: unknown) => {
          if (current && !controller.signal.aborted) {
            setPreviewHtml("");
            setPreviewProblem(toProblem(error));
          }
        })
        .finally(() => {
          if (current) {
            setPreviewLoading(false);
          }
        });
    }, 350);
    return () => {
      current = false;
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [config.apiBase, config.mode, draft, loading, revision]);

  async function save(event: FormEvent) {
    event.preventDefault();
    if (saving) {
      return;
    }
    setSaving(true);
    setProblem(null);
    setNotice("");
    try {
      const note =
        revision === null
          ? await createNote(config.apiBase, draft)
          : await updateNote(config.apiBase, noteId, draft, revision);
      dispatch({ type: "accept-note", note });
      setNotice("保存しました。");
      if (revision === null) {
        window.history.replaceState(
          null,
          "",
          externalPath(config.basePath, `/notes/${note.note_id}/edit`),
        );
      }
    } catch (error: unknown) {
      const nextProblem = toProblem(error);
      setProblem(nextProblem);
      if (nextProblem.code === "conflict" && noteId) {
        try {
          const current = await readNote(config.apiBase, noteId);
          dispatch({ type: "conflict", current });
        } catch (refreshError: unknown) {
          setProblem(toProblem(refreshError));
          dispatch({ type: "clear-conflict" });
        }
      }
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
            headingId="load-problem-heading"
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
        <ProblemMessage
          problem={problem}
          heading="保存できませんでした"
          headingId="save-problem-heading"
        />
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}
      {conflict && (
        <ConflictPanel
          editingStarted={conflict.editingStarted}
          editing={form}
          current={noteToForm(conflict.current)}
          currentRevision={conflict.current.revision}
          onUseCurrentRevision={() => {
            dispatch({ type: "rebase", note: conflict.current });
            setProblem(null);
            setNotice(
              `更新番号${conflict.current.revision}を基準にしました。内容を確認して保存してください。`,
            );
          }}
        />
      )}

      <div className="editor-workspace">
        <form className="editor-form" onSubmit={save}>
          <label>
            題名
            <input
              ref={titleInput}
              name="title"
              value={form.title}
              onChange={(event) =>
                dispatch({
                  type: "change",
                  field: "title",
                  value: event.target.value,
                })
              }
              disabled={saving}
            />
          </label>
          <LineNumberedTextarea
            value={form.body}
            disabled={saving}
            onChange={(body) =>
              dispatch({ type: "change", field: "body", value: body })
            }
          />
          <label>
            タグ（コンマ区切り）
            <input
              name="tags"
              value={form.tagsText}
              onChange={(event) =>
                dispatch({
                  type: "change",
                  field: "tagsText",
                  value: event.target.value,
                })
              }
              disabled={saving}
            />
          </label>
          <div className="editor-actions">
            <button type="submit" disabled={saving || !isDirty}>
              {saving ? "保存しています…" : "保存"}
            </button>
            <span role="status">
              {isDirty
                ? "未保存の変更があります。"
                : "変更は保存されています。"}
            </span>
          </div>
        </form>
        <PreviewPanel
          body={form.body}
          html={previewHtml}
          loading={previewLoading}
          problem={previewProblem}
        />
      </div>
    </section>
  );
}

function ConflictPanel({
  editingStarted,
  editing,
  current,
  currentRevision,
  onUseCurrentRevision,
}: {
  editingStarted: FormState;
  editing: FormState;
  current: FormState;
  currentRevision: number;
  onUseCurrentRevision: () => void;
}) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => heading.current?.focus(), []);
  return (
    <section className="conflict-panel" aria-labelledby="conflict-heading">
      <h2 id="conflict-heading" ref={heading} tabIndex={-1}>
        更新内容の競合
      </h2>
      <p>
        編集中の内容は維持されています。三つの内容を比較し、必要な修正を行ってください。
      </p>
      <ConflictFields
        editingStarted={editingStarted}
        editing={editing}
        current={current}
      />
      <h3>本文の行単位比較</h3>
      <BodyConflictTable
        editingStarted={editingStarted.body}
        editing={editing.body}
        current={current.body}
      />
      <button type="button" onClick={onUseCurrentRevision}>
        更新番号{currentRevision}を編集の基準にする
      </button>
      <p>
        この操作では保存しません。比較後にフォームの「保存」を選んでください。
      </p>
    </section>
  );
}

function ConflictFields({
  editingStarted,
  editing,
  current,
}: {
  editingStarted: FormState;
  editing: FormState;
  current: FormState;
}) {
  const values = [
    { label: "編集開始時点", form: editingStarted },
    { label: "編集中", form: editing },
    { label: "現在保存されている内容", form: current },
  ];
  return (
    <div className="conflict-fields">
      {values.map(({ label, form }) => (
        <section key={label} aria-label={label}>
          <h3>{label}</h3>
          <dl>
            <dt>題名</dt>
            <dd>{form.title || "（空欄）"}</dd>
            <dd className="change-status">
              {fieldStatus(label, form.title, editingStarted.title)}
            </dd>
            <dt>タグ</dt>
            <dd>{form.tagsText || "（なし）"}</dd>
            <dd className="change-status">
              {fieldStatus(
                label,
                normalizedTags(form.tagsText),
                normalizedTags(editingStarted.tagsText),
              )}
            </dd>
          </dl>
        </section>
      ))}
    </div>
  );
}

function BodyConflictTable({
  editingStarted,
  editing,
  current,
}: {
  editingStarted: string;
  editing: string;
  current: string;
}) {
  const rows = alignThreeVersions(
    splitLines(editingStarted),
    splitLines(editing),
    splitLines(current),
  );
  return (
    <div
      className="conflict-body-scroll"
      tabIndex={0}
      aria-label="本文比較表のスクロール領域"
    >
      <table className="conflict-body">
        <caption>本文の行単位比較</caption>
        <thead>
          <tr>
            <th scope="col">行</th>
            <th scope="col">状態</th>
            <th scope="col">編集開始時点</th>
            <th scope="col">編集中</th>
            <th scope="col">現在保存されている内容</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr className={row.changed ? "changed" : undefined} key={index}>
              <th scope="row">{row.line}</th>
              <td className="change-status">{row.status}</td>
              {[row.editingStarted, row.editing, row.current].map(
                (value, column) => (
                  <td key={column}>
                    <code>{value ?? "\u00a0"}</code>
                  </td>
                ),
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ProblemMessage({
  problem,
  heading,
  headingId,
}: {
  problem: Problem;
  heading: string;
  headingId: string;
}) {
  return (
    <section className="problem" aria-labelledby={headingId} role="alert">
      <h2 id={headingId}>{heading}</h2>
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

function LineNumberedTextarea({
  value,
  disabled,
  onChange,
}: {
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const [scrollTop, setScrollTop] = useState(0);
  const lineNumbers = Array.from(
    { length: Math.max(1, value.split(/\r\n|\r|\n/).length) },
    (_, index) => index + 1,
  ).join("\n");
  const syncScroll = (event: UIEvent<HTMLTextAreaElement>) => {
    setScrollTop(event.currentTarget.scrollTop);
  };

  return (
    <label>
      本文（AsciiDoc）
      <span className="source-editor">
        <span
          aria-hidden="true"
          className="line-numbers"
          style={{ transform: `translateY(-${scrollTop}px)` }}
        >
          {lineNumbers}
        </span>
        <textarea
          name="body"
          rows={20}
          wrap="off"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onScroll={syncScroll}
          disabled={disabled}
        />
      </span>
    </label>
  );
}

function PreviewPanel({
  body,
  html,
  loading,
  problem,
}: {
  body: string;
  html: string;
  loading: boolean;
  problem: Problem | null;
}) {
  return (
    <section className="preview-panel" aria-labelledby="preview-heading">
      <div className="preview-heading">
        <h2 id="preview-heading">プレビュー</h2>
        <span role="status">{loading ? "更新しています…" : "最新です。"}</span>
      </div>
      {problem && (
        <section
          className="problem"
          aria-labelledby="preview-problem-heading"
          role="alert"
        >
          <h3 id="preview-problem-heading">プレビューできませんでした</h3>
          <p>{problemMessage(problem)}</p>
          {problem.diagnostics && (
            <ul>
              {problem.diagnostics.map((diagnostic, index) => (
                <li key={`${diagnostic.code}-${index}`}>
                  {diagnosticLocation(body, diagnostic)}
                  {diagnosticMessage(diagnostic.code)}
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
      {!problem && html && <SafePreview html={html} />}
      {!problem && !html && !loading && <p>プレビューはありません。</p>}
    </section>
  );
}

function SafePreview({ html }: { html: string }) {
  // 同じ保存規則とRenderPolicyを通ったサーバー生成HTMLだけを受け取る。
  return (
    <div
      className="preview-content"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function splitLines(value: string): string[] {
  return value.split(/\r\n|\r|\n/);
}

function fieldStatus(label: string, value: string, baseline: string): string {
  if (label === "編集開始時点") {
    return "比較基準";
  }
  return value === baseline ? "変更なし" : "変更あり";
}

function normalizedTags(value: string): string {
  return parseTags(value)
    .map((tag) => tag.toLocaleLowerCase())
    .sort()
    .join("\u0000");
}

interface AlignedLines {
  insertions: string[][];
  matches: Array<string | null>;
}

interface ConflictLine {
  line: string;
  status: string;
  changed: boolean;
  editingStarted: string | null;
  editing: string | null;
  current: string | null;
}

function alignThreeVersions(
  baseline: string[],
  editing: string[],
  current: string[],
): ConflictLine[] {
  const editingAligned = alignToBaseline(baseline, editing);
  const currentAligned = alignToBaseline(baseline, current);
  const rows: ConflictLine[] = [];
  for (
    let baselineIndex = 0;
    baselineIndex <= baseline.length;
    baselineIndex++
  ) {
    const editingInsertions = editingAligned.insertions[baselineIndex] ?? [];
    const currentInsertions = currentAligned.insertions[baselineIndex] ?? [];
    const insertionCount = Math.max(
      editingInsertions.length,
      currentInsertions.length,
    );
    for (
      let insertionIndex = 0;
      insertionIndex < insertionCount;
      insertionIndex++
    ) {
      const editingLine = editingInsertions[insertionIndex] ?? null;
      const currentLine = currentInsertions[insertionIndex] ?? null;
      rows.push({
        line: "追加",
        status:
          editingLine !== null && currentLine !== null
            ? "編集中と現在の内容に追加"
            : editingLine !== null
              ? "編集中に追加"
              : "現在の内容に追加",
        changed: true,
        editingStarted: null,
        editing: editingLine,
        current: currentLine,
      });
    }
    if (baselineIndex === baseline.length) {
      continue;
    }
    const editingLine = editingAligned.matches[baselineIndex] ?? null;
    const currentLine = currentAligned.matches[baselineIndex] ?? null;
    const deletions = [
      editingLine === null ? "編集中から削除" : "",
      currentLine === null ? "現在の内容から削除" : "",
    ].filter(Boolean);
    rows.push({
      line: String(baselineIndex + 1),
      status: deletions.length > 0 ? deletions.join("、") : "変更なし",
      changed: deletions.length > 0,
      editingStarted: baseline[baselineIndex] ?? "",
      editing: editingLine,
      current: currentLine,
    });
  }
  return rows;
}

function alignToBaseline(baseline: string[], variant: string[]): AlignedLines {
  if (baseline.length * variant.length > 250_000) {
    return alignLargeDocument(baseline, variant);
  }
  const lengths = Array.from({ length: baseline.length + 1 }, () =>
    Array<number>(variant.length + 1).fill(0),
  );
  for (let left = baseline.length - 1; left >= 0; left--) {
    for (let right = variant.length - 1; right >= 0; right--) {
      lengths[left]![right] =
        baseline[left] === variant[right]
          ? (lengths[left + 1]?.[right + 1] ?? 0) + 1
          : Math.max(
              lengths[left + 1]?.[right] ?? 0,
              lengths[left]?.[right + 1] ?? 0,
            );
    }
  }
  const insertions = Array.from(
    { length: baseline.length + 1 },
    () => [] as string[],
  );
  const matches = Array<string | null>(baseline.length).fill(null);
  let left = 0;
  let right = 0;
  while (left < baseline.length && right < variant.length) {
    if (baseline[left] === variant[right]) {
      matches[left] = variant[right] ?? "";
      left++;
      right++;
    } else if (
      (lengths[left]?.[right + 1] ?? 0) >= (lengths[left + 1]?.[right] ?? 0)
    ) {
      insertions[left]?.push(variant[right] ?? "");
      right++;
    } else {
      left++;
    }
  }
  while (right < variant.length) {
    insertions[baseline.length]?.push(variant[right] ?? "");
    right++;
  }
  return { insertions, matches };
}

function alignLargeDocument(
  baseline: string[],
  variant: string[],
): AlignedLines {
  const insertions = Array.from(
    { length: baseline.length + 1 },
    () => [] as string[],
  );
  const matches = baseline.map((line, index) =>
    variant[index] === line ? line : null,
  );
  for (let index = 0; index < variant.length; index++) {
    if (matches[index] === null || index >= baseline.length) {
      insertions[Math.min(index, baseline.length)]?.push(variant[index] ?? "");
    }
  }
  return { insertions, matches };
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

function diagnosticLocation(
  body: string,
  diagnostic: ValidationDiagnostic,
): string {
  if (
    diagnostic.target.field !== "body" ||
    diagnostic.span?.unit !== "utf8_byte"
  ) {
    return "";
  }
  const location = utf8ByteOffsetToLineColumn(body, diagnostic.span.start);
  return `${location.line}行${location.column}列: `;
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
