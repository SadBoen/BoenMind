import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  build: {
    rollupOptions: {
      // 多入口:主界面 + 输入对照测试页(排查真键盘吞字)
      input: {
        main: path.resolve(__dirname, "index.html"),
        "input-test": path.resolve(__dirname, "input-test.html"),
      },
    },
  },
  server: {
    proxy: {
      "/v1": "http://127.0.0.1:7531",
      "/rpc": "http://127.0.0.1:7531",
      "/admin": "http://127.0.0.1:7531",
    },
  },
});
