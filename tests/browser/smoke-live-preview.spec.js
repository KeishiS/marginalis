const { expect, test } = require("./fixtures/browser-diagnostics");

// Live Preview(ADR 0016)のカーソル開示をDOMで検証する。span注釈は本来サーバーの
// 解析結果だが、smoke環境ではこの文書に対する値を固定で返す。バイト位置はUTF-8で数える。
const DOCUMENT = "= 題名\n\n**太字**の本文";
const SPANS = [
  {
    kind: "document_title",
    span: { start: 0, end: 10, unit: "utf8_byte" },
    content_span: { start: 2, end: 8, unit: "utf8_byte" },
    marker_spans: [
      { start: 0, end: 1, unit: "utf8_byte" },
      { start: 1, end: 2, unit: "utf8_byte" },
    ],
  },
  {
    kind: "strong",
    span: { start: 10, end: 20, unit: "utf8_byte" },
    content_span: { start: 12, end: 18, unit: "utf8_byte" },
    marker_spans: [
      { start: 10, end: 12, unit: "utf8_byte" },
      { start: 18, end: 20, unit: "utf8_byte" },
    ],
  },
];

test("執筆中の装飾とカーソルによる記法の開示を切り替える", async ({ page }) => {
  await page.route("**/api/v3/notes/preview", async (route) => {
    const source = (await route.request().postDataJSON()).source;
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: "<p>プレビュー</p>",
        math_macros: [],
        diagnostics: [],
        spans: source === DOCUMENT ? SPANS : [],
      }),
    });
  });
  await page.goto("/notes/new");
  const editor = page.getByRole("textbox", { name: "AsciiDoc文書" });
  await expect(editor).toBeFocused();
  await editor.fill(DOCUMENT);

  // span注釈はデバウンス後のプレビュー応答で届く。装飾の出現を待つ。
  const strong = page.locator(".lp-strong");
  await expect(strong).toHaveText("太字");
  const headingLine = page.locator(".cm-line.lp-heading-0");
  await expect(headingLine).toHaveCount(1);

  // カーソルが離れている間は記法文字が折り畳まれる。
  const strongLine = page.locator(".cm-line", { hasText: "太字" });
  await expect(strongLine).not.toContainText("**");
  await expect(headingLine).not.toContainText("= ");

  // 装飾された本文をクリックすると記法が現れ、外すと再び折り畳まれる。
  await strong.click();
  await expect(strongLine).toContainText("**太字**");
  await page.keyboard.press("Control+End");
  await expect(strongLine).not.toContainText("**");

  // 装飾を無効にすると素の原文へ戻る。
  await page.getByRole("button", { name: "装飾" }).click();
  await expect(page.locator(".lp-strong")).toHaveCount(0);
  await expect(strongLine).toContainText("**太字**");
  await page.getByRole("button", { name: "装飾" }).click();
  await expect(page.locator(".lp-strong")).toHaveText("太字");
});

// 数式spanの位置。"= 題名\n:stem: latexmath\n\n面積は stem:[x^2] です。" に対する値。
const MATH_DOCUMENT = "= 題名\n:stem: latexmath\n\n面積は stem:[x^2] です。";
const MATH_SPANS = [
  {
    kind: "document_title",
    span: { start: 0, end: 9, unit: "utf8_byte" },
    content_span: { start: 2, end: 8, unit: "utf8_byte" },
    marker_spans: [
      { start: 0, end: 1, unit: "utf8_byte" },
      { start: 1, end: 2, unit: "utf8_byte" },
    ],
  },
  {
    kind: "document_attribute",
    span: { start: 9, end: 26, unit: "utf8_byte" },
  },
  {
    kind: "inline_math",
    span: { start: 37, end: 47, unit: "utf8_byte" },
    content_span: { start: 43, end: 46, unit: "utf8_byte" },
    marker_spans: [
      { start: 37, end: 43, unit: "utf8_byte" },
      { start: 46, end: 47, unit: "utf8_byte" },
    ],
  },
];

test("編集欄の数式を組版し、カーソル交差で原文を開示する", async ({ page }) => {
  await page.route("**/api/v3/notes/preview", async (route) => {
    const source = (await route.request().postDataJSON()).source;
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: "<p>プレビュー</p>",
        math_macros: [],
        diagnostics: [],
        spans: source === MATH_DOCUMENT ? MATH_SPANS : [],
      }),
    });
  });
  await page.goto("/notes/new");
  const editor = page.getByRole("textbox", { name: "AsciiDoc文書" });
  await expect(editor).toBeFocused();
  await editor.fill(MATH_DOCUMENT);

  // MathJaxの読み込みと組版を待つ。組版後はSVGのcontainerが入る。
  const widget = page.locator(".lp-math");
  await expect(widget.locator("mjx-container")).toBeVisible({
    timeout: 10_000,
  });
  const mathLine = page.locator(".cm-line", { hasText: "面積は" });
  await expect(mathLine).not.toContainText("stem:[");

  // 行頭から記法の先頭までカーソルを動かすと原文が現れる。
  await mathLine.click();
  await page.keyboard.press("Home");
  for (let step = 0; step < 4; step += 1) {
    await page.keyboard.press("ArrowRight");
  }
  await expect(mathLine).toContainText("stem:[x^2]");
});
