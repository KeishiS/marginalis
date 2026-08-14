// tests/browser のPlaywright spec向けのESLint設定。
//
// flat configは設定ファイルのあるディレクトリーを基点とし、その外側のファイルを
// 対象にできないため、リポジトリ直下に置く。frontend配下のlintは
// frontend/eslint.config.js が受け持ち、そちらの実行時はこの設定を参照しない。
// ESLint本体と依存はfrontendのものを使うため、createRequireでfrontendから解決する。
import { createRequire } from "node:module";

const require = createRequire(new URL("./frontend/package.json", import.meta.url));
const js = require("@eslint/js");
const globals = require("globals");

export default [
  {
    files: ["tests/browser/**/*.js"],
    ...js.configs.recommended,
  },
  {
    files: ["tests/browser/**/*.js"],
    // specはCommonJSで、Node上の実行とpage.evaluate内のブラウザーコードが混在する。
    languageOptions: {
      sourceType: "commonjs",
      globals: {
        ...globals.node,
        ...globals.browser,
      },
    },
  },
];
