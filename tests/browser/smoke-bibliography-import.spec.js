const { expect, test } = require("./fixtures/browser-diagnostics");

test("CSL-JSONの競合をキーボードで解決して一括取り込みする", async ({
  page,
}) => {
  const sourceId = "0197c9bc-0000-7000-8000-000000000101";
  const itemId = "0197c9bc-0000-7000-8000-000000000102";
  let applied = null;
  await page.route("**/api/v3/bibliography**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path.endsWith("/import-sources")) {
      await route.fulfill({ contentType: "application/json", body: "[]" });
      return;
    }
    if (path.endsWith("/import-previews")) {
      await route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({
          source_id: null,
          source_revision: null,
          preview_token: "a".repeat(64),
          entries: [
            {
              position: 0,
              external_item_id: "smith2026",
              citation_key: "smith2026",
              classification: "conflict",
              item_id: itemId,
              item_revision: 2,
              current_csl_json: {
                id: "smith2026",
                title: "Marginalis側の文献",
                type: "book",
              },
              candidates: [],
              rejection_code: null,
            },
          ],
        }),
      });
      return;
    }
    if (path.endsWith("/imports")) {
      applied = request.postDataJSON();
      await route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({
          source_id: sourceId,
          source_revision: 1,
          created: 0,
          updated: 0,
          kept: 1,
          excluded: 0,
        }),
      });
      return;
    }
    await route.fulfill({ contentType: "application/json", body: "[]" });
  });

  await page.goto("/bibliography");
  await page
    .getByRole("button", { name: "CSL-JSONをまとめて取り込む" })
    .click();
  await page.getByLabel("取込元の表示名").fill("Zotero研究ライブラリー");
  await page.getByLabel("CSL-JSONファイル").setInputFiles({
    name: "library.json",
    mimeType: "application/json",
    buffer: Buffer.from(
      JSON.stringify([{ id: "smith2026", type: "book", title: "研究資料" }]),
    ),
  });
  await page.getByRole("button", { name: "事前確認" }).click();

  const apply = page.getByRole("button", { name: "選択した計画を取り込む" });
  await expect(apply).toBeDisabled();
  await page.getByText("Marginalis側の現在値").click();
  await expect(page.getByText(/Marginalis側の文献/)).toBeVisible();
  const decision = page.getByLabel("1件目の処理");
  await decision.focus();
  await decision.press("ArrowDown");
  await decision.press("Enter");
  await expect(decision).toHaveValue("keep_local");
  await expect(apply).toBeEnabled();
  await apply.click();

  await expect(
    page.getByRole("status").filter({ hasText: "保持1件" }),
  ).toContainText("保持1件");
  expect(applied.preview_token).toBe("a".repeat(64));
  expect(applied.decisions).toEqual([
    { position: 0, action: "keep_local", candidate_item_id: null },
  ]);
});
