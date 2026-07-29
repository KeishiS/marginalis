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

import { asciiDocCommandEdit, type AsciiDocCommand } from "./asciiDocEditing";

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
  applyCommand: (command: AsciiDocCommand) => void;
  focus: () => void;
  selectRange: (anchor: number, head: number) => void;
  setScrollRatio: (ratio: number) => void;
}

interface AsciiDocEditorProps {
  value: string;
  disabled: boolean;
  labelledBy: string;
  onChange: (value: string) => void;
  onCompositionChange: (composing: boolean) => void;
  onSave: () => void;
  onScroll: (ratio: number) => void;
}

export const AsciiDocEditor = forwardRef<
  AsciiDocEditorHandle,
  AsciiDocEditorProps
>(function AsciiDocEditor(
  {
    value,
    disabled,
    labelledBy,
    onChange,
    onCompositionChange,
    onSave,
    onScroll,
  },
  forwardedRef,
) {
  const container = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView>(null);
  const initialValue = useRef(value);
  const initialLabel = useRef(labelledBy);
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
    editor.focus();
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
      applyCommand(command) {
        const editor = view.current;
        if (!editor || editor.state.readOnly || editor.compositionStarted) {
          return;
        }
        const range = editor.state.selection.main;
        const edit = asciiDocCommandEdit(
          command,
          editor.state.doc.toString(),
          range.anchor,
          range.head,
        );
        editor.dispatch({
          changes: {
            from: edit.from,
            to: edit.to,
            insert: edit.insert,
          },
          selection: EditorSelection.range(
            edit.selection.anchor,
            edit.selection.head,
          ),
          scrollIntoView: true,
          userEvent: "input.complete",
        });
        editor.focus();
      },
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
