import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { RenderedContent } from "../src/RenderedContent";
import {
  enhanceCodeBlocks,
  prepareMath,
} from "../src/renderedContentEnhancement";

afterEach(() => {
  cleanup();
  delete window.MathJax;
  vi.restoreAllMocks();
});

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

test("対応するAsciiDoc表示要素を一つのfixtureで固定する", () => {
  const container = document.createElement("div");
  container.innerHTML = `
    <nav id="toc"><ul><li><a href="#section">目次</a></li></ul></nav>
    <h2 id="section">見出し</h2>
    <blockquote><p>引用</p></blockquote>
    <ul><li>箇条書き</li></ul>
    <table><caption>比較表</caption><tbody><tr><td>値</td></tr></tbody></table>
    <pre><code class="language-rust">fn main() {}</code></pre>
    <p><code class="math-latex">x^2</code></p>
    <pre class="math-latex"><code>x^2 + y^2</code></pre>
  `;

  enhanceCodeBlocks(container);
  prepareMath(container);

  expect(container.querySelector("#toc a")).toHaveAttribute("href", "#section");
  expect(container.querySelector("blockquote")).toHaveTextContent("引用");
  expect(container.querySelectorAll("ul")[1]).toHaveTextContent("箇条書き");
  expect(container.querySelector("table")).toHaveTextContent("比較表");
  expect(container.querySelector("pre[data-language='rust']")).toBeTruthy();
  expect(container.querySelector(".math-inline")).toBeTruthy();
  expect(container.querySelector(".math-display")).toBeTruthy();
});

test("MathJaxの組版失敗を利用者へ通知する", async () => {
  const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
  window.MathJax = {
    startup: { promise: Promise.resolve() },
    typesetPromise: vi.fn().mockRejectedValue(new Error("typeset failed")),
  };

  render(
    <RenderedContent
      html={
        '<pre><code class="language-rust">fn main() {}</code></pre>' +
        '<code class="math-latex">x^2</code>'
      }
      preview
    />,
  );

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "数式を描画できませんでした",
  );
  expect(document.querySelector(".preview-content")).toHaveAttribute(
    "data-math-status",
    "failed",
  );
  expect(document.querySelector(".preview-content pre")).toHaveAttribute(
    "data-language",
    "rust",
  );
  expect(consoleError).toHaveBeenCalledWith(
    "MathJaxによる数式の組版に失敗しました。",
    expect.any(Error),
  );
});
