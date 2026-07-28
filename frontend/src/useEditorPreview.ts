import { useEffect, useState } from "react";

import { Problem, previewNote } from "./api";

export interface EditorPreview {
  html: string;
  loading: boolean;
  problem: Problem | null;
}

export function useEditorPreview(
  apiBase: string,
  source: string,
  enabled: boolean,
  toProblem: (error: unknown) => Problem,
): EditorPreview {
  const [preview, setPreview] = useState<EditorPreview>({
    html: "",
    loading: false,
    problem: null,
  });

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    let current = true;
    const timer = window.setTimeout(() => {
      setPreview((value) => ({ ...value, loading: true }));
      previewNote(apiBase, { source }, controller.signal)
        .then((result) => {
          if (current) {
            setPreview({ html: result.html, loading: false, problem: null });
          }
        })
        .catch((error: unknown) => {
          if (current && !controller.signal.aborted) {
            setPreview((value) => ({
              ...value,
              loading: false,
              problem: toProblem(error),
            }));
          }
        });
    }, 350);
    return () => {
      current = false;
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [apiBase, enabled, source, toProblem]);

  return preview;
}
