import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { RenderedContent } from "../src/RenderedContent";
import {
  enhanceSourceBlocks,
  prepareMath,
} from "../src/renderedContentEnhancement";

afterEach(() => {
  cleanup();
  delete window.MathJax;
  vi.restoreAllMocks();
});

test("公開属性で指定されたLaTeX数式をMathJaxの入力へ変換する", () => {
  const container = document.createElement("div");
  container.innerHTML =
    '<p>inline <code class="math-latex" data-math-language="latexmath" data-math-display="inline">x^2</code></p>' +
    '<pre class="math-latex" data-math-language="latexmath" data-math-display="block"><code>x^2 + y^2</code></pre>';

  expect(prepareMath(container)).toBe(true);
  expect(container.querySelector(".math-inline")).toHaveTextContent(
    String.raw`\(x^2\)`,
  );
  expect(container.querySelector(".math-display")).toHaveTextContent(
    String.raw`\[x^2 + y^2\]`,
  );
  expect(container.querySelector(".math-inline")).toHaveAttribute(
    "data-math-display",
    "inline",
  );
  expect(container.querySelector(".math-display")).toHaveAttribute(
    "data-math-display",
    "block",
  );
  expect(container.querySelector("pre.math-latex")).not.toBeInTheDocument();
  const prepared = container.textContent;
  expect(prepareMath(container)).toBe(false);
  expect(container.textContent).toBe(prepared);
});

test("公開属性で指定された開始行からコードの各行へ番号を付ける", () => {
  const container = document.createElement("div");
  container.innerHTML =
    '<pre data-line-numbers="true" data-line-start="7"><code>first\nsecond\n</code></pre>';
  const code = container.querySelector("code");
  const source = code?.textContent;

  enhanceSourceBlocks(container);

  const rows = container.querySelectorAll(".source-line");
  expect(rows).toHaveLength(2);
  expect(rows[0]).toHaveAttribute("data-line-number", "7");
  expect(rows[0]).toHaveTextContent("first");
  expect(rows[1]).toHaveAttribute("data-line-number", "8");
  expect(rows[1]).toHaveTextContent("second");
  expect(code?.textContent).toBe(source);

  enhanceSourceBlocks(container);
  expect(container.querySelectorAll(".source-line")).toHaveLength(2);
});

test("不正な開始行から行番号を推測しない", () => {
  const container = document.createElement("div");
  container.innerHTML =
    '<pre data-line-numbers="true" data-line-start="0"><code>first</code></pre>';

  enhanceSourceBlocks(container);

  expect(container.querySelector(".source-line")).not.toBeInTheDocument();
});

test("属性がない要素や未対応の数式言語を推測しない", () => {
  const container = document.createElement("div");
  container.innerHTML =
    '<code class="math-latex">x</code>' +
    '<code data-math-language="asciimath" data-math-display="inline">y</code>';

  expect(prepareMath(container)).toBe(false);
  expect(container.querySelectorAll("code")).toHaveLength(2);
});

test("対応するAsciiDoc表示要素を一つのfixtureで固定する", () => {
  const container = document.createElement("div");
  container.innerHTML = `
    <nav id="toc"><ul><li><a href="#section">目次</a></li></ul></nav>
    <h2 id="section">見出し</h2>
    <blockquote><p>引用</p></blockquote>
    <ul><li>箇条書き</li></ul>
    <table><caption>比較表</caption><tbody><tr><td>値</td></tr></tbody></table>
    <figure class="source-block"><figcaption>例</figcaption><pre data-language="rust" data-line-numbers="true" data-line-start="7"><code class="language-rust">fn main() {}</code></pre></figure>
    <p><code class="math-latex" data-math-language="latexmath" data-math-display="inline">x^2</code></p>
    <pre class="math-latex" data-math-language="latexmath" data-math-display="block"><code>x^2 + y^2</code></pre>
  `;

  enhanceSourceBlocks(container);
  prepareMath(container);

  expect(container.querySelector("#toc a")).toHaveAttribute("href", "#section");
  expect(container.querySelector("blockquote")).toHaveTextContent("引用");
  expect(container.querySelectorAll("ul")[1]).toHaveTextContent("箇条書き");
  expect(container.querySelector("table")).toHaveTextContent("比較表");
  expect(container.querySelector("pre[data-language='rust']")).toBeTruthy();
  expect(container.querySelector(".source-line")).toHaveAttribute(
    "data-line-number",
    "7",
  );
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
        '<pre data-language="rust"><code class="language-rust">fn main() {}</code></pre>' +
        '<code class="math-latex" data-math-language="latexmath" data-math-display="inline">x^2</code>'
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
  await waitFor(() => {
    expect(document.querySelector(".preview-content pre")).toHaveAttribute(
      "data-language",
      "rust",
    );
  });
  expect(document.querySelector(".math-inline")).toHaveTextContent(
    String.raw`\(x^2\)`,
  );
  expect(consoleError).toHaveBeenCalledWith(
    "MathJaxによる数式の組版に失敗しました。",
    expect.any(Error),
  );
});
