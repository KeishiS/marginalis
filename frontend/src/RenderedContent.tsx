import { useEffect, useLayoutEffect, useRef, useState } from "react";

import boldsymbolUrl from "mathjax/input/tex/extensions/boldsymbol.js?url&no-inline";
import mathtoolsUrl from "mathjax/input/tex/extensions/mathtools.js?url&no-inline";
import mathJaxUrl from "mathjax/tex-svg.js?url";
import { MathMacro } from "./api";
import {
  enhanceSourceBlocks,
  prepareMath,
  wrapTables,
} from "./renderedContentEnhancement";

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
let mathJaxTypesetQueue: Promise<void> = Promise.resolve();
const NO_MATH_MACROS: MathMacro[] = [];
const ENABLED_TEX_PACKAGES = [
  "base",
  "ams",
  "newcommand",
  "textmacros",
  "noundefined",
  "configmacros",
  "boldsymbol",
  "mathtools",
] as const;

export function RenderedContent({
  html,
  styleNonce,
  preview = false,
  active = true,
  mathMacros = NO_MATH_MACROS,
}: {
  html: string;
  styleNonce: string;
  preview?: boolean;
  active?: boolean;
  mathMacros?: MathMacro[];
}) {
  const container = useRef<HTMLDivElement>(null);
  const [failedHtml, setFailedHtml] = useState<string | null>(null);

  useLayoutEffect(() => {
    const element = container.current;
    // MathJaxとコード表示処理が子要素を変更するため、HTMLが変わった時だけ置き換える。
    if (element) element.innerHTML = html;
  }, [html]);

  useEffect(() => {
    const element = container.current;
    if (!element || !active) return;
    enhanceSourceBlocks(element);
    wrapTables(element);
    if (!prepareMath(element)) return;
    if (failedHtml === html) {
      element.dataset.mathStatus = "failed";
      return;
    }

    let current = true;
    // 組版中に表示対象が変わっても古い処理が現在のDOMを変更しないよう、複製上で処理する。
    const staging = element.cloneNode(true) as HTMLElement;
    void loadMathJax(styleNonce, mathMacros)
      .then((mathJax) =>
        enqueueMathJaxTypeset(async () => {
          let cleared = false;
          try {
            await mathJax.typesetPromise([staging]);
            const renderedNodes = Array.from(staging.childNodes);
            mathJax.typesetClear?.([staging]);
            cleared = true;
            const rendered = document.createDocumentFragment();
            rendered.append(...renderedNodes);
            return rendered;
          } finally {
            if (!cleared) mathJax.typesetClear?.([staging]);
          }
        }),
      )
      .then((rendered) => {
        if (!current) return;
        // 文字列へ戻すとMathJaxのstyle属性を再解釈し、CSP違反になるため、組み立てたnodeを移す。
        element.replaceChildren(rendered);
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
  }, [active, failedHtml, html, mathMacros, styleNonce]);

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
      />
    </>
  );
}

function enqueueMathJaxTypeset<T>(task: () => Promise<T>): Promise<T> {
  const result = mathJaxTypesetQueue.then(task);
  mathJaxTypesetQueue = result.then(
    () => undefined,
    () => undefined,
  );
  return result;
}

async function loadMathJax(
  styleNonce: string,
  mathMacros: MathMacro[],
): Promise<MathJaxRuntime> {
  if (isMathJaxRuntime(window.MathJax)) return window.MathJax;
  if (mathJaxLoader) return mathJaxLoader;

  mathJaxLoader = new Promise<MathJaxRuntime>((resolve, reject) => {
    const mathJaxScriptUrl = new URL(mathJaxUrl, document.baseURI);
    const boldsymbolScriptUrl = new URL(boldsymbolUrl, document.baseURI);
    const mathtoolsScriptUrl = new URL(mathtoolsUrl, document.baseURI);
    const fontDirectory = new URL("mathjax-fonts", mathJaxScriptUrl).toString();
    window.MathJax = {
      startup: {
        typeset: false,
        ready: () => initializeMathJaxWithStyleNonce(styleNonce),
      },
      loader: {
        load: ["[tex]/boldsymbol", "[tex]/mathtools"],
        paths: { fonts: fontDirectory },
        source: {
          "[tex]/boldsymbol": boldsymbolScriptUrl.toString(),
          "[tex]/mathtools": mathtoolsScriptUrl.toString(),
        },
      },
      // enrichmentはexplorerを有効化し、読み上げ用領域のinline styleを挿入する。
      // Marginalisはstyle-srcを同一originに限定するため、任意の対話機能を無効にする。
      options: {
        enableEnrichment: false,
        enableExplorer: false,
        enableMenu: false,
        menuOptions: { settings: { enrich: false } },
      },
      tex: {
        maxMacros: 1000,
        packages: [...ENABLED_TEX_PACKAGES],
        macros: Object.fromEntries(
          mathMacros.map((macro) => [
            macro.name,
            macro.argument_count === 0
              ? macro.replacement
              : [macro.replacement, macro.argument_count],
          ]),
        ),
      },
      svg: { fontCache: "local" },
    };
    const script = document.createElement("script");
    script.src = mathJaxScriptUrl.toString();
    script.async = true;
    script.addEventListener("load", () => {
      void waitForMathJaxRuntime().then(resolve, reject);
    });
    script.addEventListener("error", () =>
      reject(new Error("MathJaxを読み込めませんでした。")),
    );
    document.head.append(script);
  });
  return mathJaxLoader;
}

async function waitForMathJaxRuntime(): Promise<MathJaxRuntime> {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const startupPromise = mathJaxStartupPromise(window.MathJax);
    if (startupPromise) {
      await startupPromise;
      if (isMathJaxRuntime(window.MathJax)) return window.MathJax;
    }
    await new Promise((resolve) => window.setTimeout(resolve, 10));
  }
  throw new Error("MathJaxを初期化できませんでした。");
}

function mathJaxStartupPromise(value: unknown): Promise<void> | null {
  if (typeof value !== "object" || value === null) return null;
  const startup = (value as { startup?: unknown }).startup;
  if (
    (typeof startup !== "object" && typeof startup !== "function") ||
    startup === null
  )
    return null;
  const promise = (startup as { promise?: unknown }).promise;
  return promise instanceof Promise ? promise : null;
}

function initializeMathJaxWithStyleNonce(styleNonce: string) {
  const startup = (window.MathJax as { startup?: unknown })?.startup;
  if (!hasMathJaxDefaultReady(startup)) {
    throw new Error("MathJaxの起動処理を初期化できませんでした。");
  }
  const appendChild = document.head.appendChild.bind(document.head);
  document.head.appendChild = ((child: Node) => {
    if (child instanceof HTMLStyleElement) child.nonce = styleNonce;
    return appendChild(child);
  }) as typeof document.head.appendChild;
  try {
    restrictMathJaxTexPackages(window.MathJax);
    startup.defaultReady();
  } finally {
    document.head.appendChild = appendChild;
  }
  if (!hasMathJaxAdaptor(startup)) {
    throw new Error("MathJaxのDOM処理を初期化できませんでした。");
  }
  const createNode = startup.adaptor.node.bind(startup.adaptor);
  const appendNode = startup.adaptor.append.bind(startup.adaptor);
  startup.adaptor.node = (
    kind: string,
    attributes: Record<string, unknown> = {},
    ...rest: unknown[]
  ) =>
    createNode(
      kind,
      kind === "style" ? { ...attributes, nonce: styleNonce } : attributes,
      ...rest,
    );
  startup.adaptor.append = (parent: unknown, child: unknown) => {
    if (child instanceof HTMLStyleElement) child.nonce = styleNonce;
    return appendNode(parent, child);
  };
}

function restrictMathJaxTexPackages(value: unknown) {
  // MathJax 4は起動前のpackages配列を既定一覧への追加として解釈するため、
  // TeX入力処理を構築する直前に、実際に使う一覧を許可した値だけへ置き換える。
  if (typeof value !== "object" || value === null) {
    throw new Error("MathJaxのTeX設定を初期化できませんでした。");
  }
  const config = (value as { config?: unknown }).config;
  if (typeof config !== "object" || config === null) {
    throw new Error("MathJaxのTeX設定を初期化できませんでした。");
  }
  const tex = (config as { tex?: unknown }).tex;
  if (typeof tex !== "object" || tex === null) {
    throw new Error("MathJaxのTeX設定を初期化できませんでした。");
  }
  (tex as { packages: string[] }).packages = [...ENABLED_TEX_PACKAGES];
}

function hasMathJaxDefaultReady(
  value: unknown,
): value is { defaultReady: () => void } {
  return (
    (typeof value === "object" || typeof value === "function") &&
    value !== null &&
    typeof (value as { defaultReady?: unknown }).defaultReady === "function"
  );
}

function hasMathJaxAdaptor(value: unknown): value is {
  adaptor: {
    node: (
      kind: string,
      attributes?: Record<string, unknown>,
      ...rest: unknown[]
    ) => unknown;
    append: (parent: unknown, child: unknown) => unknown;
  };
} {
  if (
    (typeof value !== "object" && typeof value !== "function") ||
    value === null
  )
    return false;
  const candidate = value as {
    adaptor?: { append?: unknown; node?: unknown };
  };
  return (
    typeof candidate.adaptor?.node === "function" &&
    typeof candidate.adaptor.append === "function"
  );
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
