const { expect, test } = require("./fixtures/browser-diagnostics");
const {
  pendingWebProvenance,
  escapeHtml,
  SCREENSHOT_OPTIONS,
  editorScreenshotOptions,
  detailScreenshotOptions,
} = require("./fixtures/smoke-helpers");

test("数式を組版したまま分割表示とプレビュー表示を切り替える", async ({
  page,
}) => {
  const fontRequests = [];
  const extensionResponses = [];
  page.on("request", (request) => {
    if (request.url().includes("mathjax-newcm-font")) {
      fontRequests.push(request.url());
    }
  });
  page.on("response", (response) => {
    if (
      response.url().endsWith("/assets/boldsymbol.js") ||
      response.url().endsWith("/assets/mathtools.js")
    ) {
      extensionResponses.push({
        url: response.url(),
        status: response.status(),
      });
    }
  });
  await page.route("**/api/v3/notes/preview", async (route) => {
    const source = (await route.request().postDataJSON()).source;
    const html = source.includes(String.raw`stem:[\lambda]`)
      ? String.raw`<p>インライン数式 <code class="math-latex" data-math-language="latexmath" data-math-display="inline">f(x) \coloneqq \argmax_{x \in S} f(x) + \bm{x},\quad x \in \mathbb{R}</code>のチェックです。</p>` +
        (source.includes("プレビューから分割への確認")
          ? "<p>プレビューから分割への確認</p>"
          : "")
      : "<p>プレビュー</p>";
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html,
        math_macros: [
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
        ],
        diagnostics: [],
      }),
    });
  });
  await page.goto("/notes/new");
  await page.getByRole("button", { name: "執筆" }).click();
  await page.getByRole("textbox", { name: "AsciiDoc文書" }).fill(
    String.raw`= 新規ノート
:marginalis-tags:
:sectnums:

== 見出し1

インライン数式 stem:[\lambda]のチェックです。`,
  );

  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "分割" }).click();

  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(page.locator(".preview-content mjx-merror")).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    page.locator(".preview-content [data-math-prepared='true']"),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "プレビュー" }).click();
  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(0);

  await page.getByRole("button", { name: "執筆" }).click();
  await page.getByRole("textbox", { name: "AsciiDoc文書" }).fill(
    String.raw`= 新規ノート
:marginalis-tags:
:sectnums:

== 見出し1

インライン数式 stem:[\lambda]のチェックです。

プレビューから分割への確認`,
  );
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "プレビュー" }).click();
  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await page.getByRole("button", { name: "分割" }).click();
  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(0);
  expect(fontRequests).toContain(
    "http://127.0.0.1:42877/assets/mathjax-fonts/mathjax-newcm-font/svg/dynamic/double-struck.js",
  );
  expect(
    fontRequests.every(
      (url) => new URL(url).origin === page.url().replace(/\/notes\/new$/, ""),
    ),
  ).toBe(true);
  expect(extensionResponses).toEqual(
    expect.arrayContaining([
      {
        url: "http://127.0.0.1:42877/assets/boldsymbol.js",
        status: 200,
      },
      {
        url: "http://127.0.0.1:42877/assets/mathtools.js",
        status: 200,
      },
    ]),
  );
});

test("許可していないTeX packageを数式から読み込まない", async ({ page }) => {
  const unexpectedExtensionRequests = [];
  page.on("request", (request) => {
    if (/\/(?:autoload|require|html|color)\.js(?:\?|$)/.test(request.url())) {
      unexpectedExtensionRequests.push(request.url());
    }
  });
  await page.route("**/api/v3/notes/preview", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: String.raw`<p><code class="math-latex" data-math-language="latexmath" data-math-display="inline">x + \require{html}\href{https://example.test}{y} + \color{red}{z}</code></p>`,
        math_macros: [],
        diagnostics: [],
      }),
    });
  });

  await page.goto("/notes/new");
  await page.getByRole("button", { name: "執筆" }).click();
  await page
    .getByRole("textbox", { name: "AsciiDoc文書" })
    .fill("= TeX package制限\n\nstem:[x]");
  await page.getByRole("button", { name: "分割" }).click();

  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(await page.evaluate(() => window.MathJax.config.tex.packages)).toEqual(
    [
      "base",
      "ams",
      "newcommand",
      "textmacros",
      "noundefined",
      "configmacros",
      "boldsymbol",
      "mathtools",
    ],
  );
  expect(unexpectedExtensionRequests).toEqual([]);
});

test("旧保存値の不正な未使用マクロを除外して安全なマクロを組版する", async ({
  page,
  browserDiagnostics,
}) => {
  browserDiagnostics.allow((diagnostic) => diagnostic.kind === "console.error");
  await page.route("**/api/v3/notes/preview", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: String.raw`<p><code class="math-latex" data-math-language="latexmath" data-math-display="inline">\safe{x}</code></p>`,
        math_macros: [
          { name: "safe", replacement: "#1", argument_count: 1 },
          { name: "unused", replacement: "{broken", argument_count: 0 },
          { name: "comment", replacement: "x%broken", argument_count: 0 },
          { name: "def", replacement: "unused", argument_count: 0 },
        ],
        diagnostics: [],
      }),
    });
  });

  await page.goto("/notes/new");
  await page.getByRole("button", { name: "執筆" }).click();
  await page
    .getByRole("textbox", { name: "AsciiDoc文書" })
    .fill("= 旧マクロの確認\n\nstem:[x]");
  await page.getByRole("button", { name: "分割" }).click();

  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(page.locator(".preview-content mjx-merror")).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(browserDiagnostics.diagnostics).toEqual([
    expect.objectContaining({ kind: "console.error" }),
  ]);
});
