import {
  FormEvent,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";

import { Button } from "@/components/ui/button";

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
import {
  citationKeyCompletionSource,
  createCitationKeyLoader,
  createNoteCandidateLoader,
  createTagCandidateLoader,
  noteReferenceCompletionSource,
  tagCompletionSource,
} from "./editor/completion";
import { EditorViewToolbar } from "./editor/EditorViewToolbar";
import { TemplatePicker } from "./editor/TemplatePicker";
import { EditorViewMode } from "./editor/viewMode";
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
  const toastSequence = useRef(0);
  const [isComposing, setIsComposing] = useState(false);
  const [saveToast, setSaveToast] = useState<number | null>(null);
  const [viewMode, setViewMode] = useState<EditorViewMode>("write");
  const [livePreviewEnabled, setLivePreviewEnabled] = useState(true);
  const isDirty = useMemo(
    () => JSON.stringify(form) !== JSON.stringify(baseline),
    [baseline, form],
  );
  const draft = useMemo(() => ({ source: form.source }), [form.source]);
  const completionSources = useMemo(
    () => [
      tagCompletionSource(createTagCandidateLoader(config.apiBase)),
      noteReferenceCompletionSource(createNoteCandidateLoader(config.apiBase)),
      citationKeyCompletionSource(createCitationKeyLoader(config.apiBase)),
    ],
    [config.apiBase],
  );
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
    if (viewMode === "preview") {
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
        <h1 id="editor-heading" className="text-xl font-bold">
          ノートの編集
        </h1>
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
    <section className="grid gap-5" aria-labelledby="editor-heading">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <p className="m-0 text-xs font-bold tracking-[0.12em] text-primary uppercase">
            Editor
          </p>
          <h1
            id="editor-heading"
            className="m-0 text-(length:--text-note-title) leading-tight font-bold tracking-[-0.035em]"
          >
            {revision === null ? "ノートの作成" : "ノートの編集"}
          </h1>
          {revision !== null && (
            <p className="mt-1 mb-0 text-sm text-muted-foreground">
              更新番号: {revision}
            </p>
          )}
        </div>
        <Button variant="outline" asChild>
          <a href={noteId ? notePath(config, noteId) : listPath(config)}>
            {noteId ? "閲覧画面へ戻る" : "一覧へ戻る"}
          </a>
        </Button>
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
        <div className="grid gap-3 rounded-md border bg-card p-3 shadow-xs">
          <EditorViewToolbar
            mode={viewMode}
            onModeChange={changeViewMode}
            livePreviewEnabled={livePreviewEnabled}
            onLivePreviewChange={setLivePreviewEnabled}
          />
          {config.mode === "create" && (
            <TemplatePicker
              apiBase={config.apiBase}
              disabled={saving}
              dirty={isDirty}
              onApply={changeSource}
            />
          )}
        </div>
        <div className="editor-workspace" data-view-mode={viewMode}>
          <div className="editor-source-pane">
            <div className="source-editor-field">
              <span id="source-editor-label">AsciiDoc文書</span>
              <AsciiDocEditor
                ref={sourceEditor}
                completionSources={completionSources}
                value={form.source}
                diagnostics={preview.diagnostics}
                spans={preview.spans}
                mathMacros={preview.mathMacros}
                livePreviewEnabled={livePreviewEnabled}
                disabled={saving}
                onChange={changeSource}
                labelledBy="source-editor-label"
                onCompositionChange={setIsComposing}
                onSave={() => editorForm.current?.requestSubmit()}
                styleNonce={config.styleNonce}
              />
            </div>
          </div>
          <div className="preview-scroll">
            <PreviewPanel
              active={viewMode !== "write"}
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
          <Button type="submit" disabled={saving || !isDirty || isComposing}>
            {saving ? "保存しています…" : "保存"}
          </Button>
          <span className="text-sm text-muted-foreground" role="status">
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
      <div
        data-slot="toast-region"
        className="pointer-events-none fixed end-5 top-20 z-50 max-[60rem]:start-3 max-[60rem]:end-3 max-[60rem]:top-[4.75rem]"
        aria-live="polite"
        aria-atomic="true"
      >
        {saveToast !== null && (
          <div
            data-slot="toast"
            className="flex max-w-[22rem] items-start gap-3 rounded-md border bg-card p-3 shadow-lg max-[60rem]:w-full max-[60rem]:max-w-none"
          >
            <span
              className="grid size-6 shrink-0 place-items-center rounded-full bg-success text-xs font-bold text-primary-foreground"
              aria-hidden="true"
            >
              ✓
            </span>
            <div>
              <p className="m-0 font-bold">保存しました。</p>
              <p className="m-0 text-sm text-muted-foreground">
                変更内容は最新です。
              </p>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
