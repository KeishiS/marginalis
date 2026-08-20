import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { MathMacro } from "./api";
import {
  applyMathMacros,
  enqueueMathJaxTypeset,
  loadMathJax,
} from "./mathTypesetting";
import {
  enhanceSourceBlocks,
  prepareMath,
  wrapTables,
} from "./renderedContentEnhancement";

const NO_MATH_MACROS: MathMacro[] = [];

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
  const [failedRender, setFailedRender] = useState<string | null>(null);
  const macroSignature = JSON.stringify(mathMacros);
  const renderKey = `${html}\0${macroSignature}`;

  useLayoutEffect(() => {
    const element = container.current;
    // MathJaxとコード表示処理が子要素を変更するため、HTMLが変わった時だけ置き換える。
    if (element) element.innerHTML = html;
  }, [html, macroSignature]);

  useEffect(() => {
    const element = container.current;
    if (!element || !active) return;
    enhanceSourceBlocks(element);
    wrapTables(element);
    if (!prepareMath(element)) return;
    applyMathMacros(element, mathMacros);
    if (failedRender === renderKey) {
      element.dataset.mathStatus = "failed";
      return;
    }

    let current = true;
    // 組版中に表示対象が変わっても古い処理が現在のDOMを変更しないよう、複製上で処理する。
    const staging = element.cloneNode(true) as HTMLElement;
    void loadMathJax(styleNonce)
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
          setFailedRender(renderKey);
        }
      });
    return () => {
      current = false;
    };
  }, [
    active,
    failedRender,
    html,
    macroSignature,
    mathMacros,
    renderKey,
    styleNonce,
  ]);

  return (
    <>
      {failedRender === renderKey && (
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
