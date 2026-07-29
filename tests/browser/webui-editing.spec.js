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
  const source = page.getByRole("textbox", { name: "AsciiDoc文書" });
  await expect(source).toBeFocused();
  await source.fill(
    "= VMで作成したノート\n:tags: 受入試験, 日本語\n:stem: latexmath\n\n.実行例\n[source,rust,linenums,start=7]\n----\nfn main() {}\n----\n\nstem:[x^2 + y^2]\n\n日本語と絵文字😀\r\n\n*強調した本文*",
  );
  await expect(page.getByText("未保存の変更があります。")).toBeVisible();
  await expect(page.locator(".preview-content")).toContainText(
    "日本語と絵文字😀",
  );
  await expect(
    page.locator(".preview-content pre[data-language='rust']"),
  ).toContainText("fn main() {}");
  const previewSource = page.locator(".preview-content figure.source-block");
  await expect(previewSource.locator("figcaption")).toHaveText("実行例");
  await expect(previewSource.locator("pre")).toHaveAttribute(
    "data-line-numbers",
    "true",
  );
  await expect(previewSource.locator("pre")).toHaveAttribute(
    "data-line-start",
    "7",
  );
  await expect(previewSource.locator(".source-line")).toHaveAttribute(
    "data-line-number",
    "7",
  );
  await expect
    .poll(async () => {
      if ((await page.locator(".preview-content mjx-container").count()) > 0) {
        return "rendered";
      }
      const error = page.getByRole("alert").filter({
        hasText: "数式を描画できませんでした",
      });
      return (await error.count()) > 0
        ? `failed: ${await error.innerText()}`
        : "pending";
    })
    .toBe("rendered");

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
  const renderedSource = page.locator(".page-main figure.source-block");
  await expect(renderedSource.locator("figcaption")).toHaveText("実行例");
  await expect(renderedSource.locator("pre")).toHaveAttribute(
    "data-line-numbers",
    "true",
  );
  await expect(renderedSource.locator("pre")).toHaveAttribute(
    "data-line-start",
    "7",
  );
  await expect(renderedSource.locator(".source-line")).toHaveAttribute(
    "data-line-number",
    "7",
  );
  await expect(page.locator(".page-main mjx-container")).toBeVisible();
  await page.getByRole("link", { name: "編集" }).click();

  await expect(page.locator(".preview-content")).toContainText(
    "日本語と絵文字😀",
  );
  await source.fill("= VMで作成したノート\n\ninclude::secret[]");
  await expect(
    page.getByRole("heading", { name: "プレビューできませんでした" }),
  ).toBeVisible();
  await expect(page.getByText(/includeディレクティブ/)).toBeVisible();
  await expect(page.locator(".preview-content")).toContainText(
    "日本語と絵文字😀",
  );
  await source.fill("= VMで作成したノート\n\n更新した本文\n\n== 結果\n\n成功😀");
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
        source: current.source.replace(
          "= VMで作成したノート",
          "= 別操作で更新した題名",
        ),
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

  await source.fill(
    (await source.inputValue()).replace(
      "= VMで作成したノート",
      "= 競合後に保存する題名",
    ),
  );
  await page.getByRole("button", { name: "保存" }).click();
  const conflictHeading = page.getByRole("heading", {
    name: "更新内容の競合",
  });
  await expect(conflictHeading).toBeVisible();
  await expect(conflictHeading).toBeFocused();
  const conflictTable = page.getByRole("table", {
    name: "本文の行単位比較",
  });
  await expect(
    conflictTable.getByRole("columnheader", { name: "編集中" }),
  ).toBeVisible();
  await expect(
    conflictTable.getByRole("columnheader", {
      name: "現在保存されている内容",
    }),
  ).toBeVisible();
  await expect(conflictTable).toContainText("競合後に保存する題名");
  await expect(conflictTable).toContainText("別操作で更新した題名");

  await page
    .getByRole("button", { name: /更新番号3を編集の基準にする/ })
    .click();
  await expect(
    page.getByText(
      "更新番号3を基準にしました。内容を確認して保存してください。",
    ),
  ).toBeVisible();
  await expect(source).toHaveValue(/= 競合後に保存する題名/);
  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("更新番号: 4")).toBeVisible();
  await expect(page.getByText("保存しました。")).toBeVisible();

  await page.goto(`${baseUrl}/notes/new`);
  await page
    .getByRole("textbox", { name: "AsciiDoc文書" })
    .fill("= 参照先ノート\n\n参照先の本文");
  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("保存しました。")).toBeVisible();
  const targetId = page.url().match(/\/notes\/([^/]+)\/edit$/)?.[1];
  expect(targetId).toBeTruthy();

  await page.goto(`${baseUrl}/notes/${noteId}/edit`);
  await source.fill(
    `= 競合後に保存する題名\n:tags: 受入試験, 日本語\n\nxref:note:${targetId}[参照先へ]`,
  );
  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("保存しました。")).toBeVisible();
  await page.getByRole("link", { name: "閲覧画面へ戻る" }).click();
  const relatedNotes = page.getByRole("complementary", { name: "関連ノート" });
  await expect(relatedNotes).toContainText("参照先ノート");
  await relatedNotes.getByRole("link", { name: "参照先ノート" }).click();
  await expect(page).toHaveURL(`${baseUrl}/notes/${targetId}`);
  await expect(
    page.getByRole("complementary", { name: "関連ノート" }),
  ).toContainText("競合後に保存する題名");

  const listQuery = new URLSearchParams({ tag: "受入試験" });
  await page.goto(`${baseUrl}/?${listQuery}`);
  await expect(page.getByLabel("タグ", { exact: true })).toHaveValue(
    "受入試験",
  );
  await expect(page.getByRole("status")).toContainText("1件のノート");
  await expect(page.getByText("所有")).toBeVisible();
  await page.getByRole("link", { name: "競合後に保存する題名" }).click();
  await page.getByRole("link", { name: "一覧", exact: true }).click();
  await expect(page).toHaveURL(`${baseUrl}/?${listQuery}`);
});
