const { test, expect } = require("@playwright/test");

test("production build starts and renders a note returned by the API", async ({
  page,
}) => {
  const browserErrors = [];
  page.on("pageerror", (error) => browserErrors.push(error.message));
  await page.route("**/api/v3/notes", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify([
        {
          note_id: "0197c9bc-0000-7000-8000-000000000001",
          title: "ブラウザー基本試験",
          tags: ["smoke"],
          updated_at_ms: Date.parse("2026-07-28T12:00:00Z"),
          revision: 1,
          access: "manage",
        },
      ]),
    });
  });

  await page.goto("/");
  await page.waitForLoadState("networkidle");
  await page.evaluate(() => document.fonts.ready);
  expect(browserErrors).toEqual([]);

  await expect(
    page.getByRole("heading", { name: "ノート", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "ブラウザー基本試験" }),
  ).toHaveAttribute("href", "/notes/0197c9bc-0000-7000-8000-000000000001");
  await expect(page.getByRole("status")).toContainText("1件のノート");
  await expect(page.getByRole("link", { name: "新規ノート" })).toHaveCount(1);
  await expect(page).toHaveScreenshot("note-list-wide.png", SCREENSHOT_OPTIONS);
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page).toHaveScreenshot(
    "note-list-wide-dark.png",
    SCREENSHOT_OPTIONS,
  );
  await page.emulateMedia({ colorScheme: "light" });
  await page.setViewportSize({ width: 360, height: 720 });
  await expect(page).toHaveScreenshot(
    "note-list-narrow.png",
    SCREENSHOT_OPTIONS,
  );
});

test("閲覧画面でnote IDをコピーし、広い本文を表示する", async ({
  page,
  context,
}) => {
  const noteId = "0197c9bc-0000-7000-8000-000000000001";
  await context.grantPermissions(["clipboard-read", "clipboard-write"], {
    origin: "http://127.0.0.1:42877",
  });
  await page.route(`**/api/v3/notes/${noteId}/view`, async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        note: {
          note_id: noteId,
          title: "広い閲覧画面",
          source: "= 広い閲覧画面\n\n本文",
          tags: ["design"],
          created_at_ms: 1,
          updated_at_ms: 1,
          revision: 1,
        },
        access: "manage",
        html:
          "<article><h1>広い閲覧画面</h1><p>長い文章と表、コード、数式を読みやすい幅で表示します。</p>" +
          "<table><tr><th>項目</th><th>説明</th><th>確認</th></tr><tr><td>本文幅</td><td>広い画面を活用します</td><td>成功</td></tr></table>" +
          '<pre data-language="rust"><code>fn main() { println!("wide"); }</code></pre></article>',
        related: { outgoing: [], incoming: [] },
      }),
    });
  });

  await page.goto(`/notes/${noteId}`);
  await page.waitForLoadState("networkidle");
  await page.evaluate(() => document.fonts.ready);
  const documentSurface = page.locator(".document-surface");
  await expect(documentSurface).toBeVisible();
  const documentPosition = await documentSurface.boundingBox();
  expect(documentPosition).not.toBeNull();
  expect(documentPosition.width).toBeGreaterThan(1000);

  await page.getByRole("button", { name: "note IDをコピー" }).click();
  await expect(page.getByRole("status")).toHaveText(
    "note IDをコピーしました。",
  );
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    noteId,
  );
  await expect(page).toHaveScreenshot("note-view-wide.png", SCREENSHOT_OPTIONS);

  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page).toHaveScreenshot(
    "note-view-wide-dark.png",
    SCREENSHOT_OPTIONS,
  );
  await page.emulateMedia({ colorScheme: "light" });
  await page.setViewportSize({ width: 360, height: 720 });
  await expect(documentSurface).toBeVisible();
  const narrowPosition = await documentSurface.boundingBox();
  expect(narrowPosition).not.toBeNull();
  expect(narrowPosition.width).toBeLessThanOrEqual(336);
  await expect(page).toHaveScreenshot(
    "note-view-narrow.png",
    SCREENSHOT_OPTIONS,
  );
});

test("CodeMirrorで行番号、表示切替、日本語入力状態を扱う", async ({ page }) => {
  await page.route("**/api/v3/notes/preview", async (route) => {
    const source = (await route.request().postDataJSON()).source;
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: `<div class="preview-content"><p>${escapeHtml(source)}</p></div>`,
        diagnostics: [],
      }),
    });
  });
  await page.goto("/notes/new");
  const editor = page.getByRole("textbox", { name: "AsciiDoc文書" });
  await expect(editor).toBeFocused();
  await editor.fill("= 編集画面\n\n1行目\n2行目");
  const firstLine = page.locator(".cm-line").first();
  const firstLineNumber = page
    .locator(".cm-lineNumbers .cm-gutterElement")
    .nth(1);
  await expect(firstLineNumber).toHaveText("1");
  const linePosition = await firstLine.boundingBox();
  const lineNumberPosition = await firstLineNumber.boundingBox();
  expect(linePosition).not.toBeNull();
  expect(lineNumberPosition).not.toBeNull();
  expect(lineNumberPosition.x + lineNumberPosition.width).toBeLessThanOrEqual(
    linePosition.x,
  );
  await expect(page.getByRole("toolbar", { name: "入力補助" })).toHaveCount(0);

  await editor.evaluate((element) =>
    element.dispatchEvent(
      new CompositionEvent("compositionstart", {
        bubbles: true,
        data: "編集中",
      }),
    ),
  );
  await expect(page.getByText("日本語入力を確定してください。")).toBeVisible();
  await editor.evaluate((element) =>
    element.dispatchEvent(
      new CompositionEvent("compositionend", {
        bubbles: true,
        data: "確定",
      }),
    ),
  );

  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(page).toHaveScreenshot(
    "editor-wide-split.png",
    SCREENSHOT_OPTIONS,
  );
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page).toHaveScreenshot(
    "editor-wide-split-dark.png",
    SCREENSHOT_OPTIONS,
  );
  await page.emulateMedia({ colorScheme: "light" });
  await page.setViewportSize({ width: 320, height: 720 });
  await expect(page.locator(".editor-workspace")).toHaveAttribute(
    "data-view-mode",
    "write",
  );
  await expect(page.getByRole("button", { name: "分割" })).toBeDisabled();
  await expect(page).toHaveScreenshot(
    "editor-narrow-write.png",
    SCREENSHOT_OPTIONS,
  );
});

test("数式を組版したまま分割表示とプレビュー表示を切り替える", async ({
  page,
}) => {
  const fontRequests = [];
  page.on("request", (request) => {
    if (request.url().includes("mathjax-newcm-font")) {
      fontRequests.push(request.url());
    }
  });
  await page.route("**/api/v3/notes/preview", async (route) => {
    const source = (await route.request().postDataJSON()).source;
    const html = source.includes(String.raw`stem:[\lambda]`)
      ? String.raw`<p>インライン数式 <code class="math-latex" data-math-language="latexmath" data-math-display="inline">\lambda \in \mathbb{R}</code>のチェックです。</p>` +
        (source.includes("プレビューから分割への確認")
          ? "<p>プレビューから分割への確認</p>"
          : "")
      : "<p>プレビュー</p>";
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({ html, diagnostics: [] }),
    });
  });
  await page.goto("/notes/new");
  await page.getByRole("button", { name: "執筆" }).click();
  await page.getByRole("textbox", { name: "AsciiDoc文書" }).fill(
    String.raw`= 新規ノート
:tags:
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
:tags:
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
});

test("閲覧画面の遅延字体を同一オリジンから読み込む", async ({ page }) => {
  const noteId = "0197c9bc-0000-7000-8000-000000000002";
  const fontResponses = [];
  page.on("response", (response) => {
    if (response.url().includes("mathjax-newcm-font")) {
      fontResponses.push({ url: response.url(), status: response.status() });
    }
  });
  await page.route(`**/api/v3/notes/${noteId}/view`, async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        note: {
          note_id: noteId,
          title: "数式の閲覧",
          source: String.raw`= 数式の閲覧

stem:[\mathbb{R}]`,
          tags: [],
          created_at_ms: 1,
          updated_at_ms: 1,
          revision: 1,
        },
        access: "manage",
        html: String.raw`<p><code class="math-latex" data-math-language="latexmath" data-math-display="inline">\mathbb{R}</code></p>`,
        related: { outgoing: [], incoming: [] },
      }),
    });
  });

  await page.goto(`/notes/${noteId}`);
  await expect(page.locator(".rendered-content mjx-container")).toBeVisible();
  expect(fontResponses).toContainEqual({
    url: "http://127.0.0.1:42877/assets/mathjax-fonts/mathjax-newcm-font/svg/dynamic/double-struck.js",
    status: 200,
  });
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("5,000行の文書を編集して保存できる", async ({ page }) => {
  test.setTimeout(30_000);
  const source = [
    "= 長文試験",
    "",
    ...Array.from(
      { length: 4998 },
      (_, index) => `${index + 3}行目の日本語とemoji😀`,
    ),
  ].join("\n");
  expect(Buffer.byteLength(source, "utf8")).toBeLessThanOrEqual(512 * 1024);

  await page.route("**/api/v3/notes/preview", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: '<div class="preview-content"><p>長文プレビュー</p></div>',
        diagnostics: [],
      }),
    });
  });
  await page.route("**/api/v3/notes", async (route) => {
    if (route.request().method() !== "POST") {
      await route.fallback();
      return;
    }
    const input = await route.request().postDataJSON();
    await route.fulfill({
      status: 201,
      contentType: "application/json",
      body: JSON.stringify({
        note_id: "0197c9bc-0000-7000-8000-000000000099",
        title: "長文試験",
        source: input.source,
        tags: [],
        created_at_ms: 1,
        updated_at_ms: 1,
        revision: 1,
      }),
    });
  });
  await page.goto("/notes/new");
  const editor = page.getByRole("textbox", { name: "AsciiDoc文書" });
  await editor.focus();
  await editor.press("Control+a");
  const insertionTime = await editor.evaluate((element, input) => {
    const clipboardData = new DataTransfer();
    clipboardData.setData("text/plain", input);
    const started = performance.now();
    element.dispatchEvent(
      new ClipboardEvent("paste", {
        bubbles: true,
        clipboardData,
      }),
    );
    return performance.now() - started;
  }, source);
  expect(insertionTime).toBeLessThan(5_000);
  await expect(page.getByText("未保存の変更があります。")).toBeVisible();
  await page.getByRole("button", { name: "プレビュー" }).click();
  await page.getByRole("button", { name: "執筆" }).click();
  await expect(editor).toBeFocused();
  await editor.press("Control+s");
  const toast = page.locator(".toast");
  await expect(toast.getByText("保存しました。")).toBeVisible();
  const toastPosition = await toast.boundingBox();
  const viewport = page.viewportSize();
  expect(toastPosition).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(toastPosition.y).toBeLessThan(130);
  expect(viewport.width - (toastPosition.x + toastPosition.width)).toBeLessThan(
    40,
  );
});

function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

const SCREENSHOT_OPTIONS = {
  animations: "disabled",
  // Linux環境ごとのフォント描画差を許容し、配置の大きな崩れは検出します。
  maxDiffPixelRatio: 0.03,
};
