export function enhanceSourceBlocks(container: HTMLElement) {
  for (const code of container.querySelectorAll<HTMLElement>(
    "pre[data-line-numbers='true'][data-line-start] > code",
  )) {
    if (code.dataset.lineNumbersEnhanced === "true") continue;
    const startValue = code.parentElement?.dataset.lineStart;
    if (!startValue || !/^[1-9][0-9]*$/.test(startValue)) continue;
    const start = Number(startValue);
    if (!Number.isSafeInteger(start) || start > 4_294_967_295) continue;

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
