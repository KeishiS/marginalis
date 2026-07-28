export function enhanceCodeBlocks(container: HTMLElement) {
  for (const code of container.querySelectorAll<HTMLElement>(
    "pre > code[class^='language-']",
  )) {
    const language = [...code.classList]
      .find((name) => name.startsWith("language-"))
      ?.slice("language-".length);
    if (language && code.parentElement) {
      code.parentElement.dataset.language = language;
    }
  }
}

export function prepareMath(container: HTMLElement): boolean {
  const formulas = [
    ...container.querySelectorAll<HTMLElement>(
      "code.math-latex, pre.math-latex > code",
    ),
  ];
  for (const formula of formulas) {
    const display = formula.parentElement?.matches("pre.math-latex") ?? false;
    const replacement = document.createElement(display ? "div" : "span");
    replacement.className = display
      ? "math-latex math-display"
      : "math-latex math-inline";
    replacement.textContent = display
      ? `\\[${formula.textContent ?? ""}\\]`
      : `\\(${formula.textContent ?? ""}\\)`;
    if (display) {
      formula.parentElement?.replaceWith(replacement);
    } else {
      formula.replaceWith(replacement);
    }
  }
  return formulas.length > 0;
}
