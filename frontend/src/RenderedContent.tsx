import { useEffect, useRef, useState } from "react";

import mathJaxUrl from "mathjax/tex-svg.js?url";
import { enhanceCodeBlocks, prepareMath } from "./renderedContentEnhancement";

interface MathJaxRuntime {
  startup: { promise: Promise<void> };
  typesetClear?: (elements: HTMLElement[]) => void;
  typesetPromise: (elements: HTMLElement[]) => Promise<void>;
}

declare global {
  interface Window {
    MathJax?: MathJaxRuntime | Record<string, unknown>;
  }
}

let mathJaxLoader: Promise<MathJaxRuntime> | null = null;

export function RenderedContent({
  html,
  preview = false,
}: {
  html: string;
  preview?: boolean;
}) {
  const container = useRef<HTMLDivElement>(null);
  const [failedHtml, setFailedHtml] = useState<string | null>(null);

  useEffect(() => {
    const element = container.current;
    if (!element) return;
    enhanceCodeBlocks(element);
    if (!prepareMath(element)) return;
    if (failedHtml === html) {
      element.dataset.mathStatus = "failed";
      return;
    }

    let current = true;
    void loadMathJax()
      .then(async (mathJax) => {
        if (!current) return;
        mathJax.typesetClear?.([element]);
        await mathJax.typesetPromise([element]);
      })
      .catch((error: unknown) => {
        if (current) {
          element.dataset.mathStatus = "failed";
          console.error("MathJaxによる数式の組版に失敗しました。", error);
          setFailedHtml(html);
        }
      });
    return () => {
      current = false;
    };
  }, [failedHtml, html]);

  return (
    <>
      {failedHtml === html && (
        <p className="math-rendering-error" role="alert">
          数式を描画できませんでした。LaTeXの内容を確認するか、画面を再読み込みしてください。
        </p>
      )}
      <div
        ref={container}
        className={`rendered-content${preview ? " preview-content" : ""}`}
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </>
  );
}

async function loadMathJax(): Promise<MathJaxRuntime> {
  if (isMathJaxRuntime(window.MathJax)) return window.MathJax;
  if (mathJaxLoader) return mathJaxLoader;

  mathJaxLoader = new Promise<MathJaxRuntime>((resolve, reject) => {
    window.MathJax = {
      startup: { typeset: false },
      // enrichmentはexplorerを有効化し、読み上げ用領域のinline styleを挿入する。
      // Marginalisはstyle-srcを同一originに限定するため、任意の対話機能を無効にする。
      options: {
        enableEnrichment: false,
        enableExplorer: false,
        enableMenu: false,
        menuOptions: { settings: { enrich: false } },
      },
      svg: { fontCache: "local" },
    };
    const script = document.createElement("script");
    script.src = mathJaxUrl;
    script.async = true;
    script.addEventListener("load", () => {
      if (!isMathJaxRuntime(window.MathJax)) {
        reject(new Error("MathJaxを初期化できませんでした。"));
        return;
      }
      void window.MathJax.startup.promise.then(() =>
        resolve(window.MathJax as MathJaxRuntime),
      );
    });
    script.addEventListener("error", () =>
      reject(new Error("MathJaxを読み込めませんでした。")),
    );
    document.head.append(script);
  });
  return mathJaxLoader;
}

function isMathJaxRuntime(value: unknown): value is MathJaxRuntime {
  return (
    typeof value === "object" &&
    value !== null &&
    "typesetPromise" in value &&
    typeof value.typesetPromise === "function" &&
    "startup" in value
  );
}
