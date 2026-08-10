import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    port: 5173,
    // Web 开发模式下，/api 代理到本地 Rust 后端
    proxy: {
      "/api": {
        target: "http://127.0.0.1:17321",
        changeOrigin: true,
      },
    },
  },
});
