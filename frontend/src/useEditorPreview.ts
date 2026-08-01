import { useEffect, useState } from "react";

import {
  NoteDiagnostic,
  Problem,
  previewNewNote,
  previewNoteUpdate,
} from "./api";

export interface EditorPreview {
  html: string;
  diagnostics: NoteDiagnostic[];
  loading: boolean;
  problem: Problem | null;
}

interface EditorPreviewState extends EditorPreview {
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
    diagnostics: [],
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
              diagnostics: result.diagnostics,
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
    diagnostics: matchesCurrentSource ? preview.diagnostics : [],
    loading: matchesCurrentSource ? preview.loading : false,
    problem: matchesCurrentSource ? preview.problem : null,
  };
}
