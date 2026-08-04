const {
  browserDiagnostic,
  diagnosticSummary,
  expect,
  test,
} = require("./fixtures/browser-diagnostics");

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
        },
        access: "manage",
        math_macros: [],
        html:
          "<article><h1>広い閲覧画面</h1><p>長い文章と表、コード、数式を読みやすい幅で表示します。</p>" +
          "<table><tr><th>項目</th><th>説明</th><th>確認</th></tr><tr><td>本文幅</td><td>広い画面を活用します</td><td>成功</td></tr></table>" +
          '<pre data-language="rust"><code>fn main() { println!("wide"); }</code></pre></article>',
        related: { outgoing: [], incoming: [] },
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
  await page.route("**/api/v3/notes", async (route) => {
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
  await expect(page.locator('.graph-edge[data-kind="citation"]')).toHaveCount(1);

  // ノートの点は閲覧画面、文献の点は書誌ライブラリーを指す。
  const note = page.locator('.graph-vertex[data-kind="note"]').first();
  expect(await note.getAttribute("href")).toBe(`/notes/${noteId}`);
  const work = page.locator('.graph-vertex[data-kind="work"]').first();
  expect(await work.getAttribute("href")).toBe(
    "/bibliography?query=smith2024",
  );

  // 点に触れると、更新日時とタグを吹き出しで示す。図の枠の内側へ収まる。
  await note.hover();
  const detail = page.locator(".graph-detail");
  await expect(detail).toContainText("先行研究の整理");
  await expect(detail).toContainText("研究");
  expect(
    await detail.locator("time").getAttribute("datetime"),
  ).toBe(new Date(2).toISOString());
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
