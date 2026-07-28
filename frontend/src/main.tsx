import React from "react";
import { createRoot } from "react-dom/client";

import { EditorApplication } from "./EditorApplication";
import { AccessControl } from "./AccessControl";
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

const accessRoot = document.querySelector<HTMLElement>("[data-access-root]");
if (accessRoot) {
  const config = JSON.parse(accessRoot.dataset.accessConfig ?? "{}") as {
    apiBase: string;
    noteId: string;
    revision: number;
  };
  createRoot(accessRoot).render(
    <AccessControl
      apiBase={config.apiBase}
      noteId={config.noteId}
      revision={config.revision}
      onRevision={(revision) => {
        config.revision = revision;
      }}
    />,
  );
}
