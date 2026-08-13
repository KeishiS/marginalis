const { expect, test } = require("./fixtures/browser-diagnostics");

test("書誌情報の未保存編集と削除を確認してから反映する", async ({ page }) => {
  let items = [
    {
      item_id: "0197c9bc-0000-7000-8000-0000000000a1",
      citation_key: "smith2024",
      csl_json: { id: "smith2024", type: "book", title: "An Example" },
      created_at_ms: 1,
      updated_at_ms: 1,
      revision: 1,
    },
    {
      item_id: "0197c9bc-0000-7000-8000-0000000000a2",
      citation_key: "tanaka2025",
      csl_json: { id: "tanaka2025", type: "book", title: "別の文献" },
      created_at_ms: 1,
      updated_at_ms: 1,
      revision: 1,
    },
  ];
  await page.route("**/api/v3/bibliography**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname.endsWith("/import-sources")) {
      await route.fulfill({ contentType: "application/json", body: "[]" });
    } else if (request.method() === "DELETE") {
      const itemId = decodeURIComponent(url.pathname.split("/").at(-1));
      items = items.filter((item) => item.item_id !== itemId);
      await route.fulfill({ status: 204, body: "" });
    } else {
      await route.fulfill({
        contentType: "application/json",
        body: JSON.stringify(items),
      });
    }
  });

  await page.goto("/bibliography");
  await page.getByRole("button", { name: /smith2024/ }).click();
  const input = page.getByRole("textbox", { name: "CSL-JSON" });
  await input.fill('{"id":"draft"}');
  await page.getByRole("button", { name: /tanaka2025/ }).click();
  const discard = page.getByRole("alertdialog");
  await expect(discard).toContainText("編集中の内容を破棄しますか");
  await discard.getByRole("button", { name: "取り消す" }).click();
  await expect(input).toHaveValue('{"id":"draft"}');

  await page.getByRole("button", { name: /tanaka2025/ }).click();
  await discard.getByRole("button", { name: "変更を破棄" }).click();
  await expect(input).toContainText("tanaka2025");

  const item = page.locator(".bibliography-list li", { hasText: "tanaka2025" });
  await item.getByRole("button", { name: "削除" }).click();
  const deletion = page.getByRole("alertdialog");
  await expect(deletion).toContainText("書誌情報の削除は取り消せません");
  await deletion.getByRole("button", { name: "削除する" }).click();
  await expect(item).toHaveCount(0);
  await expect(
    page.getByRole("status").filter({ hasText: "削除しました" }),
  ).toContainText("削除しました");
});
