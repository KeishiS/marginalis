import {
  FormEvent,
  type RefObject,
  UIEvent,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";

import {
  Problem,
  NoteDiagnostic,
  createNote,
  readNote,
  updateNote,
} from "./api";
import { utf8ByteOffsetToTextOffset } from "./textPosition";
import {
  EditorForm as FormState,
  editorReducer,
  initialEditorState,
  noteToForm,
} from "./editorState";
import {
  editorActivityReducer,
  initialEditorActivityState,
} from "./editorActivityState";
import { RenderedContent } from "./RenderedContent";
import { useEditorPreview } from "./useEditorPreview";
import { alignThreeVersions } from "./editorConflict";
import {
  canSelectDiagnostic,
  diagnosticLocation,
  diagnosticMessage,
  diagnosticSeverityLabel,
  editorStatus,
  problemMessage,
  toProblem,
} from "./editorPresentation";
import { externalPath } from "./paths";

export interface EditorConfig {
  mode: "create" | "edit";
  noteId: string;
  apiBase: string;
  basePath: string;
  search: string;
}

export function EditorApplication({ config }: { config: EditorConfig }) {
  const [editor, dispatch] = useReducer(
    editorReducer,
    config.noteId,
    initialEditorState,
  );
  const { noteId, revision, form, baseline, conflict } = editor;
  const [loading, setLoading] = useState(config.mode === "edit");
  const [loadProblem, setLoadProblem] = useState<Problem | null>(null);
  const [activity, dispatchActivity] = useReducer(
    editorActivityReducer,
    initialEditorActivityState,
  );
  const { saving, problem, notice } = activity;
  const sourceInput = useRef<HTMLTextAreaElement>(null);
  const isDirty = useMemo(
    () => JSON.stringify(form) !== JSON.stringify(baseline),
    [baseline, form],
  );
  const draft = useMemo(() => ({ source: form.source }), [form.source]);
  const preview = useEditorPreview(
    config.apiBase,
    form.source,
    !loading && (config.mode !== "edit" || revision !== null),
    toProblem,
  );

  useEffect(() => {
    if (config.mode !== "edit") {
      return;
    }
    const controller = new AbortController();
    readNote(config.apiBase, config.noteId, controller.signal)
      .then((note) => {
        dispatch({ type: "accept-note", note });
        setLoadProblem(null);
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) {
          setLoadProblem(toProblem(error));
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

  function selectDiagnostic(diagnostic: NoteDiagnostic) {
    const span = diagnostic.span;
    if (diagnostic.target.field !== "source" || span?.unit !== "utf8_byte") {
      return;
    }
    const input = sourceInput.current;
    if (!input) return;
    const start = utf8ByteOffsetToTextOffset(form.source, span.start);
    const end = utf8ByteOffsetToTextOffset(form.source, span.end);
    input.focus();
    input.setSelectionRange(start, Math.max(start, end));
  }

  function changeSource(source: string) {
    dispatch({ type: "change", field: "source", value: source });
    dispatchActivity({ type: "clear-feedback" });
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    if (saving) {
      return;
    }
    dispatchActivity({ type: "save-started" });
    try {
      const note =
        revision === null
          ? await createNote(config.apiBase, draft)
          : await updateNote(config.apiBase, noteId, draft, revision);
      dispatch({ type: "accept-note", note });
      dispatchActivity({ type: "save-succeeded" });
      if (revision === null) {
        window.history.replaceState(
          null,
          "",
          `${externalPath(config.basePath, `/notes/${note.note_id}/edit`)}${config.search}`,
        );
      }
    } catch (error: unknown) {
      const nextProblem = toProblem(error);
      dispatchActivity({ type: "save-failed", problem: nextProblem });
      if (nextProblem.code === "conflict" && noteId) {
        try {
          const current = await readNote(config.apiBase, noteId);
          dispatch({ type: "conflict", current });
        } catch (refreshError: unknown) {
          dispatchActivity({
            type: "save-failed",
            problem: toProblem(refreshError),
          });
          dispatch({ type: "clear-conflict" });
        }
      }
    }
  }

  if (loading) {
    return <p role="status">ノートを読み込んでいます。</p>;
  }

  if (config.mode === "edit" && revision === null) {
    return (
      <section aria-labelledby="editor-heading">
        <h1 id="editor-heading">ノートの編集</h1>
        {loadProblem && (
          <ProblemMessage
            problem={loadProblem}
            heading="ノートを読み込めませんでした"
            headingId="load-problem-heading"
          />
        )}
        <a href={`${externalPath(config.basePath, "/")}${config.search}`}>
          一覧へ戻る
        </a>
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
              ? `${externalPath(config.basePath, `/notes/${noteId}`)}${config.search}`
              : `${externalPath(config.basePath, "/")}${config.search}`
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
          source={form.source}
          onSelectDiagnostic={selectDiagnostic}
        />
      )}
      {conflict && (
        <ConflictPanel
          editingStarted={conflict.editingStarted}
          editing={form}
          current={noteToForm(conflict.current)}
          currentRevision={conflict.current.revision}
          onUseCurrentRevision={() => {
            dispatch({ type: "rebase", note: conflict.current });
            dispatchActivity({
              type: "notice",
              message: `更新番号${conflict.current.revision}を基準にしました。内容を確認して保存してください。`,
            });
          }}
        />
      )}

      <div className="editor-workspace">
        <form className="editor-form" onSubmit={save}>
          <LineNumberedTextarea
            inputRef={sourceInput}
            value={form.source}
            disabled={saving}
            onChange={changeSource}
          />
          <div className="editor-actions">
            <button type="submit" disabled={saving || !isDirty}>
              {saving ? "保存しています…" : "保存"}
            </button>
            <span role="status">
              {editorStatus({
                saving,
                isDirty,
                failed: problem !== null,
                conflicted: conflict !== null,
                notice,
              })}
            </span>
          </div>
        </form>
        <PreviewPanel
          body={form.source}
          html={preview.html}
          diagnostics={preview.diagnostics}
          loading={preview.loading}
          problem={preview.problem}
          onSelectDiagnostic={selectDiagnostic}
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
      <h3>AsciiDoc文書の行単位比較</h3>
      <BodyConflictTable
        editingStarted={editingStarted.source}
        editing={editing.source}
        current={current.source}
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

function BodyConflictTable({
  editingStarted,
  editing,
  current,
}: {
  editingStarted: string;
  editing: string;
  current: string;
}) {
  const rows = alignThreeVersions(editingStarted, editing, current);
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
  source,
  onSelectDiagnostic,
}: {
  problem: Problem;
  heading: string;
  headingId: string;
  source?: string;
  onSelectDiagnostic?: (diagnostic: NoteDiagnostic) => void;
}) {
  return (
    <section className="problem" aria-labelledby={headingId} role="alert">
      <h2 id={headingId}>{heading}</h2>
      <p>{problemMessage(problem)}</p>
      {problem.diagnostics && problem.diagnostics.length > 0 && (
        <ul>
          {problem.diagnostics.map((diagnostic, index) => (
            <li key={`${diagnostic.code}-${index}`}>
              <span className="diagnostic-severity">
                {diagnosticSeverityLabel(diagnostic.severity)}:{" "}
              </span>
              {source ? diagnosticLocation(source, diagnostic) : ""}
              {diagnosticMessage(diagnostic.code)}{" "}
              {canSelectDiagnostic(diagnostic) && onSelectDiagnostic && (
                <button
                  type="button"
                  className="diagnostic-link"
                  onClick={() => onSelectDiagnostic(diagnostic)}
                >
                  入力位置へ移動
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function LineNumberedTextarea({
  inputRef,
  value,
  disabled,
  onChange,
}: {
  inputRef: RefObject<HTMLTextAreaElement | null>;
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
      AsciiDoc文書
      <span className="source-editor">
        <span
          aria-hidden="true"
          className="line-numbers"
          style={{ transform: `translateY(-${scrollTop}px)` }}
        >
          {lineNumbers}
        </span>
        <textarea
          ref={inputRef}
          autoFocus
          name="source"
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
  diagnostics,
  loading,
  problem,
  onSelectDiagnostic,
}: {
  body: string;
  html: string;
  diagnostics: NoteDiagnostic[];
  loading: boolean;
  problem: Problem | null;
  onSelectDiagnostic: (diagnostic: NoteDiagnostic) => void;
}) {
  return (
    <section className="preview-panel" aria-labelledby="preview-heading">
      <div className="preview-heading">
        <h2 id="preview-heading">プレビュー</h2>
        <span role="status">
          {loading
            ? "更新しています…"
            : problem && html
              ? "最後に成功したプレビューを表示しています。"
              : problem
                ? "更新に失敗しました。"
                : "最新です。"}
        </span>
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
                  <span className="diagnostic-severity">
                    {diagnosticSeverityLabel(diagnostic.severity)}:{" "}
                  </span>
                  {diagnosticLocation(body, diagnostic)}
                  {diagnosticMessage(diagnostic.code)}{" "}
                  {canSelectDiagnostic(diagnostic) && (
                    <button
                      type="button"
                      className="diagnostic-link"
                      onClick={() => onSelectDiagnostic(diagnostic)}
                    >
                      入力位置へ移動
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
      {!problem && diagnostics.length > 0 && (
        <section
          className="warnings"
          aria-labelledby="preview-diagnostics-heading"
        >
          <h3 id="preview-diagnostics-heading">入力時の診断</h3>
          <ul>
            {diagnostics.map((diagnostic, index) => (
              <li key={`${diagnostic.code}-${index}`}>
                <span className="diagnostic-severity">
                  {diagnosticSeverityLabel(diagnostic.severity)}:{" "}
                </span>
                {diagnosticLocation(body, diagnostic)}
                {diagnosticMessage(diagnostic.code)}{" "}
                {canSelectDiagnostic(diagnostic) && (
                  <button
                    type="button"
                    className="diagnostic-link"
                    onClick={() => onSelectDiagnostic(diagnostic)}
                  >
                    入力位置へ移動
                  </button>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}
      {html && <SafePreview html={html} />}
      {!html && !loading && !problem && <p>プレビューはありません。</p>}
    </section>
  );
}

function SafePreview({ html }: { html: string }) {
  // 同じ保存規則とRenderPolicyを通ったサーバー生成HTMLだけを受け取る。
  return <RenderedContent html={html} preview />;
}
