const { defineConfig } = require("@playwright/test");

// 配信対象はtarget/browser-smoke-site。frontend/distと実装から導出したHTMLシェルを
// `cargo make browser-smoke-site`が組み立て、テストはビルド成果物を変更しない。
module.exports = defineConfig({
  testDir: ".",
  testMatch:
    process.env.MARGINALIS_COMPAT_ONLY === "true"
      ? "webui-editor-compat.spec.js"
      : "smoke-*.spec.js",
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
      "cd ../../frontend && pnpm exec vite preview --outDir ../target/browser-smoke-site --host 127.0.0.1 --port 42877",
    url: "http://127.0.0.1:42877",
    reuseExistingServer: false,
    timeout: 15_000,
  },
  reporter: "line",
  workers: 1,
});
