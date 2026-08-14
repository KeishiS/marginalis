const { expect, test } = require("./fixtures/browser-diagnostics");
const {
  SCREENSHOT_OPTIONS,
  detailScreenshotOptions,
} = require("./fixtures/smoke-helpers");

test("グラフビューで点を選ぶと、その画面へ移動できる", async ({ page }) => {
  const noteId = "0197c9bc-0000-7000-8000-000000000001";
  const otherId = "0197c9bc-0000-7000-8000-000000000002";
  await page.route("**/api/v3/notes/graph*", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        notes: [
          {
            note_id: noteId,
            title: "先行研究の整理",
            tags: ["研究"],
            updated_at_ms: 2,
          },
          { note_id: otherId, title: "検証メモ", tags: [], updated_at_ms: 1 },
        ],
        works: [{ citation_key: "smith2024", title: "An Example Article" }],
        references: [{ source_note_id: noteId, target_note_id: otherId }],
        citations: [{ source_note_id: noteId, citation_key: "smith2024" }],
      }),
    });
  });

  await page.goto("/graph");
  await page.waitForLoadState("networkidle");
  await page.evaluate(() => document.fonts.ready);

  // 点はノートと文献の両方が出る。線は参照と引用で描き分ける。
  const vertices = page.locator(".graph-vertex");
  await expect(vertices).toHaveCount(3);
  await expect(page.locator('.graph-edge[data-kind="reference"]')).toHaveCount(
    1,
  );
  await expect(page.locator('.graph-edge[data-kind="citation"]')).toHaveCount(
    1,
  );

  // ノートの点は閲覧画面、文献の点は文献ライブラリを指す。
  const note = page.locator('.graph-vertex[data-kind="note"]').first();
  expect(await note.getAttribute("href")).toBe(`/notes/${noteId}`);
  const work = page.locator('.graph-vertex[data-kind="work"]').first();
  expect(await work.getAttribute("href")).toBe("/bibliography?query=smith2024");
  await expect(note.locator("text")).toContainText("先行研究の整理");
  await expect(work.locator("text")).toHaveCount(0);

  // 文献の題名は図へ常時表示せず、点に触れたときに全文を示す。
  await work.hover();
  await expect(page.locator(".graph-detail")).toContainText(
    "An Example Article",
  );

  // 点に触れると、更新日時とタグを吹き出しで示す。図の枠の内側へ収まる。
  await note.hover();
  const detail = page.locator(".graph-detail");
  await expect(detail).toContainText("先行研究の整理");
  await expect(detail).toContainText("研究");
  expect(await detail.locator("time").getAttribute("datetime")).toBe(
    new Date(2).toISOString(),
  );
  const detailBox = await detail.boundingBox();
  const figureBox = await page.locator(".graph-canvas").boundingBox();
  expect(detailBox.x).toBeGreaterThanOrEqual(figureBox.x - 1);
  expect(detailBox.x + detailBox.width).toBeLessThanOrEqual(
    figureBox.x + figureBox.width + 1,
  );
  await expect(page).toHaveScreenshot(
    "graph-vertex-detail.png",
    detailScreenshotOptions(page),
  );

  // 起点を指定すると、その範囲だけを要求し、階層を選び直せる帯が出る。
  await page.goto(`/graph?origin=${noteId}&depth=2`);
  await page.waitForLoadState("networkidle");
  await expect(page.locator(".graph-origin")).toContainText("先行研究の整理");
  expect(await page.locator(".graph-origin select").inputValue()).toBe("2");
  await page.locator(".graph-origin select").selectOption("4");
  await expect(page).toHaveURL(`/graph?origin=${noteId}&depth=4`);
  await page.getByRole("button", { name: "全体を見る" }).click();
  await expect(page.locator(".graph-origin")).toHaveCount(0);
  await expect(page).toHaveURL("/graph");

  await page.getByRole("textbox", { name: "語で絞り込む" }).fill("研究 メモ");
  await page.getByRole("button", { name: "絞り込む" }).click();
  await expect(page).toHaveURL(
    "/graph?query=%E7%A0%94%E7%A9%B6+%E3%83%A1%E3%83%A2",
  );

  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(page).toHaveScreenshot("graph-wide.png", SCREENSHOT_OPTIONS);
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page).toHaveScreenshot(
    "graph-wide-dark.png",
    SCREENSHOT_OPTIONS,
  );

  // 図はマウスがなくても使える。絞り込みの次にTabで届く点をEnterで開く。
  await page.emulateMedia({ colorScheme: "light" });
  await page.getByRole("button", { name: "条件を解除" }).click();
  await expect(page).toHaveURL("/graph");
  await expect(vertices).toHaveCount(3);
  await page.getByRole("button", { name: "絞り込む" }).focus();
  await page.keyboard.press("Tab");
  expect(
    await page.evaluate(() => document.activeElement?.getAttribute("href")),
  ).toBe(`/notes/${noteId}`);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(new RegExp(`/notes/${noteId}$`));
});
