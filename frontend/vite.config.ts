import react from "@vitejs/plugin-react";
import { cpSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { defineConfig } from "vitest/config";

const require = createRequire(import.meta.url);

export default defineConfig({
  base: "./",
  plugins: [
    react(),
    {
      name: "copy-mathjax-font-data",
      closeBundle() {
        const packageDirectory = dirname(
          require.resolve("@mathjax/mathjax-newcm-font/package.json"),
        );
        cpSync(
          resolve(packageDirectory, "svg", "dynamic"),
          resolve(
            "dist",
            "assets",
            "mathjax-fonts",
            "mathjax-newcm-font",
            "svg",
            "dynamic",
          ),
          { recursive: true },
        );
      },
    },
  ],
  build: {
    chunkSizeWarningLimit: 600,
    manifest: "manifest.json",
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        editor: "src/main.tsx",
        page: "src/page.ts",
      },
      output: {
        entryFileNames: "assets/[name].js",
        assetFileNames: (asset) =>
          asset.names.some((name) => name.endsWith(".woff2"))
            ? "assets/fonts/[name].[ext]"
            : "assets/[name].[ext]",
      },
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["tests/setup.ts"],
  },
});
