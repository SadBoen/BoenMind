import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// dev 用 proxy 指向 web-server（3080），build 产物由 web-server 直接静态服务。
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:3080",
      "/ws": "http://127.0.0.1:3080",
    },
  },
  build: {
    outDir: "dist",
  },
});
