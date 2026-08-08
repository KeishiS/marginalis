const { defineConfig } = require("@playwright/test");

// NixOS VM上で実サーバーとKanidmに対して実行するspecの共有設定。
// specを追加する場合はtestMatchへ足すだけでよく、flake.nix側の編集は不要。
module.exports = defineConfig({
  testDir: ".",
  testMatch: [
    "kanidm-login.spec.js",
    "webui-editing.spec.js",
    "webui-acl.spec.js",
  ],
  reporter: "line",
  workers: 1,
});
