const { test: base, expect } = require("@playwright/test");
const {
  browserDiagnostic,
  diagnosticSummary,
  safeSource,
} = require("./diagnostic-classification");

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


module.exports = {
  browserDiagnostic,
  diagnosticSummary,
  expect,
  safeSource,
  test,
};
