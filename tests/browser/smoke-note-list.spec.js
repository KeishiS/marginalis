const { expect, test } = require("./fixtures/browser-diagnostics");
const {
  pendingWebProvenance,
  SCREENSHOT_OPTIONS,
} = require("./fixtures/smoke-helpers");

test("production build starts and renders a note returned by the API", async ({
  page,
}) => {
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
          ...pendingWebProvenance,
          access: "manage",
        },
      ]),
    });
  });

  await page.goto("/");
  await page.waitForLoadState("networkidle");
  await page.evaluate(() => document.fonts.ready);

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

  // 狭い画面では移動先をメニューへまとめる。開くまでは畳んでおく。
  const navigation = page.getByRole("navigation", { name: "主要な画面" });
  const menu = navigation.getByRole("group");
  const menuButton = navigation.getByText("メニュー", { exact: true });
  await expect(menuButton).toBeVisible();
  await expect(navigation.getByRole("link", { name: "設定" })).toBeHidden();
  // 新規ノートはメニューへ入れず、常に押せるようにする。
  await expect(
    navigation.getByRole("link", { name: "新規ノート" }),
  ).toBeVisible();

  await menuButton.click();
  await expect(menu).toHaveAttribute("open", "");
  for (const [label, href] of [
    ["ノート", "/"],
    ["書誌", "/bibliography"],
    ["関係の図", "/graph"],
    ["設定", "/settings"],
  ]) {
    const destination = navigation.getByRole("link", {
      name: label,
      exact: true,
    });
    await expect(destination).toBeVisible();
    expect(await destination.getAttribute("href")).toBe(href);
    const box = await destination.boundingBox();
    expect(box.height).toBeGreaterThanOrEqual(40);
    expect(box.x + box.width).toBeLessThanOrEqual(360);
  }
  expect(
    await navigation.locator("[aria-current='page']").getAttribute("href"),
  ).toBe("/");

  // メニューを開いても画面全体が横へはみ出さない。
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(360);

  // 広い画面ではメニューボタンを出さず、移動先を横に並べる。
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(menuButton).toBeHidden();
  await expect(navigation.getByRole("link", { name: "設定" })).toBeVisible();

  // さらに広い画面では、一覧の要素も閲覧画面と同じ96rem(1536px)の上限を共有する。
  // 画面を移動しても要素の幅が変わらないようにするため。
  await page.setViewportSize({ width: 2560, height: 800 });
  const filterForm = page.locator("form", {
    has: page.getByRole("button", { name: "絞り込む" }),
  });
  await expect(filterForm).toBeVisible();
  const filterPosition = await filterForm.boundingBox();
  expect(filterPosition).not.toBeNull();
  expect(filterPosition.width).toBeLessThanOrEqual(1536);
  expect(filterPosition.width).toBeGreaterThan(1400);
});
