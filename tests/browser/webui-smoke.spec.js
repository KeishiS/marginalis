const {
  browserDiagnostic,
  diagnosticSummary,
  expect,
  test,
} = require("./fixtures/browser-diagnostics");

const pendingWebProvenance = {
  created_via: "web",
  review_status: "pending",
  reviewed_revision: null,
  reviewed_at_ms: null,
};

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

  // 狭い画面でも主要な移動先を隠さない。押せる大きさと現在位置も保つ。
  const navigation = page.getByRole("navigation", { name: "主要な画面" });
  for (const [label, href] of [
    ["ノート", "/"],
    ["書誌", "/bibliography"],
    ["関係の図", "/graph"],
    ["設定", "/settings"],
    ["新規ノート", "/notes/new"],
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

  // 画面全体が横へはみ出さない。
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(360);
});

test("ブラウザー診断を本文やtokenを含まない分類へ変換する", () => {
  const secret = "Bearer secret-token ノート本文";
  expect(
    diagnosticSummary(
      "console.error",
      `Refused to load a script because it violates the following directive: ${secret}`,
    ),
  ).toBe("Content Security Policy違反");
  expect(
    diagnosticSummary(
      "pageerror",
      `dynamic file 'double-struck' failed to load: ${secret}`,
    ),
  ).toBe("MathJax資源の読み込みまたは組版の失敗");

  const diagnostic = browserDiagnostic("console.error", secret, {
    url: "https://example.test/notes/private?token=secret-token",
    lineNumber: 12,
    columnNumber: 4,
  });
  expect(JSON.stringify(diagnostic)).not.toContain("secret-token");
  expect(JSON.stringify(diagnostic)).not.toContain("ノート本文");
  expect(diagnostic.source).toBe("private:12:4");
});

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
  const documentSurface = page.locator(".document-surface");
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

  // 文章は行の長さを制限し、表とコードは器の幅まで使う。
  const paragraphPosition = await page
    .locator(".rendered-content > p")
    .first()
    .boundingBox();
  const tablePosition = await page
    .locator(".rendered-content table")
    .first()
    .boundingBox();
  expect(paragraphPosition.width).toBeLessThanOrEqual(46 * 16);
  expect(tablePosition.width).toBeGreaterThan(paragraphPosition.width);

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
  await expect(page.getByRole("status")).toHaveText(
    "note IDをコピーしました。",
  );
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
  const dialog = page.getByRole("dialog", {
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
    page.getByRole("dialog", { name: "このノートを復元しますか？" }),
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

test("CodeMirrorで行番号、表示切替、日本語入力状態を扱う", async ({ page }) => {
  await page.route("**/api/v3/notes/preview", async (route) => {
    const source = (await route.request().postDataJSON()).source;
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: `<div class="preview-content"><p>${escapeHtml(source)}</p></div>`,
        math_macros: [],
        diagnostics: [],
      }),
    });
  });
  await page.goto("/notes/new");
  const editor = page.getByRole("textbox", { name: "AsciiDoc文書" });
  await expect(editor).toBeFocused();
  await editor.fill("= 編集画面\n\n1行目\n2行目");
  const firstLine = page.locator(".cm-line").first();
  const firstLineNumber = page
    .locator(".cm-lineNumbers .cm-gutterElement")
    .nth(1);
  await expect(firstLineNumber).toHaveText("1");
  const linePosition = await firstLine.boundingBox();
  const lineNumberPosition = await firstLineNumber.boundingBox();
  expect(linePosition).not.toBeNull();
  expect(lineNumberPosition).not.toBeNull();
  expect(lineNumberPosition.x + lineNumberPosition.width).toBeLessThanOrEqual(
    linePosition.x,
  );
  await expect(page.getByRole("toolbar", { name: "入力補助" })).toHaveCount(0);

  await editor.evaluate((element) =>
    element.dispatchEvent(
      new CompositionEvent("compositionstart", {
        bubbles: true,
        data: "編集中",
      }),
    ),
  );
  await expect(page.getByText("日本語入力を確定してください。")).toBeVisible();
  await editor.evaluate((element) =>
    element.dispatchEvent(
      new CompositionEvent("compositionend", {
        bubbles: true,
        data: "確定",
      }),
    ),
  );

  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(page).toHaveScreenshot(
    "editor-wide-split.png",
    editorScreenshotOptions(page),
  );
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page).toHaveScreenshot(
    "editor-wide-split-dark.png",
    editorScreenshotOptions(page),
  );
  await page.emulateMedia({ colorScheme: "light" });
  await page.setViewportSize({ width: 320, height: 720 });
  await expect(page.locator(".editor-workspace")).toHaveAttribute(
    "data-view-mode",
    "write",
  );
  await expect(page.getByRole("button", { name: "分割" })).toBeDisabled();
  await expect(page).toHaveScreenshot(
    "editor-narrow-write.png",
    editorScreenshotOptions(page),
  );
});

test("数式を組版したまま分割表示とプレビュー表示を切り替える", async ({
  page,
}) => {
  const fontRequests = [];
  const extensionResponses = [];
  page.on("request", (request) => {
    if (request.url().includes("mathjax-newcm-font")) {
      fontRequests.push(request.url());
    }
  });
  page.on("response", (response) => {
    if (
      response.url().endsWith("/assets/boldsymbol.js") ||
      response.url().endsWith("/assets/mathtools.js")
    ) {
      extensionResponses.push({
        url: response.url(),
        status: response.status(),
      });
    }
  });
  await page.route("**/api/v3/notes/preview", async (route) => {
    const source = (await route.request().postDataJSON()).source;
    const html = source.includes(String.raw`stem:[\lambda]`)
      ? String.raw`<p>インライン数式 <code class="math-latex" data-math-language="latexmath" data-math-display="inline">f(x) \coloneqq \argmax_{x \in S} f(x) + \bm{x},\quad x \in \mathbb{R}</code>のチェックです。</p>` +
        (source.includes("プレビューから分割への確認")
          ? "<p>プレビューから分割への確認</p>"
          : "")
      : "<p>プレビュー</p>";
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html,
        math_macros: [
          {
            name: "argmax",
            replacement: String.raw`\operatorname*{arg\,max}`,
            argument_count: 0,
          },
          {
            name: "bm",
            replacement: String.raw`\boldsymbol{#1}`,
            argument_count: 1,
          },
        ],
        diagnostics: [],
      }),
    });
  });
  await page.goto("/notes/new");
  await page.getByRole("button", { name: "執筆" }).click();
  await page.getByRole("textbox", { name: "AsciiDoc文書" }).fill(
    String.raw`= 新規ノート
:marginalis-tags:
:sectnums:

== 見出し1

インライン数式 stem:[\lambda]のチェックです。`,
  );

  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "分割" }).click();

  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(page.locator(".preview-content mjx-merror")).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    page.locator(".preview-content [data-math-prepared='true']"),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "プレビュー" }).click();
  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(0);

  await page.getByRole("button", { name: "執筆" }).click();
  await page.getByRole("textbox", { name: "AsciiDoc文書" }).fill(
    String.raw`= 新規ノート
:marginalis-tags:
:sectnums:

== 見出し1

インライン数式 stem:[\lambda]のチェックです。

プレビューから分割への確認`,
  );
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "プレビュー" }).click();
  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await page.getByRole("button", { name: "分割" }).click();
  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(
    page.locator(
      ".preview-content .math-latex:not([data-math-prepared='true'])",
    ),
  ).toHaveCount(0);
  expect(fontRequests).toContain(
    "http://127.0.0.1:42877/assets/mathjax-fonts/mathjax-newcm-font/svg/dynamic/double-struck.js",
  );
  expect(
    fontRequests.every(
      (url) => new URL(url).origin === page.url().replace(/\/notes\/new$/, ""),
    ),
  ).toBe(true);
  expect(extensionResponses).toEqual(
    expect.arrayContaining([
      {
        url: "http://127.0.0.1:42877/assets/boldsymbol.js",
        status: 200,
      },
      {
        url: "http://127.0.0.1:42877/assets/mathtools.js",
        status: 200,
      },
    ]),
  );
});

test("許可していないTeX packageを数式から読み込まない", async ({ page }) => {
  const unexpectedExtensionRequests = [];
  page.on("request", (request) => {
    if (/\/(?:autoload|require|html|color)\.js(?:\?|$)/.test(request.url())) {
      unexpectedExtensionRequests.push(request.url());
    }
  });
  await page.route("**/api/v3/notes/preview", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: String.raw`<p><code class="math-latex" data-math-language="latexmath" data-math-display="inline">x + \require{html}\href{https://example.test}{y} + \color{red}{z}</code></p>`,
        math_macros: [],
        diagnostics: [],
      }),
    });
  });

  await page.goto("/notes/new");
  await page.getByRole("button", { name: "執筆" }).click();
  await page
    .getByRole("textbox", { name: "AsciiDoc文書" })
    .fill("= TeX package制限\n\nstem:[x]");
  await page.getByRole("button", { name: "分割" }).click();

  await expect(page.locator(".preview-content mjx-container")).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(await page.evaluate(() => window.MathJax.config.tex.packages)).toEqual(
    [
      "base",
      "ams",
      "newcommand",
      "textmacros",
      "noundefined",
      "configmacros",
      "boldsymbol",
      "mathtools",
    ],
  );
  expect(unexpectedExtensionRequests).toEqual([]);
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

test("5,000行の文書を編集して保存できる", async ({ page }) => {
  test.setTimeout(30_000);
  const source = [
    "= 長文試験",
    "",
    ...Array.from(
      { length: 4998 },
      (_, index) => `${index + 3}行目の日本語とemoji😀`,
    ),
  ].join("\n");
  expect(Buffer.byteLength(source, "utf8")).toBeLessThanOrEqual(512 * 1024);

  await page.route("**/api/v3/notes/preview", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        html: '<div class="preview-content"><p>長文プレビュー</p></div>',
        math_macros: [],
        diagnostics: [],
      }),
    });
  });
  await page.route("**/api/v3/web/notes", async (route) => {
    if (route.request().method() !== "POST") {
      await route.fallback();
      return;
    }
    const input = await route.request().postDataJSON();
    await route.fulfill({
      status: 201,
      contentType: "application/json",
      body: JSON.stringify({
        note_id: "0197c9bc-0000-7000-8000-000000000099",
        title: "長文試験",
        source: input.source,
        tags: [],
        created_at_ms: 1,
        updated_at_ms: 1,
        revision: 1,
        ...pendingWebProvenance,
      }),
    });
  });
  await page.goto("/notes/new");
  const editor = page.getByRole("textbox", { name: "AsciiDoc文書" });
  await editor.focus();
  await editor.press("Control+a");
  const insertionTime = await editor.evaluate((element, input) => {
    const clipboardData = new DataTransfer();
    clipboardData.setData("text/plain", input);
    const started = performance.now();
    element.dispatchEvent(
      new ClipboardEvent("paste", {
        bubbles: true,
        clipboardData,
      }),
    );
    return performance.now() - started;
  }, source);
  expect(insertionTime).toBeLessThan(5_000);
  await expect(page.getByText("未保存の変更があります。")).toBeVisible();
  await page.getByRole("button", { name: "プレビュー" }).click();
  await page.getByRole("button", { name: "執筆" }).click();
  await expect(editor).toBeFocused();
  await editor.press("Control+s");
  const toast = page.locator(".toast");
  await expect(toast.getByText("保存しました。")).toBeVisible();
  const toastPosition = await toast.boundingBox();
  const viewport = page.viewportSize();
  expect(toastPosition).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(toastPosition.y).toBeLessThan(130);
  expect(viewport.width - (toastPosition.x + toastPosition.width)).toBeLessThan(
    40,
  );
});

function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

const SCREENSHOT_OPTIONS = {
  animations: "disabled",
  // Linux環境ごとのフォント描画差を許容し、配置の大きな崩れは検出します。
  maxDiffPixelRatio: 0.03,
};

/**
 * 編集欄の中身を隠して比較する。
 *
 * CodeMirrorが描く文字は環境ごとの差が大きく、実行環境を変えると配置が同じでも許容差を
 * 超える。隠す対象は入力した文字だけで、行番号、枠、操作、分割の位置は比較に残る。
 */
function editorScreenshotOptions(page) {
  return { ...SCREENSHOT_OPTIONS, mask: [page.locator(".cm-content")] };
}

/**
 * 日時の表示を隠して比較する。
 *
 * 日時は実行環境の地域と時間帯で文字列が変わる。値そのものはDOMの`datetime`属性で確かめ、
 * 画像では吹き出しの位置と大きさだけを比較する。
 */
function detailScreenshotOptions(page) {
  return { ...SCREENSHOT_OPTIONS, mask: [page.locator(".graph-detail time")] };
}

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

  await expect(page.getByRole("status")).toContainText("保持1件");
  expect(applied.preview_token).toBe("a".repeat(64));
  expect(applied.decisions).toEqual([
    { position: 0, action: "keep_local", candidate_item_id: null },
  ]);
});

test("関係の図で点を選ぶと、その画面へ移動できる", async ({ page }) => {
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

  // ノートの点は閲覧画面、文献の点は書誌ライブラリーを指す。
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
  await page.getByRole("button", { name: "全体を見る" }).click();
  await expect(page.locator(".graph-origin")).toHaveCount(0);

  // 図と同じ内容を一覧からも辿れる。
  await page.getByText("つながりの一覧").click();
  await expect(
    page.locator(".graph-outline a", { hasText: "先行研究の整理" }),
  ).toBeVisible();

  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(page).toHaveScreenshot("graph-wide.png", SCREENSHOT_OPTIONS);
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page).toHaveScreenshot(
    "graph-wide-dark.png",
    SCREENSHOT_OPTIONS,
  );

  // 図はマウスがなくても使える。絞り込みの次にTabで届く点をEnterで開く。
  await page.emulateMedia({ colorScheme: "light" });
  await page.getByRole("button", { name: "絞り込む" }).focus();
  await page.keyboard.press("Tab");
  expect(
    await page.evaluate(() => document.activeElement?.getAttribute("href")),
  ).toBe(`/notes/${noteId}`);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(new RegExp(`/notes/${noteId}$`));
});
