import {
  type CSSProperties,
  FormEvent,
  UIEvent,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";

import { AsciiDocEditor, type AsciiDocEditorHandle } from "./AsciiDocEditor";
import { ASCII_DOC_COMMANDS, type AsciiDocCommand } from "./asciiDocEditing";
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

type EditorViewMode = "write" | "split" | "preview";

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
  const editorForm = useRef<HTMLFormElement>(null);
  const sourceEditor = useRef<AsciiDocEditorHandle>(null);
  const previewScroll = useRef<HTMLDivElement>(null);
  const scrollSource = useRef<"editor" | "preview" | null>(null);
  const [isComposing, setIsComposing] = useState(false);
  const [viewMode, setViewMode] = useState<EditorViewMode>("split");
  const [editorWidth, setEditorWidth] = useState(50);
  const [syncScroll, setSyncScroll] = useState(true);
  const narrowViewport = useMediaQuery("(max-width: 60rem)");
  const effectiveViewMode =
    narrowViewport && viewMode === "split" ? "write" : viewMode;
  const isDirty = useMemo(
    () => JSON.stringify(form) !== JSON.stringify(baseline),
    [baseline, form],
  );
  const draft = useMemo(() => ({ source: form.source }), [form.source]);
  const preview = useEditorPreview(
    config.apiBase,
    form.source,
    !isComposing && !loading && (config.mode !== "edit" || revision !== null),
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
    const start = utf8ByteOffsetToTextOffset(form.source, span.start);
    const end = utf8ByteOffsetToTextOffset(form.source, span.end);
    const select = () =>
      sourceEditor.current?.selectRange(start, Math.max(start, end));
    if (effectiveViewMode === "preview") {
      setViewMode("write");
      window.requestAnimationFrame(select);
    } else {
      select();
    }
  }

  function changeSource(source: string) {
    dispatch({ type: "change", field: "source", value: source });
    dispatchActivity({ type: "clear-feedback" });
  }

  function changeViewMode(mode: EditorViewMode) {
    setViewMode(mode);
    if (mode !== "preview") {
      window.requestAnimationFrame(() => sourceEditor.current?.focus());
    }
  }

  function applyEditorCommand(command: AsciiDocCommand) {
    if (effectiveViewMode === "preview") {
      setViewMode("write");
      window.requestAnimationFrame(() =>
        sourceEditor.current?.applyCommand(command),
      );
      return;
    }
    sourceEditor.current?.applyCommand(command);
  }

  function synchronizeFromEditor(ratio: number) {
    if (
      !syncScroll ||
      effectiveViewMode !== "split" ||
      scrollSource.current === "preview"
    ) {
      return;
    }
    const preview = previewScroll.current;
    if (!preview) return;
    scrollSource.current = "editor";
    const maximum = Math.max(0, preview.scrollHeight - preview.clientHeight);
    preview.scrollTop = ratio * maximum;
    window.requestAnimationFrame(() => {
      if (scrollSource.current === "editor") scrollSource.current = null;
    });
  }

  function synchronizeFromPreview(event: UIEvent<HTMLDivElement>) {
    if (
      !syncScroll ||
      effectiveViewMode !== "split" ||
      scrollSource.current === "editor"
    ) {
      return;
    }
    const preview = event.currentTarget;
    const maximum = preview.scrollHeight - preview.clientHeight;
    const ratio = maximum > 0 ? preview.scrollTop / maximum : 0;
    scrollSource.current = "preview";
    sourceEditor.current?.setScrollRatio(ratio);
    window.requestAnimationFrame(() => {
      if (scrollSource.current === "preview") scrollSource.current = null;
    });
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
    <section className="editor-page" aria-labelledby="editor-heading">
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

      <form className="editor-form" onSubmit={save} ref={editorForm}>
        <div className="editor-controls">
          <EditorViewToolbar
            mode={effectiveViewMode}
            requestedMode={viewMode}
            narrow={narrowViewport}
            editorWidth={editorWidth}
            syncScroll={syncScroll}
            onModeChange={changeViewMode}
            onEditorWidthChange={setEditorWidth}
            onSyncScrollChange={setSyncScroll}
          />
          <EditorInputToolbar
            disabled={saving || isComposing}
            onCommand={applyEditorCommand}
          />
        </div>
        <div
          className="editor-workspace"
          data-view-mode={effectiveViewMode}
          style={
            {
              "--editor-width": `${editorWidth}fr`,
              "--preview-width": `${100 - editorWidth}fr`,
            } as CSSProperties
          }
        >
          <div className="editor-source-pane">
            <div className="source-editor-field">
              <span id="source-editor-label">AsciiDoc文書</span>
              <AsciiDocEditor
                ref={sourceEditor}
                value={form.source}
                disabled={saving}
                onChange={changeSource}
                labelledBy="source-editor-label"
                onCompositionChange={setIsComposing}
                onSave={() => editorForm.current?.requestSubmit()}
                onScroll={synchronizeFromEditor}
              />
            </div>
          </div>
          <div className="editor-divider" aria-hidden="true" />
          <div
            className="preview-scroll"
            ref={previewScroll}
            onScroll={synchronizeFromPreview}
          >
            <PreviewPanel
              body={form.source}
              html={preview.html}
              diagnostics={preview.diagnostics}
              loading={preview.loading}
              problem={preview.problem}
              onSelectDiagnostic={selectDiagnostic}
            />
          </div>
        </div>
        <div className="editor-actions">
          <button type="submit" disabled={saving || !isDirty || isComposing}>
            {saving ? "保存しています…" : "保存"}
          </button>
          <span role="status">
            {isComposing
              ? "日本語入力を確定してください。"
              : editorStatus({
                  saving,
                  isDirty,
                  failed: problem !== null,
                  conflicted: conflict !== null,
                  notice,
                })}
          </span>
        </div>
      </form>
    </section>
  );
}

const EDITOR_COMMAND_LABELS: Record<AsciiDocCommand, string> = {
  title: "題名",
  section: "節",
  list: "箇条書き",
  link: "リンク",
  "code-block": "コードブロック",
  "inline-math": "インライン数式",
  "block-math": "ブロック数式",
  "note-reference": "ノート参照",
};

function EditorInputToolbar({
  disabled,
  onCommand,
}: {
  disabled: boolean;
  onCommand: (command: AsciiDocCommand) => void;
}) {
  return (
    <div className="editor-input-toolbar" role="toolbar" aria-label="入力補助">
      {ASCII_DOC_COMMANDS.map((command) => (
        <button
          key={command}
          type="button"
          disabled={disabled}
          onClick={() => onCommand(command)}
        >
          {EDITOR_COMMAND_LABELS[command]}
        </button>
      ))}
    </div>
  );
}

function EditorViewToolbar({
  mode,
  requestedMode,
  narrow,
  editorWidth,
  syncScroll,
  onModeChange,
  onEditorWidthChange,
  onSyncScrollChange,
}: {
  mode: EditorViewMode;
  requestedMode: EditorViewMode;
  narrow: boolean;
  editorWidth: number;
  syncScroll: boolean;
  onModeChange: (mode: EditorViewMode) => void;
  onEditorWidthChange: (width: number) => void;
  onSyncScrollChange: (enabled: boolean) => void;
}) {
  const modes: ReadonlyArray<{ mode: EditorViewMode; label: string }> = [
    { mode: "write", label: "執筆" },
    { mode: "split", label: "分割" },
    { mode: "preview", label: "プレビュー" },
  ];
  return (
    <div className="editor-view-toolbar" aria-label="表示設定">
      <div className="editor-view-buttons" role="group" aria-label="表示">
        {modes.map((item) => (
          <button
            key={item.mode}
            type="button"
            aria-pressed={mode === item.mode}
            disabled={item.mode === "split" && narrow}
            onClick={() => onModeChange(item.mode)}
          >
            {item.label}
          </button>
        ))}
      </div>
      {mode === "split" && (
        <>
          <label className="editor-width-control">
            執筆欄の幅
            <input
              type="range"
              min="30"
              max="70"
              step="5"
              value={editorWidth}
              onChange={(event) =>
                onEditorWidthChange(Number(event.currentTarget.value))
              }
            />
            <output>{editorWidth}%</output>
          </label>
          <label className="scroll-sync-control">
            <input
              type="checkbox"
              checked={syncScroll}
              onChange={(event) =>
                onSyncScrollChange(event.currentTarget.checked)
              }
            />
            相対位置でスクロールを同期
          </label>
          <span className="editor-view-note">
            文書全体に対する位置の割合を合わせるため、見出しや図表の高さによって位置がずれます。
          </span>
        </>
      )}
      {narrow && requestedMode === "split" && (
        <span className="editor-view-note" role="status">
          この画面幅では執筆表示に切り替えています。
        </span>
      )}
    </div>
  );
}

function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(
    () => window.matchMedia?.(query).matches ?? false,
  );
  useEffect(() => {
    const media = window.matchMedia?.(query);
    if (!media) return;
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [query]);
  return matches;
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
