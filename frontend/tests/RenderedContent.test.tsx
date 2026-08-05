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

test("許可したTeX packageだけを同一オリジンから読み込む", async () => {
  render(
    <RenderedContent
      html={String.raw`<code class="math-latex" data-math-language="latexmath" data-math-display="inline">a \coloneqq b</code>`}
      mathMacros={[
        {
          name: "argmax",
          replacement: String.raw`\operatorname*{arg\,max}`,
          argument_count: 0,
        },
        {
          name: "bm",
          replacement: String.raw`\boldsymbol{#1}`,
          argument_count: 1,
        },
      ]}
      preview
      styleNonce="test-nonce"
    />,
  );

  await waitFor(() =>
    expect(window.MathJax).toMatchObject({
      loader: {
        load: ["[tex]/boldsymbol", "[tex]/mathtools"],
        source: {
          "[tex]/boldsymbol": expect.stringContaining("boldsymbol.js"),
          "[tex]/mathtools": expect.stringContaining("mathtools.js"),
        },
      },
      tex: {
        maxMacros: 1000,
        packages: [
          "base",
          "ams",
          "newcommand",
          "textmacros",
          "noundefined",
          "configmacros",
          "boldsymbol",
          "mathtools",
        ],
        macros: {
          argmax: String.raw`\operatorname*{arg\,max}`,
          bm: [String.raw`\boldsymbol{#1}`, 1],
        },
      },
    }),
  );
  const loader = window.MathJax as {
    loader: { source: Record<string, string> };
  };
  expect(new URL(loader.loader.source["[tex]/boldsymbol"]).origin).toBe(
    window.location.origin,
  );
  expect(new URL(loader.loader.source["[tex]/mathtools"]).origin).toBe(
    window.location.origin,
  );
  const configuredPackages = (window.MathJax as { tex: { packages: string[] } })
    .tex.packages;
  expect(configuredPackages).not.toContain("autoload");
  expect(configuredPackages).not.toContain("require");
  document.querySelector("script[src$='/tex-svg.js']")?.remove();
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
    <RenderedContent
      active={false}
      html={html}
      preview
      styleNonce="test-nonce"
    />,
  );

  expect(typesetPromise).not.toHaveBeenCalled();
  expect(document.querySelector(".math-latex")).toHaveTextContent(
    String.raw`\lambda`,
  );

  rerender(
    <RenderedContent active html={html} preview styleNonce="test-nonce" />,
  );

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
  const { rerender } = render(
    <RenderedContent active html={html} preview styleNonce="test-nonce" />,
  );

  await waitFor(() =>
    expect(document.querySelector("mjx-container")).toBeInTheDocument(),
  );

  rerender(<RenderedContent active html={html} styleNonce="test-nonce" />);

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
      styleNonce="test-nonce"
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

test("古い組版の完了結果を新しいHTMLへ混ぜない", async () => {
  const completions: Array<() => void> = [];
  const typesetClear = vi.fn();
  const typesetPromise = vi.fn(async ([element]: HTMLElement[]) => {
    const formula = element.querySelector(".math-inline");
    const value = formula?.textContent ?? "";
    await new Promise<void>((resolve) => completions.push(resolve));
    const rendered = document.createElement("mjx-container");
    rendered.textContent = value;
    formula?.replaceWith(rendered);
  });
  window.MathJax = {
    startup: { promise: Promise.resolve() },
    typesetClear,
    typesetPromise,
  };
  const first = String.raw`<code class="math-latex" data-math-language="latexmath" data-math-display="inline">alpha</code>`;
  const second = String.raw`<code class="math-latex" data-math-language="latexmath" data-math-display="inline">beta</code>`;
  const { rerender } = render(
    <RenderedContent html={first} preview styleNonce="test-nonce" />,
  );
  await waitFor(() => expect(typesetPromise).toHaveBeenCalledTimes(1));

  rerender(<RenderedContent html={second} preview styleNonce="test-nonce" />);
  completions.shift()?.();
  await waitFor(() => expect(typesetPromise).toHaveBeenCalledTimes(2));
  completions.shift()?.();

  await waitFor(() =>
    expect(document.querySelector("mjx-container")).toHaveTextContent("beta"),
  );
  expect(document.body).not.toHaveTextContent("alpha");
  expect(typesetClear).toHaveBeenCalledTimes(2);
  expect(typesetClear.mock.calls[0][0][0]).not.toBe(
    typesetClear.mock.calls[1][0][0],
  );
});
