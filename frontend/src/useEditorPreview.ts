import { useEffect, useState } from "react";

import {
  MathMacro,
  NoteDiagnostic,
  NoteSourceSpan,
  Problem,
  previewNewNote,
  previewNoteUpdate,
} from "./api";

export interface EditorPreview {
  html: string;
  mathMacros: MathMacro[];
  diagnostics: NoteDiagnostic[];
  /**
   * 現在の本文に対するspan注釈。最新の解析結果が現在の本文と一致しない間は`null`で、
   * その間の装飾は編集側が既存の装飾を編集へ追従させて保つ。
   */
  spans: NoteSourceSpan[] | null;
  loading: boolean;
  problem: Problem | null;
}

interface EditorPreviewState extends Omit<EditorPreview, "spans"> {
  spans: NoteSourceSpan[];
  /** `spans`を解析した本文。取得の失敗や進行中の値と混ざらないよう、成功時だけ更新する。 */
  spansSource: string | null;
  source: string;
}

export function useEditorPreview(
  apiBase: string,
  noteId: string | null,
  source: string,
  enabled: boolean,
  toProblem: (error: unknown) => Problem,
): EditorPreview {
  const [preview, setPreview] = useState<EditorPreviewState>({
    html: "",
    mathMacros: [],
    diagnostics: [],
    spans: [],
    spansSource: null,
    loading: false,
    problem: null,
    source: "",
  });

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    let current = true;
    const timer = window.setTimeout(() => {
      setPreview((value) => ({
        ...value,
        diagnostics: [],
        loading: true,
        problem: null,
        source,
      }));
      (noteId === null
        ? previewNewNote(apiBase, { source }, controller.signal)
        : previewNoteUpdate(apiBase, noteId, { source }, controller.signal)
      )
        .then((result) => {
          if (current) {
            setPreview({
              html: result.html,
              mathMacros: result.math_macros,
              diagnostics: result.diagnostics,
              spans: result.spans,
              spansSource: source,
              loading: false,
              problem: null,
              source,
            });
          }
        })
        .catch((error: unknown) => {
          if (current && !controller.signal.aborted) {
            setPreview((value) => ({
              ...value,
              diagnostics: [],
              loading: false,
              problem: toProblem(error),
              source,
            }));
          }
        });
    }, 350);
    return () => {
      current = false;
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [apiBase, enabled, noteId, source, toProblem]);

  const matchesCurrentSource = preview.source === source;
  return {
    html: preview.html,
    mathMacros: preview.mathMacros,
    diagnostics: matchesCurrentSource ? preview.diagnostics : [],
    spans: preview.spansSource === source ? preview.spans : null,
    loading: matchesCurrentSource ? preview.loading : false,
    problem: matchesCurrentSource ? preview.problem : null,
  };
}
