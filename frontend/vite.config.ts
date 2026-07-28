import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  build: {
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
        assetFileNames: "assets/editor.[ext]",
      },
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["tests/setup.ts"],
  },
});
