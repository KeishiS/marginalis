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

test("コードブロックへ1から始まる行番号を付ける", () => {
  const container = document.createElement("div");
  container.innerHTML =
    '<figure class="source-block"><pre><code>first\nsecond\n</code></pre></figure>';
  const code = container.querySelector("code");
  const source = code?.textContent;

  enhanceSourceBlocks(container);

  const rows = container.querySelectorAll(".source-line");
  expect(rows).toHaveLength(2);
  expect(rows[0]).toHaveAttribute("data-line-number", "1");
  expect(rows[0]).toHaveTextContent("first");
  expect(rows[1]).toHaveAttribute("data-line-number", "2");
  expect(rows[1]).toHaveTextContent("second");
  expect(code?.textContent).toBe(source);

  enhanceSourceBlocks(container);
  expect(container.querySelectorAll(".source-line")).toHaveLength(2);
});

test("不正な開始行を無視して1から行番号を付ける", () => {
  const container = document.createElement("div");
  container.innerHTML =
    '<figure class="source-block"><pre data-line-start="0"><code>first</code></pre></figure>';

  enhanceSourceBlocks(container);

  expect(container.querySelector(".source-line")).toHaveAttribute(
    "data-line-number",
    "1",
  );
});

test("source blockではないcode要素や未対応の数式言語を推測しない", () => {
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

test("非表示中に届いた数式を再表示時に組版する", async () => {
  const typesetClear = vi.fn();
  const typesetPromise = vi.fn().mockResolvedValue(undefined);
  window.MathJax = {
    startup: { promise: Promise.resolve() },
    typesetClear,
    typesetPromise,
  };
  const html = String.raw`<p>数式 <code class="math-latex" data-math-language="latexmath" data-math-display="inline">\lambda</code></p>`;
  const { rerender } = render(
    <RenderedContent active={false} html={html} preview />,
  );

  expect(typesetPromise).not.toHaveBeenCalled();
  expect(document.querySelector(".math-latex")).toHaveTextContent(
    String.raw`\lambda`,
  );

  rerender(<RenderedContent active html={html} preview />);

  await waitFor(() => expect(typesetPromise).toHaveBeenCalledOnce());
  expect(typesetClear).toHaveBeenCalledOnce();
  expect(document.querySelector(".math-inline")).toHaveTextContent(
    String.raw`\(\lambda\)`,
  );
});

test("組版済みの数式を表示方式の切り替え後も維持する", async () => {
  const typesetPromise = vi.fn(async ([element]: HTMLElement[]) => {
    const formula = element.querySelector(".math-inline");
    const rendered = document.createElement("mjx-container");
    formula?.replaceWith(rendered);
  });
  window.MathJax = {
    startup: { promise: Promise.resolve() },
    typesetPromise,
  };
  const html = String.raw`<p>数式 <code class="math-latex" data-math-language="latexmath" data-math-display="inline">\lambda</code></p>`;
  const { rerender } = render(<RenderedContent active html={html} preview />);

  await waitFor(() =>
    expect(document.querySelector("mjx-container")).toBeInTheDocument(),
  );

  rerender(<RenderedContent active html={html} />);

  expect(document.querySelector("mjx-container")).toBeInTheDocument();
  expect(document.querySelector(".math-latex")).not.toBeInTheDocument();
  expect(typesetPromise).toHaveBeenCalledOnce();
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
