import React from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/noto-sans-jp/wght.css";
import "@fontsource-variable/noto-sans-mono/wght.css";
import "@fontsource-variable/noto-serif-jp/wght.css";

import { parseApplicationConfig } from "./api";
import { Application } from "./Application";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/editor.css";
import "./styles/content.css";

const root = document.querySelector<HTMLElement>("[data-application-root]");
if (root) {
  // 起動設定もサーバーとの公開契約である。REST応答と同じく実行時に検査し、
  // 解釈できない場合は画面を描画せず理由を表示する。
  try {
    const config = parseApplicationConfig(
      JSON.parse(root.dataset.applicationConfig ?? "null"),
    );
    createRoot(root).render(
      <React.StrictMode>
        <Application config={config} />
      </React.StrictMode>,
    );
  } catch {
    root.textContent =
      "画面の設定を読み取れませんでした。再読み込みしても解決しない場合は管理者へ連絡してください。";
  }
}
