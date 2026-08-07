/**
 * 表を横スクロールできる枠で包む。
 *
 * 表だけでは、列の幅を保ったまま器の幅で頭打ちにできない。`min-width`は`max-width`より優先される
 * ため、内容の幅を保とうとすると本文ごと横へはみ出す。`thead`と`tbody`を別々の表として扱うと列が
 * ずれるため、外側に枠を足してそこをスクロールさせる。
 */
export function wrapTables(container: HTMLElement) {
  for (const table of container.querySelectorAll<HTMLTableElement>("table")) {
    if (table.parentElement?.classList.contains("table-scroll")) continue;
    const scroll = document.createElement("div");
    scroll.className = "table-scroll";
    // キーボードだけでも横へスクロールできるようにする。
    scroll.tabIndex = 0;
    scroll.setAttribute("role", "region");
    scroll.setAttribute("aria-label", "表");
    table.replaceWith(scroll);
    scroll.append(table);
  }
}

export function enhanceSourceBlocks(container: HTMLElement) {
  for (const code of container.querySelectorAll<HTMLElement>(
    "figure.source-block > pre > code",
  )) {
    if (code.dataset.lineNumbersEnhanced === "true") continue;
    const startValue = code.parentElement?.dataset.lineStart;
    const requestedStart =
      startValue && /^[1-9][0-9]*$/.test(startValue) ? Number(startValue) : 1;
    const start =
      Number.isSafeInteger(requestedStart) && requestedStart <= 4_294_967_295
        ? requestedStart
        : 1;

    const source = code.textContent ?? "";
    const hasTrailingNewline = source.endsWith("\n");
    const lines = (hasTrailingNewline ? source.slice(0, -1) : source).split(
      "\n",
    );
    const fragment = document.createDocumentFragment();
    lines.forEach((line, index) => {
      const row = document.createElement("span");
      row.className = "source-line";
      row.dataset.lineNumber = String(start + index);
      row.textContent = line;
      fragment.append(row);
      if (index + 1 < lines.length || hasTrailingNewline) {
        fragment.append("\n");
      }
    });
    code.replaceChildren(fragment);
    code.dataset.lineNumbersEnhanced = "true";
  }
}

export function prepareMath(container: HTMLElement): boolean {
  const formulas = [
    ...container.querySelectorAll<HTMLElement>(
      "[data-math-language='latexmath'][data-math-display='inline']:not([data-math-prepared='true']), " +
        "[data-math-language='latexmath'][data-math-display='block']:not([data-math-prepared='true'])",
    ),
  ];
  for (const formula of formulas) {
    const display = formula.dataset.mathDisplay === "block";
    const replacement = document.createElement(display ? "div" : "span");
    replacement.className = display
      ? "math-latex math-display"
      : "math-latex math-inline";
    replacement.dataset.mathLanguage = "latexmath";
    replacement.dataset.mathDisplay = display ? "block" : "inline";
    replacement.dataset.mathPrepared = "true";
    replacement.textContent = display
      ? `\\[${formula.textContent ?? ""}\\]`
      : `\\(${formula.textContent ?? ""}\\)`;
    formula.replaceWith(replacement);
  }
  return formulas.length > 0;
}
