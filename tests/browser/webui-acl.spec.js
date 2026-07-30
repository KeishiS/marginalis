const { test, expect } = require("./fixtures/browser-diagnostics");

const baseUrl = "https://marginalis.example.test/marginalis";

async function loginOwner(page) {
  await page.goto(`${baseUrl}/auth/oidc/login?next=%2Fmarginalis%2F`);
  await page
    .getByRole("textbox", { name: "Username", exact: true })
    .fill("idm_admin");
  await page
    .getByRole("textbox", { name: "Username", exact: true })
    .press("Enter");
  await page.getByLabel(/password/i).fill("test-idm-admin-password");
  await page.getByLabel(/password/i).press("Enter");
  const proceed = page.getByRole("button", { name: "Proceed", exact: true });
  const outcome = await Promise.race([
    page.waitForURL(`${baseUrl}/`).then(() => "redirected"),
    proceed.waitFor({ state: "visible" }).then(() => "proceed"),
  ]);
  if (outcome === "proceed") {
    await proceed.click();
  }
  await expect(page).toHaveURL(`${baseUrl}/`);
  await expect(page.getByText("閲覧できるノートはありません。")).toBeVisible();
}

async function actorContext(browser, session, csrf) {
  const context = await browser.newContext({ ignoreHTTPSErrors: true });
  await context.addCookies([
    {
      name: "marginalis_session",
      value: session,
      domain: "marginalis.example.test",
      path: "/marginalis",
      secure: true,
      sameSite: "Lax",
    },
    {
      name: "marginalis_csrf",
      value: csrf,
      domain: "marginalis.example.test",
      path: "/marginalis",
      secure: true,
      sameSite: "Strict",
    },
  ]);
  return context;
}

test("ACLは所有者、閲覧者、編集者、対象外利用者の境界を保つ", async ({
  page,
  browser,
  browserDiagnostics,
}) => {
  await loginOwner(page);
  await page.getByRole("link", { name: "新規ノート" }).click();
  await page
    .getByRole("textbox", { name: "AsciiDoc文書" })
    .fill("= ACL受入試験\n\n共有前の本文");
  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("保存しました。")).toBeVisible();
  const noteId = page.url().match(/\/notes\/([^/]+)\/edit$/)?.[1];
  expect(noteId).toBeTruthy();
  await page.getByRole("link", { name: "閲覧画面へ戻る" }).click();
  await page.getByRole("link", { name: "共有設定" }).click();

  const subject = page.getByRole("textbox", { name: "利用者subject" });
  await subject.fill("reader-subject");
  await page.getByRole("button", { name: "共有先を追加" }).click();
  await subject.fill("editor-subject");
  await page.getByRole("combobox", { name: "権限" }).selectOption("edit");
  await page.getByRole("button", { name: "共有先を追加" }).click();
  await page.getByRole("button", { name: "共有設定を保存" }).click();
  await expect(page.getByText("共有設定を保存しました。")).toBeVisible();

  const reader = await actorContext(browser, "reader-session", "reader-csrf");
  const readerPage = await reader.newPage();
  browserDiagnostics.observe(readerPage);
  await readerPage.goto(`${baseUrl}/notes/${noteId}`);
  await expect(
    readerPage.getByRole("heading", { name: "ACL受入試験" }),
  ).toBeVisible();
  await expect(readerPage.getByRole("link", { name: "編集" })).toHaveCount(0);
  await expect(readerPage.getByRole("link", { name: "共有設定" })).toHaveCount(
    0,
  );
  const readerUpdateStatus = await readerPage.evaluate(
    async ({ baseUrl, noteId }) => {
      const response = await fetch(`${baseUrl}/api/v3/notes/${noteId}`, {
        method: "PUT",
        headers: {
          "content-type": "application/json",
          "x-csrf-token": "reader-csrf",
          "if-match": '"rev-2"',
        },
        body: JSON.stringify({
          source: "= 変更不可\n\n変更不可",
        }),
      });
      return response.status;
    },
    { baseUrl, noteId },
  );
  expect(readerUpdateStatus).toBe(404);

  const editor = await actorContext(browser, "editor-session", "editor-csrf");
  const editorPage = await editor.newPage();
  browserDiagnostics.observe(editorPage);
  await editorPage.goto(`${baseUrl}/notes/${noteId}`);
  await expect(editorPage.getByRole("link", { name: "編集" })).toBeVisible();
  await expect(editorPage.getByRole("link", { name: "共有設定" })).toHaveCount(
    0,
  );
  await editorPage.getByRole("link", { name: "編集" }).click();
  await editorPage
    .getByRole("textbox", { name: "AsciiDoc文書" })
    .fill("= 編集者が更新した題名\n\n共有前の本文");
  await editorPage.getByRole("button", { name: "保存" }).click();
  await expect(editorPage.getByText("更新番号: 3")).toBeVisible();
  const editorDeleteStatus = await editorPage.evaluate(
    async ({ baseUrl, noteId }) => {
      const response = await fetch(`${baseUrl}/api/v3/notes/${noteId}`, {
        method: "DELETE",
        headers: {
          "content-type": "application/json",
          "x-csrf-token": "editor-csrf",
          "if-match": '"rev-3"',
        },
      });
      return response.status;
    },
    { baseUrl, noteId },
  );
  expect(editorDeleteStatus).toBe(404);

  const outsider = await actorContext(
    browser,
    "outsider-session",
    "outsider-csrf",
  );
  const outsiderPage = await outsider.newPage();
  browserDiagnostics.observe(outsiderPage);
  const hidden = await outsiderPage.goto(`${baseUrl}/notes/${noteId}`);
  expect(hidden.status()).toBe(200);
  await expect(
    outsiderPage.getByRole("alert").filter({
      hasText: "ノートを読み込めませんでした。",
    }),
  ).toBeVisible();
  await expect(
    outsiderPage.getByRole("heading", { name: "編集者が更新した題名" }),
  ).toHaveCount(0);
  const hiddenApi = await outsider.request.get(
    `${baseUrl}/api/v3/notes/${noteId}`,
  );
  expect(hiddenApi.status()).toBe(404);
  await outsiderPage.goto(`${baseUrl}/`);
  await expect(
    outsiderPage.getByText("閲覧できるノートはありません。"),
  ).toBeVisible();
  await expect(outsiderPage.getByText("ACL受入試験")).toHaveCount(0);
  await expect(outsiderPage.getByText("編集者が更新した題名")).toHaveCount(0);

  await page.goto(`${baseUrl}/notes/${noteId}`);
  await expect(
    page.getByRole("heading", { name: "編集者が更新した題名" }),
  ).toBeVisible();
  await expect(page.getByRole("link", { name: "共有設定" })).toBeVisible();

  await Promise.all([reader.close(), editor.close(), outsider.close()]);
});
