// 临时本地配置(不入库):CDN 装不上 chromium 时用本机 Edge 跑 smoke。
// 用法:npx playwright test --config playwright.smoke.local.config.ts
import { defineConfig } from "@playwright/test";
import base from "./playwright.config";

export default defineConfig({
  ...base,
  use: {
    // @ts-expect-error 展开自基线 config 的 use
    ...base.use,
    channel: "msedge",
  },
});
