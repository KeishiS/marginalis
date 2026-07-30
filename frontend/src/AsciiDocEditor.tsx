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

import { type NoteDiagnostic } from "./api";
import { canSelectDiagnostic, diagnosticMessage } from "./editorPresentation";
import { utf8ByteOffsetToTextOffset } from "./textPosition";

const adaptiveTheme = EditorView.theme({
  "&": {
    backgroundColor: "light-dark(#ffffff, #111418)",
    color: "light-dark(#20242a, #f2f4f7)",
  },
  ".cm-content": {
    caretColor: "currentColor",
  },
  ".cm-cursor": {
    borderLeftColor: "currentColor",
  },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
    {
      backgroundColor: "light-dark(#c9dcf8, #3b506d)",
    },
  ".cm-activeLine": {
    backgroundColor: "light-dark(#edf4fd, #202832)",
  },
  ".cm-activeLineGutter": {
    backgroundColor: "light-dark(#dce8f6, #2a3440)",
  },
});

export interface AsciiDocEditorHandle {
  focus: () => void;
  selectRange: (anchor: number, head: number) => void;
  setScrollRatio: (ratio: number) => void;
}

interface AsciiDocEditorProps {
  value: string;
  diagnostics: NoteDiagnostic[];
  disabled: boolean;
  labelledBy: string;
  onChange: (value: string) => void;
  onCompositionChange: (composing: boolean) => void;
  onSave: () => void;
  onScroll: (ratio: number) => void;
  styleNonce: string;
}

export const AsciiDocEditor = forwardRef<
  AsciiDocEditorHandle,
  AsciiDocEditorProps
>(function AsciiDocEditor(
  {
    value,
    diagnostics,
    disabled,
    labelledBy,
    onChange,
    onCompositionChange,
    onSave,
    onScroll,
    styleNonce,
  },
  forwardedRef,
) {
  const container = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView>(null);
  const initialValue = useRef(value);
  const initialLabel = useRef(labelledBy);
  const initialStyleNonce = useRef(styleNonce);
  const onChangeRef = useRef(onChange);
  const onCompositionChangeRef = useRef(onCompositionChange);
  const onSaveRef = useRef(onSave);
  const onScrollRef = useRef(onScroll);
  const readOnly = useRef(new Compartment());
  const editable = useRef(new Compartment());

  useLayoutEffect(() => {
    onChangeRef.current = onChange;
    onCompositionChangeRef.current = onCompositionChange;
    onSaveRef.current = onSave;
    onScrollRef.current = onScroll;
  }, [onChange, onCompositionChange, onSave, onScroll]);

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
          adaptiveTheme,
          EditorView.cspNonce.of(initialStyleNonce.current),
          EditorView.lineWrapping,
          EditorState.tabSize.of(4),
          readOnly.current.of(EditorState.readOnly.of(false)),
          editable.current.of(EditorView.editable.of(true)),
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
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              onChangeRef.current(update.state.doc.toString());
            }
          }),
        ],
      }),
    });
    const reportScroll = () => {
      onScrollRef.current(scrollRatio(editor.scrollDOM));
    };
    editor.scrollDOM.addEventListener("scroll", reportScroll, {
      passive: true,
    });
    view.current = editor;
    editor.contentDOM.focus({ preventScroll: true });
    return () => {
      editor.scrollDOM.removeEventListener("scroll", reportScroll);
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
      setScrollRatio(ratio) {
        const editor = view.current;
        if (!editor) return;
        const scroll = editor.scrollDOM;
        const maximum = Math.max(0, scroll.scrollHeight - scroll.clientHeight);
        scroll.scrollTop = clampRatio(ratio) * maximum;
      },
    }),
    [],
  );

  return <div className="ascii-doc-editor" ref={container} />;
});

function scrollRatio(element: HTMLElement): number {
  const maximum = element.scrollHeight - element.clientHeight;
  return maximum > 0 ? clampRatio(element.scrollTop / maximum) : 0;
}

function clampRatio(value: number): number {
  return Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
}

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
