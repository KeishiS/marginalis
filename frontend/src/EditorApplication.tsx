import {
  FormEvent,
  UIEvent,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";

import { AsciiDocEditor, type AsciiDocEditorHandle } from "./AsciiDocEditor";
import {
  Problem,
  NoteDiagnostic,
  createNote,
  readNote,
  updateNote,
} from "./api";
import { utf8ByteOffsetToTextOffset } from "./textPosition";
import { editorReducer, initialEditorState, noteToForm } from "./editorState";
import {
  editorActivityReducer,
  initialEditorActivityState,
} from "./editorActivityState";
import { useEditorPreview } from "./useEditorPreview";
import { editorStatus, toProblem } from "./editorPresentation";
import { editPath, listPath, notePath } from "./paths";
import { ConflictPanel } from "./editor/ConflictPanel";
import { EditorViewToolbar } from "./editor/EditorViewToolbar";
import { EditorViewMode } from "./editor/viewMode";
import { useMediaQuery } from "./useMediaQuery";
import { PreviewPanel } from "./editor/PreviewPanel";
import { ProblemMessage } from "./editor/ProblemMessage";

export interface EditorConfig {
  mode: "create" | "edit";
  noteId: string;
  apiBase: string;
  basePath: string;
  search: string;
  styleNonce: string;
}

const SAVE_TOAST_DURATION_MS = 4_000;

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
  const toastSequence = useRef(0);
  const [isComposing, setIsComposing] = useState(false);
  const [saveToast, setSaveToast] = useState<number | null>(null);
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
    revision === null ? null : noteId,
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

  useEffect(() => {
    if (saveToast === null) return;
    const timeout = window.setTimeout(
      () => setSaveToast(null),
      SAVE_TOAST_DURATION_MS,
    );
    return () => window.clearTimeout(timeout);
  }, [saveToast]);

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
    setSaveToast(null);
    dispatchActivity({ type: "save-started" });
    try {
      const note =
        revision === null
          ? await createNote(config.apiBase, draft)
          : await updateNote(config.apiBase, noteId, draft, revision);
      dispatch({ type: "accept-note", note });
      dispatchActivity({ type: "save-succeeded" });
      toastSequence.current += 1;
      setSaveToast(toastSequence.current);
      if (revision === null) {
        window.history.replaceState(null, "", editPath(config, note.note_id));
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
        <a href={listPath(config)}>一覧へ戻る</a>
      </section>
    );
  }

  return (
    <section className="editor-page" aria-labelledby="editor-heading">
      <div className="page-heading editor-heading">
        <div>
          <p className="page-eyebrow">Editor</p>
          <h1 id="editor-heading">
            {revision === null ? "ノートの作成" : "ノートの編集"}
          </h1>
          {revision !== null && (
            <p className="metadata">更新番号: {revision}</p>
          )}
        </div>
        <a
          className="button button-secondary"
          href={noteId ? notePath(config, noteId) : listPath(config)}
        >
          {noteId ? "閲覧画面へ戻る" : "一覧へ戻る"}
        </a>
      </div>

      {problem && (
        <ProblemMessage
          problem={problem}
          heading="保存できませんでした"
          headingId="save-problem-heading"
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
        <div className="editor-controls surface">
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
        </div>
        <div
          className="editor-workspace"
          data-editor-width={editorWidth}
          data-view-mode={effectiveViewMode}
        >
          <div className="editor-source-pane">
            <div className="source-editor-field">
              <span id="source-editor-label">AsciiDoc文書</span>
              <AsciiDocEditor
                ref={sourceEditor}
                value={form.source}
                diagnostics={preview.diagnostics}
                disabled={saving}
                onChange={changeSource}
                labelledBy="source-editor-label"
                onCompositionChange={setIsComposing}
                onSave={() => editorForm.current?.requestSubmit()}
                onScroll={synchronizeFromEditor}
                styleNonce={config.styleNonce}
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
              active={effectiveViewMode !== "write"}
              html={preview.html}
              diagnostics={preview.diagnostics}
              loading={preview.loading}
              mathMacros={preview.mathMacros}
              problem={preview.problem}
              onSelectDiagnostic={selectDiagnostic}
              styleNonce={config.styleNonce}
            />
          </div>
        </div>
        <div className="editor-actions">
          <button
            className="button button-primary"
            type="submit"
            disabled={saving || !isDirty || isComposing}
          >
            {saving ? "保存しています…" : "保存"}
          </button>
          <span className="editor-status" role="status">
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
      <div className="toast-region" aria-live="polite" aria-atomic="true">
        {saveToast !== null && (
          <div className="toast toast-success">
            <span className="toast-mark" aria-hidden="true">
              ✓
            </span>
            <div>
              <p className="toast-title">保存しました。</p>
              <p className="toast-description">変更内容は最新です。</p>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
