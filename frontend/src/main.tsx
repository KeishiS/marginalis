import React from "react";
import { createRoot } from "react-dom/client";

import { EditorApplication } from "./EditorApplication";
import "./styles.css";

const root = document.querySelector<HTMLElement>("[data-editor-application]");

if (root) {
  createRoot(root).render(
    <React.StrictMode>
      <EditorApplication />
    </React.StrictMode>,
  );
}
