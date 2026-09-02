// BoenMind webapp 前端冒烟测试(Playwright)。
// 策略:真实静态页 + 拦截 /v1 与 /admin 网络响应 mock 数据——确定性、
// 零模型 token、不依赖后端进程(page.route 拦截优先于真实网络)。
// 运行:npm run test:smoke(需先 npm run build;webServer 复用 dist)。
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: "http://127.0.0.1:4173",
    viewport: { width: 1280, height: 720 },
  },
  webServer: {
    command: "npx vite preview --port 4173 --strictPort --host 127.0.0.1",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: true,
    timeout: 30_000,
  },
  reporter: "list",
});
