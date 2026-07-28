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
  expect(browserErrors).toEqual([]);

  await expect(
    page.getByRole("heading", { name: "ノート", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "ブラウザー基本試験" }),
  ).toHaveAttribute("href", "/notes/0197c9bc-0000-7000-8000-000000000001");
  await expect(page.getByRole("status")).toContainText("1件のノート");
});
