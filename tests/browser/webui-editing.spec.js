const { test, expect } = require("./fixtures/browser-diagnostics");

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
      name === "marginalis_csrf" && domain === "marginalis.example.test",
  );
  expect(cookie).toBeTruthy();
  return cookie.value;
}

test("Web UI creates, previews, edits, and resolves a revision conflict", async ({
  page,
  context,
  browserDiagnostics,
}) => {
  // NixOS VM上ではログイン、数式描画、競合解決、診断表示を通した
  // 一連の受入確認に30秒を超えることがあるため、試験全体の上限を明示する。
  test.setTimeout(60_000);
  browserDiagnostics.allow((diagnostic) =>
    ["HTTP 404応答", "HTTP 409応答", "HTTP 422応答"].includes(
      diagnostic.summary,
    ),
  );
  await page.addInitScript(() => {
    window.__marginalisCspViolations = [];
    document.addEventListener("securitypolicyviolation", (event) => {
      window.__marginalisCspViolations.push({
        blockedURI: event.blockedURI,
        effectiveDirective: event.effectiveDirective,
      });
    });
  });
  await login(page);

  const script = await context.request.get(`${baseUrl}/assets/editor.js`);
  expect(script.status()).toBe(200);
  expect(script.headers()["content-type"]).toContain("javascript");
  const stylesheet = await context.request.get(`${baseUrl}/assets/editor.css`);
  expect(stylesheet.status()).toBe(200);
  expect(stylesheet.headers()["content-type"]).toContain("text/css");

  await page.getByRole("link", { name: "新規ノート" }).click();
  await expect(
    page.getByRole("heading", { name: "ノートの作成" }),
  ).toBeVisible();
  const source = page.getByRole("textbox", { name: "AsciiDoc文書" });
  await expect(source).toBeFocused();
  await expect(page.getByRole("button", { name: "分割" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  const workspace = page.locator(".editor-workspace");
  await expect(workspace).not.toHaveAttribute("style", /.+/);
  await page.getByRole("slider", { name: /執筆欄の幅/ }).fill("65");
  await expect(workspace).toHaveAttribute("data-editor-width", "65");
  const paneRatio = await page.evaluate(() => {
    const sourcePane = document.querySelector(".editor-source-pane");
    const previewPane = document.querySelector(".preview-scroll");
    return (
      sourcePane.getBoundingClientRect().width /
      previewPane.getBoundingClientRect().width
    );
  });
  expect(paneRatio).toBeCloseTo(65 / 35, 1);
  expect(
    await page.evaluate(() =>
      window.__marginalisCspViolations.filter(
        ({ effectiveDirective }) =>
          effectiveDirective === "style-src-attr" ||
          effectiveDirective === "style-src-elem",
      ),
    ),
  ).toEqual([]);
  await page.getByRole("button", { name: "執筆" }).click();
  await expect(page.locator(".editor-workspace")).toHaveAttribute(
    "data-view-mode",
    "write",
  );
  await expect(source).toBeFocused();
  await page.getByRole("button", { name: "プレビュー" }).click();
  await expect(page.locator(".editor-workspace")).toHaveAttribute(
    "data-view-mode",
    "preview",
  );
  await page.getByRole("button", { name: "分割" }).click();
  const documentSource =
    "= VMで作成したノート\n:marginalis-tags: 受入試験, 日本語\n:stem: latexmath\n\n.実行例\n[source,rust]\n----\nfn main() {}\n----\n\nstem:[x^2 + y^2]\n\n日本語と絵文字😀\r\n\n*強調した本文*\n\n* 最初の行 +\n続きの行\n* 次の項目";
  await source.fill(documentSource);
  await expect(page.getByText("未保存の変更があります。")).toBeVisible();
  await expect(page.locator(".preview-content")).toContainText(
    "日本語と絵文字😀",
  );
  await expect(
    page.locator(".preview-content pre[data-language='rust']"),
  ).toContainText("fn main() {}");
  const previewListItems = page.locator(".preview-content ul > li");
  await expect(previewListItems).toHaveCount(2);
  await expect(previewListItems.first()).toContainText("最初の行");
  await expect(previewListItems.first().locator("br")).toHaveCount(1);
  await expect(previewListItems.first()).toContainText("続きの行");
  const previewSource = page.locator(".preview-content figure.source-block");
  await expect(previewSource.locator("figcaption")).toHaveText("実行例");
  await expect(previewSource.locator(".source-line")).toHaveAttribute(
    "data-line-number",
    "1",
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

  await page.getByRole("button", { name: "執筆" }).click();
  await source.fill(`${documentSource}\n\n非表示中の更新`);
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "プレビュー" }).click();
  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(
    page.locator(".preview-content [data-math-prepared='true']"),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "分割" }).click();
  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(0);

  await page.getByRole("button", { name: "執筆" }).click();
  await source.fill(`${documentSource}\n\n分割表示用の更新`);
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "分割" }).click();
  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await page.getByRole("button", { name: "プレビュー" }).click();
  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "分割" }).click();

  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("保存しました。")).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`${baseUrl}/notes/[0-9a-f-]+/edit$`));
  const noteId = page.url().match(/\/notes\/([^/]+)\/edit$/)?.[1];
  expect(noteId).toBeTruthy();

  await page.getByRole("link", { name: "閲覧画面へ戻る" }).click();
  await expect(
    page.getByRole("heading", { name: "VMで作成したノート" }),
  ).toBeVisible();
  await expect(page.locator(".page-main")).toContainText("日本語と絵文字😀");
  const renderedSource = page.locator(".page-main figure.source-block");
  await expect(renderedSource.locator("figcaption")).toHaveText("実行例");
  await expect(renderedSource.locator(".source-line")).toHaveAttribute(
    "data-line-number",
    "1",
  );
  await expect(page.locator(".page-main mjx-container")).toBeVisible();
  const renderedListItems = page.locator(".page-main .rendered-content ul > li");
  await expect(renderedListItems).toHaveCount(2);
  await expect(renderedListItems.first().locator("br")).toHaveCount(1);
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
  await source.fill(
    "= VMで作成したノート\n\n更新した本文\n\n== 結果\n\n成功😀",
  );
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
    "= 競合後に保存する題名\n\n更新した本文\n\n== 結果\n\n成功😀",
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
  await expect(source).toContainText("競合後に保存する題名");
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
    `= 競合後に保存する題名\n:marginalis-tags: 受入試験, 日本語\n\nxref:note:${targetId}[参照先へ]`,
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

  await page.goto(`${baseUrl}/notes/new`);
  const warningSource = page.getByRole("textbox", {
    name: "AsciiDoc文書",
  });
  await warningSource.fill(
    "= 警告を確認するノート\n\nこの結果はxref:note:0197c9bc-0000-7000-8000-000000000002[参照]に記載されています。",
  );
  const warning = page.locator(".cm-lintRange-warning");
  await expect(warning).toHaveText("xref");
  await warning.hover();
  await expect(page.locator(".cm-tooltip-lint")).toContainText(
    "インラインマクロの前に空白を入れてください。",
  );
  await warningSource.press("F8");
  expect(
    await warningSource.evaluate(() => window.getSelection()?.toString()),
  ).toBe("xref");
  await page.getByRole("button", { name: "保存" }).click();
  await expect(page.getByText("保存しました。")).toBeVisible();
  await warningSource.fill(
    "= 警告を確認するノート\n\nこの結果は xref:note:0197c9bc-0000-7000-8000-000000000002[参照]に記載されています。",
  );
  await expect(page.locator(".cm-lintRange-warning")).toHaveCount(0);
  expect(await page.evaluate(() => window.__marginalisCspViolations)).toEqual(
    [],
  );
});
