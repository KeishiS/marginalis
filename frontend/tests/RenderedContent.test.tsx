import { expect, test } from "vitest";

import {
  enhanceCodeBlocks,
  prepareMath,
} from "../src/renderedContentEnhancement";

test("コードブロックへ言語名を付ける", () => {
  const container = document.createElement("div");
  container.innerHTML =
    '<pre><code class="language-rust">fn main() {}</code></pre>';

  enhanceCodeBlocks(container);

  expect(container.querySelector("pre")).toHaveAttribute(
    "data-language",
    "rust",
  );
});

test("LaTeX数式をMathJaxの入力へ変換する", () => {
  const container = document.createElement("div");
  container.innerHTML =
    '<p>inline <code class="math-latex">x^2</code></p>' +
    '<pre class="math-latex"><code>x^2 + y^2</code></pre>';

  expect(prepareMath(container)).toBe(true);
  expect(container.querySelector(".math-inline")).toHaveTextContent(
    String.raw`\(x^2\)`,
  );
  expect(container.querySelector(".math-display")).toHaveTextContent(
    String.raw`\[x^2 + y^2\]`,
  );
  expect(container.querySelector("pre.math-latex")).not.toBeInTheDocument();
});
