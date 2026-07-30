const { defineConfig } = require("@playwright/test");

module.exports = defineConfig({
  testDir: ".",
  testMatch:
    process.env.MARGINALIS_COMPAT_ONLY === "true"
      ? "webui-editor-compat.spec.js"
      : ["webui-smoke.spec.js", "webui-editor-compat.spec.js"],
  outputDir: "../../test-results/browser-smoke",
  timeout: 15_000,
  use: {
    baseURL: "http://127.0.0.1:42877",
    browserName: process.env.MARGINALIS_BROWSER || "chromium",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  webServer: {
    command:
      "cp fixtures/webui-smoke.html ../../frontend/dist/index.html && cd ../../frontend && pnpm exec vite preview --host 127.0.0.1 --port 42877",
    url: "http://127.0.0.1:42877",
    reuseExistingServer: false,
    timeout: 15_000,
  },
  reporter: "line",
  workers: 1,
});
