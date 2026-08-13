const { expect, test } = require("./fixtures/browser-diagnostics");
const {
  pendingWebProvenance,
  escapeHtml,
  SCREENSHOT_OPTIONS,
  editorScreenshotOptions,
  detailScreenshotOptions,
} = require("./fixtures/smoke-helpers");

test("閲覧画面でnote IDをコピーし、広い本文を表示する", async ({
  page,
  context,
}) => {
  const noteId = "0197c9bc-0000-7000-8000-000000000001";
  let deleteRequest = null;
  let restoreRequest = null;
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
          tags: ["設計", "Rust", "長いタグ名でも狭い画面からはみ出さない"],
          created_at_ms: 1,
          updated_at_ms: 1,
          revision: 1,
          ...pendingWebProvenance,
        },
        access: "manage",
        math_macros: [],
        // AdocWeaveは見出しも段落も包み要素なしで並べる。実際の描画と同じ構造にする。
        html:
          '<h1 class="document-title">広い閲覧画面</h1>' +
          "<p>長い文章と表、コード、数式を読みやすい幅で表示します。段落は行の長さを制限し、" +
          "表とコードは器の幅まで使います。</p>" +
          "<h1>章の見出し</h1><h2>節の見出し</h2>" +
          "<dl><dt>用語</dt><dd>用語の説明です。用語と説明を見分けられるようにします。</dd>" +
          "<dt>次の用語</dt><dd>次の説明です。</dd></dl>" +
          "<table><tr><th>項目</th><th>説明</th><th>確認</th></tr><tr><td>本文幅</td><td>広い画面を活用します</td><td>成功</td></tr></table>" +
          '<pre data-language="rust"><code>fn main() { println!("wide"); }</code></pre>',
        related: { outgoing: [], incoming: [] },
      }),
    });
  });
  await page.route(`**/api/v3/notes/${noteId}`, async (route) => {
    deleteRequest = {
      method: route.request().method(),
      headers: await route.request().allHeaders(),
    };
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        note_id: noteId,
        title: "広い閲覧画面",
        source: "= 広い閲覧画面\n\n本文",
        tags: ["設計", "Rust"],
        created_at_ms: 1,
        updated_at_ms: 2,
        revision: 2,
        ...pendingWebProvenance,
      }),
    });
  });
  await page.route("**/api/v3/notes", async (route) => {
    await route.fulfill({ contentType: "application/json", body: "[]" });
  });
  await page.route("**/api/v3/notes/deleted", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify([
        {
          note_id: noteId,
          title: "広い閲覧画面",
          deleted_at_ms: Date.now() - 1_000,
          purge_at_ms: Date.now() + 30 * 24 * 60 * 60 * 1_000,
          revision: 2,
        },
      ]),
    });
  });
  await page.route(`**/api/v3/notes/${noteId}/restore`, async (route) => {
    restoreRequest = {
      method: route.request().method(),
      headers: await route.request().allHeaders(),
    };
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        note_id: noteId,
        title: "広い閲覧画面",
        source: "= 広い閲覧画面\n\n本文",
        tags: ["設計", "Rust"],
        created_at_ms: 1,
        updated_at_ms: 3,
        revision: 3,
        ...pendingWebProvenance,
      }),
    });
  });

  await page.goto(`/notes/${noteId}`);
  await page.waitForLoadState("networkidle");
  await page.evaluate(() => document.fonts.ready);
  // 閲覧本文を包む枠。旧document-surface classの代わりに、本文との親子関係で特定する。
  const documentSurface = page.locator("div:has(> .rendered-content)");
  await expect(documentSurface).toBeVisible();
  const typography = await page.evaluate(() => ({
    reading: getComputedStyle(document.querySelector(".rendered-content"))
      .fontFamily,
    ui: getComputedStyle(document.body).fontFamily,
  }));
  expect(typography.ui).toContain("Noto Sans JP Variable");
  expect(typography.reading).toContain("Noto Serif JP Variable");
  const documentPosition = await documentSurface.boundingBox();
  expect(documentPosition).not.toBeNull();
  expect(documentPosition.width).toBeGreaterThan(1000);

  // 文章の行の長さは制限しない。文字の幅は言語によって違い、一つの上限では決められないため、
  // 段落も表を包む枠も器の幅を使う。
  const paragraphPosition = await page
    .locator(".rendered-content > p")
    .first()
    .boundingBox();
  const tableScrollPosition = await page
    .locator(".rendered-content .table-scroll")
    .first()
    .boundingBox();
  const contentWidth = await page
    .locator(".rendered-content")
    .boundingBox()
    .then((box) => box.width);
  expect(paragraphPosition.width).toBe(contentWidth);
  expect(tableScrollPosition.width).toBe(contentWidth);

  // 見出しは題名から本文へ段階的に小さくなる。同じ大きさの段が並ばない。
  const scale = await page.evaluate(() => {
    const size = (selector) =>
      Number.parseFloat(
        getComputedStyle(document.querySelector(selector)).fontSize,
      );
    return {
      title: size(".rendered-content h1.document-title"),
      chapter: size(".rendered-content h1:not(.document-title)"),
      section: size(".rendered-content h2"),
      body: size(".rendered-content > p"),
    };
  });
  expect(scale.title).toBeGreaterThan(scale.chapter);
  expect(scale.chapter).toBeGreaterThan(scale.section);
  expect(scale.section).toBeGreaterThan(scale.body);
  expect(scale.title / scale.body).toBeLessThan(2.2);

  // 定義リストは、説明の開始位置を用語からずらして関係を示す。
  const termPosition = await page
    .locator(".rendered-content dt")
    .first()
    .boundingBox();
  const descriptionPosition = await page
    .locator(".rendered-content dd")
    .first()
    .boundingBox();
  expect(descriptionPosition.x).toBeGreaterThan(termPosition.x);
  const termWeight = await page.evaluate(
    () =>
      getComputedStyle(document.querySelector(".rendered-content dt"))
        .fontWeight,
  );
  expect(Number.parseInt(termWeight, 10)).toBeGreaterThanOrEqual(700);

  await page.getByRole("button", { name: "note IDをコピー" }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "note ID" }),
  ).toHaveText("note IDをコピーしました。");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    noteId,
  );
  await expect(page.getByRole("list", { name: "ノートのタグ" })).toContainText(
    "設計",
  );
  expect(
    await page
      .getByRole("list", { name: "ノートのタグ" })
      .locator("li")
      .allTextContents(),
  ).toEqual(["設計", "Rust", "長いタグ名でも狭い画面からはみ出さない"]);
  // 閲覧画面から、このノートを起点にした関係の図へ移れる。
  expect(
    await page.getByRole("link", { name: "周辺の関係" }).getAttribute("href"),
  ).toBe(`/graph?origin=${noteId}&depth=2`);
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
  const tagPosition = await page
    .getByRole("list", { name: "ノートのタグ" })
    .boundingBox();
  expect(tagPosition).not.toBeNull();
  expect(tagPosition.x + tagPosition.width).toBeLessThanOrEqual(360);

  // 表とコードブロックは、本文ごと横へ広げず要素の内側でスクロールする。
  const scrollable = await page.evaluate(() => {
    const measure = (selector) => {
      const element = document.querySelector(selector);
      return {
        width: Math.round(element.getBoundingClientRect().width),
        scrollWidth: element.scrollWidth,
      };
    };
    return {
      document: document.documentElement.scrollWidth,
      table: measure(".rendered-content .table-scroll"),
      code: measure(".rendered-content pre"),
    };
  });
  expect(scrollable.document).toBeLessThanOrEqual(360);
  expect(scrollable.table.width).toBeLessThanOrEqual(336);
  expect(scrollable.table.scrollWidth).toBeGreaterThan(scrollable.table.width);
  expect(scrollable.code.width).toBeLessThanOrEqual(336);

  await expect(page).toHaveScreenshot(
    "note-view-narrow.png",
    SCREENSHOT_OPTIONS,
  );

  // 幅の広い画面では、上限まで本文領域を広げる。ヘッダーと左右端がそろうことも確認する。
  await page.setViewportSize({ width: 1600, height: 900 });
  await expect(documentSurface).toBeVisible();
  const widePosition = await documentSurface.boundingBox();
  expect(widePosition).not.toBeNull();
  expect(widePosition.width).toBeGreaterThan(1400);
  const brandPosition = await page.locator(".brand").boundingBox();
  expect(brandPosition).not.toBeNull();
  expect(Math.abs(brandPosition.x - widePosition.x)).toBeLessThanOrEqual(1);

  await page.evaluate(() => {
    document.cookie = "marginalis_csrf=browser-csrf; path=/";
  });
  const deleteButton = page.getByRole("button", { name: "削除", exact: true });
  await deleteButton.click();
  const dialog = page.getByRole("alertdialog", {
    name: "このノートを削除しますか？",
  });
  await expect(dialog).toContainText("広い閲覧画面");
  await expect(dialog).toContainText("削除後30日以内");
  await expect(page.getByRole("button", { name: "取り消す" })).toBeFocused();
  await page.getByRole("button", { name: "取り消す" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(deleteButton).toBeFocused();

  await deleteButton.click();
  await page.getByRole("button", { name: "削除する", exact: true }).click();
  await expect(page).toHaveURL(/\/?notice=note-deleted$/);
  await expect(
    page.getByRole("status").filter({ hasText: "ノートを削除しました" }),
  ).toBeVisible();
  expect(deleteRequest.method).toBe("DELETE");
  expect(deleteRequest.headers["if-match"]).toBe('"rev-1"');
  expect(deleteRequest.headers["x-csrf-token"]).toBe("browser-csrf");

  await page.getByRole("link", { name: "削除済みノート" }).click();
  await expect(
    page.getByRole("heading", { name: "削除済みノート" }),
  ).toBeVisible();
  await expect(page.getByText("広い閲覧画面")).toBeVisible();
  await expect(page.getByText("rev-2")).toBeVisible();
  await page.getByRole("button", { name: "復元", exact: true }).click();
  await expect(
    page.getByRole("alertdialog", { name: "このノートを復元しますか？" }),
  ).toContainText("広い閲覧画面");
  await page.getByRole("button", { name: "復元する" }).click();
  await expect(page).toHaveURL(/\/?notice=note-restored$/);
  await expect(
    page.getByRole("status").filter({ hasText: "ノートを復元しました" }),
  ).toBeVisible();
  expect(restoreRequest.method).toBe("POST");
  expect(restoreRequest.headers["if-match"]).toBe('"rev-2"');
  expect(restoreRequest.headers["x-csrf-token"]).toBe("browser-csrf");
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
          ...pendingWebProvenance,
        },
        access: "manage",
        math_macros: [],
        html: String.raw`<p><code class="math-latex" data-math-language="latexmath" data-math-display="inline">\mathbb{R}</code></p>`,
        related: { outgoing: [], incoming: [] },
      }),
    });
  });

  await page.goto(`/notes/${noteId}`);
  await expect(page.locator(".rendered-content mjx-container")).toBeVisible();
  expect(
    await page
      .locator("style[id^='MJX-']")
      .evaluateAll((styles) =>
        styles.every((style) => style.nonce === "browser-smoke"),
      ),
  ).toBe(true);
  expect(fontResponses).toContainEqual({
    url: "http://127.0.0.1:42877/assets/mathjax-fonts/mathjax-newcm-font/svg/dynamic/double-struck.js",
    status: 200,
  });
  await expect(page.getByRole("alert")).toHaveCount(0);
});
