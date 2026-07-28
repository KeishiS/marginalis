const { test, expect } = require("@playwright/test");

const baseUrl = "https://marginalis.example.test/marginalis";

async function login(page) {
  await page.goto(`${baseUrl}/auth/oidc/login?next=%2Fmarginalis%2F`);
  await page
    .getByRole("textbox", { name: "Username", exact: true })
    .fill("idm_admin");
  await page
    .getByRole("textbox", { name: "Username", exact: true })
    .press("Enter");
  await page.getByLabel(/password/i).fill("test-idm-admin-password");
  await page.getByLabel(/password/i).press("Enter");
  const proceed = page.getByRole("button", {
    name: "Proceed",
    exact: true,
  });
  const outcome = await Promise.race([
    page.waitForURL(`${baseUrl}/`).then(() => "redirected"),
    proceed.waitFor({ state: "visible" }).then(() => "proceed"),
  ]);
  if (outcome === "proceed") {
    await proceed.click();
  }
  await expect(page).toHaveURL(`${baseUrl}/`);
}

async function csrfToken(context) {
  const cookie = (await context.cookies()).find(
    ({ name, domain }) =>
      name === "marginalis_csrf" &&
      domain === "marginalis.example.test",
  );
  expect(cookie).toBeTruthy();
  return cookie.value;
}

test("Web UI creates, previews, edits, and resolves a revision conflict", async ({
  page,
  context,
}) => {
  await login(page);

  const script = await context.request.get(
    `${baseUrl}/assets/editor.js`,
  );
  expect(script.status()).toBe(200);
  expect(script.headers()["content-type"]).toContain("javascript");
  const stylesheet = await context.request.get(
    `${baseUrl}/assets/editor.css`,
  );
  expect(stylesheet.status()).toBe(200);
  expect(stylesheet.headers()["content-type"]).toContain("text/css");

  await page.getByRole("link", { name: "新規ノート" }).click();
  await expect(
    page.getByRole("heading", { name: "ノートの作成" }),
  ).toBeVisible();
  const title = page.getByRole("textbox", { name: "題名" });
  const body = page.getByRole("textbox", { name: "本文（AsciiDoc）" });
  const tags = page.getByRole("textbox", {
    name: "タグ（コンマ区切り）",
  });
  await expect(title).toBeFocused();
  await title.fill("VMで作成したノート");
  await body.fill("日本語と絵文字😀\r\n\n*強調した本文*");
  await tags.fill("受入試験, 日本語");
  await expect(page.getByText("未保存の変更があります。")).toBeVisible();
  await expect(page.locator(".preview-content")).toContainText(
    "日本語と絵文字😀",
  );

  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("保存しました。")).toBeVisible();
  await expect(page).toHaveURL(
    new RegExp(`${baseUrl}/notes/[0-9a-f-]+/edit$`),
  );
  const noteId = page.url().match(/\/notes\/([^/]+)\/edit$/)?.[1];
  expect(noteId).toBeTruthy();

  await page.getByRole("link", { name: "閲覧画面へ戻る" }).click();
  await expect(
    page.getByRole("heading", { name: "VMで作成したノート" }),
  ).toBeVisible();
  await expect(page.locator(".page-main")).toContainText("日本語と絵文字😀");
  await page.getByRole("link", { name: "編集" }).click();

  await body.fill("include::secret[]");
  await expect(
    page.getByRole("heading", { name: "プレビューできませんでした" }),
  ).toBeVisible();
  await expect(page.getByText(/includeディレクティブ/)).toBeVisible();
  await body.fill("更新した本文\n\n== 結果\n\n成功😀");
  await expect(page.locator(".preview-content")).toContainText("成功😀");
  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("更新番号: 2")).toBeVisible();

  const currentResponse = await context.request.get(
    `${baseUrl}/api/v3/notes/${noteId}`,
  );
  expect(currentResponse.status()).toBe(200);
  const current = await currentResponse.json();
  const externalUpdate = await context.request.put(
    `${baseUrl}/api/v3/notes/${noteId}`,
    {
      data: {
        title: "別操作で更新した題名",
        body: current.body,
        tags: current.tags,
      },
      headers: {
        Origin: "https://marginalis.example.test",
        "Sec-Fetch-Site": "same-origin",
        "X-CSRF-Token": await csrfToken(context),
        "If-Match": `"rev-${current.revision}"`,
      },
      failOnStatusCode: false,
    },
  );
  expect(externalUpdate.status()).toBe(200);

  await title.fill("競合後に保存する題名");
  await page.getByRole("button", { name: "保存" }).click();
  const conflictHeading = page.getByRole("heading", {
    name: "更新内容の競合",
  });
  await expect(conflictHeading).toBeVisible();
  await expect(conflictHeading).toBeFocused();
  await expect(
    page.getByRole("region", { name: "編集中" }),
  ).toContainText("競合後に保存する題名");
  await expect(
    page.getByRole("region", { name: "現在保存されている内容" }),
  ).toContainText("別操作で更新した題名");

  await page
    .getByRole("button", { name: /更新番号3を編集の基準にする/ })
    .click();
  await expect(
    page.getByText(
      "更新番号3を基準にしました。内容を確認して保存してください。",
    ),
  ).toBeVisible();
  await expect(title).toHaveValue("競合後に保存する題名");
  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("更新番号: 4")).toBeVisible();
  await expect(page.getByText("変更は保存されています。")).toBeVisible();

  await page.goto(`${baseUrl}/notes/new`);
  await page.getByRole("textbox", { name: "題名" }).fill("参照先ノート");
  await page
    .getByRole("textbox", { name: "本文（AsciiDoc）" })
    .fill("参照先の本文");
  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("保存しました。")).toBeVisible();
  const targetId = page.url().match(/\/notes\/([^/]+)\/edit$/)?.[1];
  expect(targetId).toBeTruthy();

  await page.goto(`${baseUrl}/notes/${noteId}/edit`);
  await body.fill(`xref:note:${targetId}[参照先へ]`);
  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("変更は保存されています。")).toBeVisible();
  await page.getByRole("link", { name: "閲覧画面へ戻る" }).click();
  const relatedNotes = page.getByRole("complementary", { name: "関連ノート" });
  await expect(relatedNotes).toContainText("参照先ノート");
  await relatedNotes.getByRole("link", { name: "参照先ノート" }).click();
  await expect(page).toHaveURL(`${baseUrl}/notes/${targetId}`);
  await expect(
    page.getByRole("complementary", { name: "関連ノート" }),
  ).toContainText("競合後に保存する題名");
});
