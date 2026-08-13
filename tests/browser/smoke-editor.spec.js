const { expect, test } = require("./fixtures/browser-diagnostics");
const {
  pendingWebProvenance,
  escapeHtml,
  SCREENSHOT_OPTIONS,
  editorScreenshotOptions,
  detailScreenshotOptions,
} = require("./fixtures/smoke-helpers");

test("CodeMirrorで行番号、表示切替、日本語入力状態を扱う", async ({ page }) => {
  await page.route("**/api/v3/notes/preview", async (route) => {
    const source = (await route.request().postDataJSON()).source;
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: `<div class="preview-content"><p>${escapeHtml(source)}</p></div>`,
        math_macros: [],
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
    editorScreenshotOptions(page),
  );
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page).toHaveScreenshot(
    "editor-wide-split-dark.png",
    editorScreenshotOptions(page),
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
    editorScreenshotOptions(page),
  );
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
        math_macros: [],
        diagnostics: [],
      }),
    });
  });
  await page.route("**/api/v3/web/notes", async (route) => {
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
        ...pendingWebProvenance,
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
  const toast = page.locator('[data-slot="toast"]');
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
