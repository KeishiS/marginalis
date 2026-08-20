// MathJaxの読み込み、CSP nonceの適用、利用者マクロの局所定義、組版の直列化を担う共有module。
// 閲覧・プレビュー(RenderedContent)と編集欄のLive Preview widgetが同じランタイムを使う。

import boldsymbolUrl from "mathjax/input/tex/extensions/boldsymbol.js?url&no-inline";
import mathtoolsUrl from "mathjax/input/tex/extensions/mathtools.js?url&no-inline";
import mathJaxUrl from "mathjax/tex-svg.js?url";

import { MathMacro } from "./api";
import { validMathMacroForRendering } from "./mathMacroState";

export interface MathJaxRuntime {
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

/** 組版を1件ずつ直列に実行する。MathJaxのtypesetは並行実行を想定していない。 */
export function enqueueMathJaxTypeset<T>(task: () => Promise<T>): Promise<T> {
  const result = mathJaxTypesetQueue.then(task);
  mathJaxTypesetQueue = result.then(
    () => undefined,
    () => undefined,
  );
  return result;
}

export async function loadMathJax(styleNonce: string): Promise<MathJaxRuntime> {
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
        macros: {},
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

/**
 * 所有者ごとのマクロを各数式の局所定義として加える。
 *
 * MathJax本体は画面全体で再利用するため、起動時の設定へ利用者別マクロを残さない。波括弧で
 * 定義範囲を数式内へ閉じ、別のノートやプレビューへ漏れないようにする。
 */
export function applyMathMacros(element: HTMLElement, macros: MathMacro[]) {
  const applicable = macros.filter(validMathMacroForRendering);
  if (applicable.length !== macros.length) {
    console.error(
      "安全に適用できない数式マクロを除外しました。数式マクロ設定を確認してください。",
    );
  }
  if (applicable.length === 0) return;
  const definitions = applicable
    .map((macro) => {
      const argumentsDeclaration = Array.from(
        { length: macro.argument_count },
        (_, index) => `#${index + 1}`,
      ).join("");
      return `\\def\\${macro.name}${argumentsDeclaration}{${macro.replacement}}`;
    })
    .join("");
  for (const formula of element.querySelectorAll<HTMLElement>(
    ".math-inline, .math-display",
  )) {
    const value = formula.textContent ?? "";
    const opening = value.slice(0, 2);
    const closing = value.slice(-2);
    formula.textContent = `${opening}{${definitions}${value.slice(2, -2)}}${closing}`;
  }
}

const FORMULA_CACHE_LIMIT = 200;
const formulaCache = new Map<string, Node>();

/**
 * 数式1件を組版し、結果のDOM node(複製)を返す。
 *
 * Live Previewは打鍵と選択のたびにwidgetを作り直すため、数式の原文とマクロの組ごとに
 * 結果を控え、同じ数式の再組版を避ける。控えの上限を超えたら古いものから捨てる。
 */
export async function typesetFormula(
  latex: string,
  display: boolean,
  macros: MathMacro[],
  styleNonce: string,
): Promise<Node> {
  const key = `${display ? "block" : "inline"}\0${JSON.stringify(macros)}\0${latex}`;
  const cached = formulaCache.get(key);
  if (cached) {
    // 使ったものを末尾へ移し、挿入順を「最近使った順」として扱う。
    formulaCache.delete(key);
    formulaCache.set(key, cached);
    return cached.cloneNode(true);
  }
  const staging = document.createElement(display ? "div" : "span");
  staging.className = display
    ? "math-latex math-display"
    : "math-latex math-inline";
  staging.textContent = display ? `\\[${latex}\\]` : `\\(${latex}\\)`;
  applyMathMacros(staging, macros);
  const mathJax = await loadMathJax(styleNonce);
  const rendered = await enqueueMathJaxTypeset(async () => {
    let cleared = false;
    try {
      await mathJax.typesetPromise([staging]);
      const renderedNodes = Array.from(staging.childNodes);
      mathJax.typesetClear?.([staging]);
      cleared = true;
      const result = document.createDocumentFragment();
      result.append(...renderedNodes);
      return result;
    } finally {
      if (!cleared) mathJax.typesetClear?.([staging]);
    }
  });
  formulaCache.set(key, rendered.cloneNode(true));
  if (formulaCache.size > FORMULA_CACHE_LIMIT) {
    const oldest = formulaCache.keys().next().value;
    if (oldest !== undefined) formulaCache.delete(oldest);
  }
  return rendered;
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
