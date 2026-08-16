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
        target: process.env.BM_API_TARGET || "http://127.0.0.1:17321",
        changeOrigin: true,
      },
    },
  },
  build: {
    rolldownOptions: {
      output: {
        // 大体积依赖拆独立 chunk：品牌图标库（@lobehub/icons 全家桶）与
        // markdown 渲染链只在相关页面用到，拆出后主 chunk 显著减小、
        // 浏览器可独立缓存（rolldown codeSplitting）
        codeSplitting: {
          groups: [
            { name: "vendor-icons", test: /node_modules[\\/]@lobehub[\\/]icons/ },
            {
              name: "vendor-markdown",
              test: /node_modules[\\/](react-markdown|remark-gfm|rehype-highlight|remark-parse|unified|hast-|mdast-|micromark)/,
            },
          ],
        },
      },
    },
  },
});
