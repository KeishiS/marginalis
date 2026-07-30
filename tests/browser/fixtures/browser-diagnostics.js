const { test: base, expect } = require("@playwright/test");
const path = require("node:path");

const test = base.extend({
  browserDiagnostics: [
    async ({ context }, use) => {
      const diagnostics = [];
      const allowedDiagnostics = [];
      const observedPages = new WeakSet();
      const observe = (page) => {
        if (observedPages.has(page)) return;
        observedPages.add(page);
        page.on("console", (message) => {
          if (message.type() !== "warning" && message.type() !== "error")
            return;
          diagnostics.push(
            browserDiagnostic(
              `console.${message.type() === "warning" ? "warn" : "error"}`,
              message.text(),
              message.location(),
            ),
          );
        });
        page.on("pageerror", (error) => {
          diagnostics.push(
            browserDiagnostic("pageerror", error.message, {
              url: "",
              lineNumber: 0,
              columnNumber: 0,
            }),
          );
        });
      };

      context.pages().forEach(observe);
      context.on("page", observe);
      await use({
        diagnostics,
        observe,
        allow: (predicate) => allowedDiagnostics.push(predicate),
      });
      context.off("page", observe);

      const unexpected = diagnostics.filter(
        (diagnostic) =>
          !allowedDiagnostics.some((allowed) => allowed(diagnostic)),
      );
      expect(
        unexpected,
        "ブラウザーコンソールに想定外の警告、エラー、または未処理例外があります。",
      ).toEqual([]);
    },
    { auto: true },
  ],
});

function browserDiagnostic(kind, message, location) {
  return {
    kind,
    summary: diagnosticSummary(kind, message),
    source: safeSource(location),
  };
}

function diagnosticSummary(kind, message) {
  const normalized = String(message).toLowerCase();
  const httpStatus = normalized.match(
    /(?:status(?: code)?(?: of)?|status_code)[^0-9]*(\d{3})/,
  )?.[1];
  if (normalized.includes("failed to load resource") && httpStatus) {
    return `HTTP ${httpStatus}応答`;
  }
  if (
    normalized.includes("content security policy") ||
    normalized.includes("violates the following directive") ||
    normalized.includes("refused to apply") ||
    normalized.includes("refused to load")
  ) {
    if (normalized.includes("style-src-attr")) {
      return "Content Security Policy違反（style-src-attr）";
    }
    if (normalized.includes("style-src-elem")) {
      return "Content Security Policy違反（style-src-elem）";
    }
    if (normalized.includes("inline style")) {
      return "Content Security Policy違反（inline style）";
    }
    return "Content Security Policy違反";
  }
  if (
    normalized.includes("mathjax") ||
    normalized.includes("dynamic file") ||
    normalized.includes("double-struck")
  ) {
    return "MathJax資源の読み込みまたは組版の失敗";
  }
  if (
    normalized.includes("unhandled promise") ||
    normalized.includes("uncaught (in promise)")
  ) {
    return "未処理のPromise拒否";
  }
  if (kind === "console.warn") return "ブラウザーコンソールの警告";
  if (kind === "console.error") return "ブラウザーコンソールのエラー";
  return "ページ内の未処理例外";
}

function safeSource({ url = "", lineNumber = 0, columnNumber = 0 } = {}) {
  let file = "ページ";
  if (url) {
    try {
      file = path.posix.basename(new URL(url).pathname) || "ページ";
    } catch {
      file = "不明な資源";
    }
  }
  return `${file}:${lineNumber}:${columnNumber}`;
}

module.exports = {
  browserDiagnostic,
  diagnosticSummary,
  expect,
  safeSource,
  test,
};
