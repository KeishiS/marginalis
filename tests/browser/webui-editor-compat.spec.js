const { expect, test } = require("./fixtures/browser-diagnostics");

test.beforeEach(async ({ page }) => {
  await page.route("**/api/v3/notes/preview", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: "<p>プレビュー</p>",
        math_macros: [],
        diagnostics: [],
        spans: [],
      }),
    });
  });
  await page.goto("/notes/new");
});

test("編集欄の等幅書体と複数行選択を保つ", async ({ page }) => {
  const editor = page.getByRole("textbox", { name: "AsciiDoc文書" });
  await editor.click();
  await expect(editor).toContainText("見出し1");

  const editorFont = await editor.evaluate(
    (element) => getComputedStyle(element).fontFamily,
  );
  const gutterFont = await page
    .locator(".cm-lineNumbers .cm-gutterElement")
    .nth(1)
    .evaluate((element) => getComputedStyle(element).fontFamily);
  expect(editorFont).toContain("Noto Sans Mono Variable");
  expect(gutterFont).toBe(editorFont);

  await selectWithKeyboard(editor);
  await expectCompleteSelection(page);

  await editor.press("ArrowRight");
  const lines = page.locator(".cm-line");
  const first = await lines.first().boundingBox();
  const last = await lines.last().boundingBox();
  expect(first).not.toBeNull();
  expect(last).not.toBeNull();
  await page.mouse.move(first.x + 18, first.y + first.height / 2);
  await page.mouse.down();
  await page.mouse.move(last.x + 36, last.y + last.height / 2, { steps: 8 });
  await page.mouse.up();
  await expectCompleteSelection(page);

  await page.emulateMedia({ colorScheme: "dark" });
  await expectCompleteSelection(page);
});

async function selectWithKeyboard(editor) {
  await editor.press("Control+Home");
  await editor.press("ArrowRight");
  await editor.press("ArrowRight");
  await editor.press("Shift+ArrowDown");
  await editor.press("Shift+ArrowDown");
  await editor.press("Shift+ArrowRight");
  await editor.press("Shift+ArrowRight");
}

async function expectCompleteSelection(page) {
  const selection = page.locator(".cm-selectionBackground");
  await expect
    .poll(() =>
      selection.evaluateAll((elements) => {
        const lineHeight = document
          .querySelector(".cm-line")
          .getBoundingClientRect().height;
        const visible = elements
          .map((element) => {
            const box = element.getBoundingClientRect();
            return {
              background: getComputedStyle(element).backgroundColor,
              bottom: box.bottom,
              top: box.top,
              width: box.width,
            };
          })
          .filter(({ width }) => width > 0);
        const top = Math.min(...visible.map((box) => box.top));
        const bottom = Math.max(...visible.map((box) => box.bottom));
        const final = visible
          .sort((left, right) => left.top - right.top)
          .at(-1);
        return (
          visible.length > 0 &&
          visible.every(
            ({ background }) => background !== "rgba(0, 0, 0, 0)",
          ) &&
          bottom - top >= lineHeight * 2.5 &&
          final.width > 0
        );
      }),
    )
    .toBe(true);
}
