import React from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/noto-sans-jp/wght.css";
import "@fontsource-variable/noto-sans-mono/wght.css";
import "@fontsource-variable/noto-serif-jp/wght.css";

import { parseApplicationConfig } from "./api";
import { Application } from "./Application";
import "./styles/globals.css";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/editor.css";
import "./styles/graph.css";
import "./styles/content.css";

const root = document.querySelector<HTMLElement>("[data-application-root]");
if (root) {
  // 起動設定もサーバーとの公開契約である。REST応答と同じく実行時に検査し、
  // 解釈できない場合は画面を描画せず理由を表示する。
  try {
    const config = parseApplicationConfig(
      JSON.parse(root.dataset.applicationConfig ?? "null"),
    );
    // Radix(react-remove-scroll)はモーダル表示中に<style>要素を挿入する。
    // CSPはnonce付きのstyle要素だけを許可するため、MathJaxと同じ
    // サーバー発行のnonceをグローバル経由で渡す。
    (window as { __webpack_nonce__?: string }).__webpack_nonce__ =
      config.styleNonce;
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
