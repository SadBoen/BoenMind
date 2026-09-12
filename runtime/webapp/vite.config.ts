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
      input: path.resolve(__dirname, "index.html"),
    },
  },
  server: {
    proxy: {
      "/v1": "http://127.0.0.1:7531",
      "/rpc": "http://127.0.0.1:7531",
      "/admin": "http://127.0.0.1:7531",
      // 门户认证:/api/portal/* 与 /login 页此前漏代理,
      // npm run dev 下登录必 404 / 回退到主应用(生产由后端直服 dist 不受影响)
      "/api": "http://127.0.0.1:7531",
      "/login": "http://127.0.0.1:7531",
    },
  },
});
