const { defineConfig } = require("@playwright/test");

module.exports = defineConfig({
  testDir: ".",
  testMatch: "webui-smoke.spec.js",
  timeout: 15_000,
  use: {
    baseURL: "http://127.0.0.1:42877",
    browserName: "chromium",
  },
  webServer: {
    command:
      "cp fixtures/webui-smoke.html ../../frontend/dist/index.html && cd ../../frontend && npm exec -- vite preview --host 127.0.0.1 --port 42877",
    url: "http://127.0.0.1:42877",
    reuseExistingServer: false,
    timeout: 15_000,
  },
  reporter: "line",
  workers: 1,
});
