import React from "react";
import { createRoot } from "react-dom/client";

import { EditorApplication } from "./EditorApplication";
import "./styles.css";

const root = document.querySelector<HTMLElement>("[data-editor-application]");

if (root) {
  const mode = root.dataset.mode;
  if (mode !== "create" && mode !== "edit") {
    throw new Error("editor mode is missing");
  }
  createRoot(root).render(
    <React.StrictMode>
      <EditorApplication
        config={{
          mode,
          noteId: root.dataset.noteId ?? "",
          apiBase: root.dataset.apiBase ?? "",
          basePath: root.dataset.basePath ?? "/",
        }}
      />
    </React.StrictMode>,
  );
}
