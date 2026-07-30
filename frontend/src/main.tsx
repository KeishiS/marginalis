import React from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/noto-sans-jp/wght.css";
import "@fontsource-variable/noto-sans-mono/wght.css";
import "@fontsource-variable/noto-serif-jp/wght.css";

import { Application, ApplicationConfig } from "./Application";
import "./styles.css";

const root = document.querySelector<HTMLElement>("[data-application-root]");
if (root) {
  const config = JSON.parse(
    root.dataset.applicationConfig ?? "{}",
  ) as ApplicationConfig;
  createRoot(root).render(
    <React.StrictMode>
      <Application config={config} />
    </React.StrictMode>,
  );
}
