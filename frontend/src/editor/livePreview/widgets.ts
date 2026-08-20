import { EditorView, WidgetType } from "@codemirror/view";

import { MathMacro } from "../../api";
import { typesetFormula } from "../../mathTypesetting";

/**
 * 数式1件をMathJaxのSVGとして表示するwidget。
 *
 * 生成直後は数式の原文を淡色で示し、組版が済んだら置き換える。組版は非同期で、結果は
 * `typesetFormula`が数式原文とマクロの組ごとに控えるため、同じ数式で再組版しない。
 */
export class MathWidget extends WidgetType {
  constructor(
    private readonly latex: string,
    private readonly display: boolean,
    private readonly macros: MathMacro[],
    private readonly macroSignature: string,
    private readonly styleNonce: string,
  ) {
    super();
  }

  override eq(other: MathWidget): boolean {
    return (
      other.latex === this.latex &&
      other.display === this.display &&
      other.macroSignature === this.macroSignature
    );
  }

  override toDOM(view: EditorView): HTMLElement {
    const element = document.createElement(this.display ? "div" : "span");
    element.className = this.display
      ? "lp-math lp-math-block"
      : "lp-math lp-math-inline";
    element.textContent = this.latex;
    element.dataset.mathStatus = "pending";
    typesetFormula(this.latex, this.display, this.macros, this.styleNonce)
      .then((rendered) => {
        element.replaceChildren(rendered);
        element.dataset.mathStatus = "typeset";
        // widgetの高さが変わるため、表示の測り直しを求める。
        view.requestMeasure();
      })
      .catch((error: unknown) => {
        element.dataset.mathStatus = "failed";
        console.error("MathJaxによる数式の組版に失敗しました。", error);
      });
    return element;
  }

  override ignoreEvent(): boolean {
    // クリックを編集側へ通し、カーソル移動によるソース開示を働かせる。
    return false;
  }
}
