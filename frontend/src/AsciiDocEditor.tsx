import {
  Compartment,
  EditorSelection,
  EditorState,
  Transaction,
} from "@codemirror/state";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentLess,
  insertTab,
} from "@codemirror/commands";
import {
  autocompletion,
  completionKeymap,
  type CompletionSource,
} from "@codemirror/autocomplete";
import {
  type Diagnostic,
  lintGutter,
  lintKeymap,
  setDiagnostics,
} from "@codemirror/lint";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import {
  crosshairCursor,
  drawSelection,
  dropCursor,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
} from "react";

import {
  type MathMacro,
  type NoteDiagnostic,
  type NoteSourceSpan,
} from "./api";
import { livePreview, setLiveSpans, toLiveSpans } from "./editor/livePreview";
import { canSelectDiagnostic, diagnosticMessage } from "./editorPresentation";
import { utf8ByteOffsetToTextOffset } from "./textPosition";

export interface AsciiDocEditorHandle {
  focus: () => void;
  selectRange: (anchor: number, head: number) => void;
  insertText: (text: string) => void;
}

interface AsciiDocEditorProps {
  value: string;
  diagnostics: NoteDiagnostic[];
  /** 現在の本文に対するspan注釈。最新の解析が本文と一致しない間は`null`。 */
  spans: NoteSourceSpan[] | null;
  /** 数式widgetへ局所適用する、ノート所有者の数式マクロ。 */
  mathMacros: MathMacro[];
  /** 装飾表示の有効・無効。無効時は素の原文を表示する。 */
  livePreviewEnabled: boolean;
  disabled: boolean;
  labelledBy: string;
  onChange: (value: string) => void;
  onCompositionChange: (composing: boolean) => void;
  onSave: () => void;
  onFilesDropped?: (files: File[]) => void;
  styleNonce: string;
  completionSources?: CompletionSource[];
}

export const AsciiDocEditor = forwardRef<
  AsciiDocEditorHandle,
  AsciiDocEditorProps
>(function AsciiDocEditor(
  {
    value,
    diagnostics,
    spans,
    mathMacros,
    livePreviewEnabled,
    disabled,
    labelledBy,
    onChange,
    onCompositionChange,
    onSave,
    onFilesDropped,
    styleNonce,
    completionSources,
  },
  forwardedRef,
) {
  const container = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView>(null);
  const initialValue = useRef(value);
  const initialLabel = useRef(labelledBy);
  const initialStyleNonce = useRef(styleNonce);
  const initialCompletionSources = useRef(completionSources);
  const onChangeRef = useRef(onChange);
  const onCompositionChangeRef = useRef(onCompositionChange);
  const onSaveRef = useRef(onSave);
  const onFilesDroppedRef = useRef(onFilesDropped);
  const readOnly = useRef(new Compartment());
  const editable = useRef(new Compartment());
  const livePreviewConfiguration = useRef(new Compartment());
  const initialLivePreviewEnabled = useRef(livePreviewEnabled);

  useLayoutEffect(() => {
    onChangeRef.current = onChange;
    onCompositionChangeRef.current = onCompositionChange;
    onSaveRef.current = onSave;
    onFilesDroppedRef.current = onFilesDropped;
  }, [onChange, onCompositionChange, onSave, onFilesDropped]);

  useLayoutEffect(() => {
    const parent = container.current;
    if (!parent) return;
    const editor = new EditorView({
      parent,
      state: EditorState.create({
        doc: initialValue.current,
        extensions: [
          lineNumbers(),
          lintGutter(),
          highlightActiveLineGutter(),
          highlightSpecialChars(),
          history(),
          drawSelection(),
          dropCursor(),
          rectangularSelection(),
          crosshairCursor(),
          highlightActiveLine(),
          highlightSelectionMatches(),
          // 候補の取得は各sourceが担い、対象外の文脈ではnullを返して黙る。
          ...(initialCompletionSources.current?.length
            ? [autocompletion({ override: initialCompletionSources.current })]
            : []),
          EditorView.cspNonce.of(initialStyleNonce.current),
          EditorView.lineWrapping,
          EditorState.tabSize.of(4),
          readOnly.current.of(EditorState.readOnly.of(false)),
          editable.current.of(EditorView.editable.of(true)),
          livePreviewConfiguration.current.of(
            initialLivePreviewEnabled.current
              ? livePreview({ styleNonce: initialStyleNonce.current })
              : [],
          ),
          EditorView.contentAttributes.of({
            "aria-labelledby": initialLabel.current,
            "aria-multiline": "true",
            spellcheck: "true",
          }),
          keymap.of([
            {
              key: "Mod-s",
              run: (currentView) => {
                if (currentView.compositionStarted) return false;
                onSaveRef.current();
                return true;
              },
            },
            { key: "Tab", run: insertTab, shift: indentLess },
            ...completionKeymap,
            ...lintKeymap,
            ...searchKeymap,
            ...historyKeymap,
            ...defaultKeymap,
          ]),
          EditorView.domEventHandlers({
            compositionstart: () => {
              onCompositionChangeRef.current(true);
              return false;
            },
            compositionend: () => {
              queueMicrotask(() => onCompositionChangeRef.current(false));
              return false;
            },
            drop: (event) => {
              const files = Array.from(event.dataTransfer?.files ?? []);
              if (files.length === 0 || !onFilesDroppedRef.current)
                return false;
              event.preventDefault();
              onFilesDroppedRef.current(files);
              return true;
            },
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              onChangeRef.current(update.state.doc.toString());
            }
          }),
        ],
      }),
    });
    view.current = editor;
    editor.contentDOM.focus({ preventScroll: true });
    return () => {
      editor.destroy();
      view.current = null;
    };
  }, []);

  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    const current = editor.state.doc.toString();
    if (current === value) return;
    editor.dispatch({
      changes: { from: 0, to: current.length, insert: value },
      annotations: Transaction.addToHistory.of(false),
    });
  }, [value]);

  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    editor.dispatch(
      setDiagnostics(
        editor.state,
        diagnostics.flatMap((diagnostic) =>
          toCodeMirrorDiagnostic(value, diagnostic),
        ),
      ),
    );
  }, [diagnostics, value]);

  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    editor.dispatch({
      effects: [
        readOnly.current.reconfigure(EditorState.readOnly.of(disabled)),
        editable.current.reconfigure(EditorView.editable.of(!disabled)),
      ],
    });
  }, [disabled]);

  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    editor.dispatch({
      effects: livePreviewConfiguration.current.reconfigure(
        livePreviewEnabled
          ? livePreview({ styleNonce: initialStyleNonce.current })
          : [],
      ),
    });
  }, [livePreviewEnabled]);

  useEffect(() => {
    const editor = view.current;
    if (!editor || spans === null) return;
    // span注釈は解析した本文とだけ整合する。編集欄が既に先へ進んでいる場合は捨て、
    // 既存の装飾を編集への追従に任せる。
    if (editor.state.doc.toString() !== value) return;
    editor.dispatch({
      effects: setLiveSpans.of({
        spans: toLiveSpans(value, spans),
        mathMacros,
      }),
    });
  }, [spans, value, mathMacros, livePreviewEnabled]);

  useImperativeHandle(
    forwardedRef,
    () => ({
      focus() {
        view.current?.focus();
      },
      selectRange(anchor, head) {
        const editor = view.current;
        if (!editor) return;
        const length = editor.state.doc.length;
        const safeAnchor = Math.max(0, Math.min(anchor, length));
        const safeHead = Math.max(0, Math.min(head, length));
        const range = EditorSelection.range(safeAnchor, safeHead);
        editor.dispatch({
          selection: range,
          effects: EditorView.scrollIntoView(range, { y: "center" }),
        });
        editor.focus();
      },
      insertText(text) {
        const editor = view.current;
        if (!editor) return;
        const range = editor.state.selection.main;
        editor.dispatch({
          changes: { from: range.from, to: range.to, insert: text },
          selection: { anchor: range.from + text.length },
        });
        editor.focus();
      },
    }),
    [],
  );

  return <div className="ascii-doc-editor" ref={container} />;
});

function toCodeMirrorDiagnostic(
  source: string,
  diagnostic: NoteDiagnostic,
): Diagnostic[] {
  if (!canSelectDiagnostic(diagnostic)) return [];
  const start = utf8ByteOffsetToTextOffset(source, diagnostic.span?.start ?? 0);
  const end = utf8ByteOffsetToTextOffset(source, diagnostic.span?.end ?? start);
  return [
    {
      from: start,
      to: Math.max(start, end),
      severity:
        diagnostic.severity === "information" ? "info" : diagnostic.severity,
      source: `Marginalis (${diagnostic.code})`,
      message: diagnosticMessage(diagnostic.code, diagnostic.message),
    },
  ];
}
