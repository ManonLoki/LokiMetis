import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

/** 建立 Tauri 本地前端构建配置，并关闭生产源码映射。 */
export default defineConfig({
  plugins: [
    tanstackRouter({
      autoCodeSplitting: true,
      target: "react",
    }),
    react(),
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    sourcemap: false,
    target: "es2022",
  },
  test: {
    clearMocks: true,
    css: true,
    environment: "jsdom",
    setupFiles: ["./tests/setup.ts"],
  },
});
