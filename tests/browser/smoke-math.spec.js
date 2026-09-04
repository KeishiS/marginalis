const { expect, test } = require("./fixtures/browser-diagnostics");
const { pendingWebProvenance } = require("./fixtures/smoke-helpers");

const mathematicalBlocksHtml = String.raw`
  <div id="definition" class="open role-definition"><div class="title">定義 1（群）</div><p>集合 <code class="math-latex" data-math-language="latexmath" data-math-display="inline">G</code> を定義します。</p></div>
  <div class="open role-proposition"><div class="title">命題 2</div><p>命題です。</p></div>
  <div class="open role-lemma"><div class="title">補題 3</div><p>補題です。</p></div>
  <div id="theorem" class="open role-theorem"><div class="title">定理 4</div><p>定理です。</p></div>
  <div class="open role-corollary"><div class="title">系 5</div><p><a href="#theorem">定理 4</a>から従います。</p></div>
  <div class="open role-proof"><div class="title">証明</div><p>証明です。</p></div>
  <p class="role-theorem">open blockではない段落です。</p>`;

test("数学文書用ブロックの範囲を閲覧画面とプレビューで示す", async ({
  page,
}) => {
  const noteId = "0197c9bc-0000-7000-8000-000000000001";
  await page.route(`**/api/v3/notes/${noteId}/view`, async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        note: {
          note_id: noteId,
          title: "代数学のノート",
          source: "= 代数学のノート",
          tags: [],
          created_at_ms: 1,
          updated_at_ms: 1,
          revision: 1,
          ...pendingWebProvenance,
        },
        access: "manage",
        math_macros: [],
        html: mathematicalBlocksHtml,
        related: { outgoing: [], incoming: [] },
      }),
    });
  });

  await page.goto(`/notes/${noteId}`);
  const blocks = page.locator(".rendered-content .open[class*='role-']");
  await expect(blocks).toHaveCount(6);
  await expect(page.locator(".rendered-content mjx-container")).toBeVisible();
  await expect(
    page.locator(".rendered-content .role-corollary a[href='#theorem']"),
  ).toHaveText("定理 4");

  const lightAppearance = await page.evaluate(() => {
    const read = (selector) => {
      const style = getComputedStyle(document.querySelector(selector));
      return {
        background: style.backgroundColor,
        borderBottom: Number.parseFloat(style.borderBottomWidth),
        borderLeft: Number.parseFloat(style.borderLeftWidth),
        borderRight: Number.parseFloat(style.borderRightWidth),
        borderTop: Number.parseFloat(style.borderTopWidth),
      };
    };
    return {
      blocks: [
        ...document.querySelectorAll(".rendered-content .open[class*='role-']"),
      ].map((element) =>
        read(
          `.${[...element.classList].find((name) => name.startsWith("role-"))}`,
        ),
      ),
      paragraph: read(".rendered-content p.role-theorem"),
      proofBorderStyle: getComputedStyle(
        document.querySelector(".rendered-content .open.role-proof"),
      ).borderTopStyle,
      proofEnd: getComputedStyle(
        document.querySelector(".rendered-content .open.role-proof"),
        "::after",
      ).content,
    };
  });
  for (const block of lightAppearance.blocks) {
    expect(block.borderTop).toBeGreaterThan(0);
    expect(block.borderBottom).toBeGreaterThan(0);
    expect(block.borderLeft).toBeGreaterThan(0);
    expect(block.borderRight).toBeGreaterThan(0);
  }
  expect(lightAppearance.proofBorderStyle).toBe("dashed");
  expect(lightAppearance.proofEnd).toContain("□");
  expect(lightAppearance.paragraph.borderTop).toBe(0);

  await page.emulateMedia({ colorScheme: "dark" });
  const darkBackground = await page
    .locator(".rendered-content .open.role-definition")
    .evaluate((element) => getComputedStyle(element).backgroundColor);
  expect(darkBackground).not.toBe(lightAppearance.blocks[0].background);

  await page.route("**/api/v3/notes/preview", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: mathematicalBlocksHtml,
        math_macros: [],
        diagnostics: [],
        spans: [],
      }),
    });
  });
  await page.goto("/notes/new");
  await page.getByRole("button", { name: "執筆" }).click();
  await page
    .getByRole("textbox", { name: "AsciiDoc文書" })
    .fill("= 代数学のノート\n\n[.theorem]\n--\n本文\n--");
  await page.getByRole("button", { name: "プレビュー" }).click();
  await expect(
    page.locator(".preview-content .open[class*='role-']"),
  ).toHaveCount(6);
  await expect(page.locator(".preview-content .open.role-proof")).toBeVisible();
});

test("数式を組版したまま執筆とプレビューを切り替える", async ({ page }) => {
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
        (source.includes("プレビューからの再組版確認")
          ? "<p>プレビューからの再組版確認</p>"
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
        spans: [],
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
  await page.getByRole("button", { name: "プレビュー" }).click();

  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(page.locator(".preview-content mjx-merror")).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    page.locator(".preview-content [data-math-prepared='true']"),
  ).toHaveCount(1);
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

プレビューからの再組版確認`,
  );
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "プレビュー" }).click();
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
        spans: [],
      }),
    });
  });

  await page.goto("/notes/new");
  await page.getByRole("button", { name: "執筆" }).click();
  await page
    .getByRole("textbox", { name: "AsciiDoc文書" })
    .fill("= TeX package制限\n\nstem:[x]");
  await page.getByRole("button", { name: "プレビュー" }).click();

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
        spans: [],
      }),
    });
  });

  await page.goto("/notes/new");
  await page.getByRole("button", { name: "執筆" }).click();
  await page
    .getByRole("textbox", { name: "AsciiDoc文書" })
    .fill("= 旧マクロの確認\n\nstem:[x]");
  await page.getByRole("button", { name: "プレビュー" }).click();

  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(page.locator(".preview-content mjx-merror")).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(browserDiagnostics.diagnostics).toEqual([
    expect.objectContaining({ kind: "console.error" }),
  ]);
});
