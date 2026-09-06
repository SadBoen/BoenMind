// 插件管控批真浏览器走查(ADR-0023):批准即上线/卸载即下线/purge 物理删除/
// 默认安装(启动播种已验证)/弃用标记/墓碑批准恢复。直连本机真实 server。
import { createRequire } from "module";
import fs from "fs";

const { chromium } = createRequire(
  "D:/96_CoderWorld/BoenMind/runtime/webapp/package.json",
)("@playwright/test");

const BASE = "http://127.0.0.1:7531";
const SHOTS = "D:/96_CoderWorld/BoenMind/milestones/shots-mcp-control-20260906/";
const shot = (page, name) => page.screenshot({ path: `${SHOTS}${name}` });
const FAKE_CMD = "C:/Users/Boen/AppData/Roaming/boenmind/mcp/fake-purge.cmd";

const errors = [];
const run = async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(`console: ${m.text()}`);
  });
  // 卸载走原生 confirm:自动接受
  page.on("dialog", (d) => d.accept());

  await page.goto(BASE);
  await page.locator('[data-slot="open-settings"]').click();
  await page.getByRole("button", { name: "插件", exact: true }).click();
  await page
    .getByText("插件与能力中心")
    .waitFor({ timeout: 10_000 });

  // 1. 总览:启动播种的 web_multisearch 在役(绿)、retired_demo 带「已不随包」徽标、
  //    操作列=编辑/配置/卸载/删除(无「测试」)
  const msRow = page.locator("tr", { hasText: "web_multisearch" });
  await msRow.waitFor({ timeout: 15_000 });
  const hasTestBtn = await page.getByRole("button", { name: "测试" }).count();
  const retireBadge = await page
    .locator("tr", { hasText: "retired_demo" })
    .getByText("已不随包")
    .count();
  console.log(
    `STEP1 操作列无「测试」按钮=${hasTestBtn === 0};retired_demo 徽标=${retireBadge > 0}`,
  );
  await shot(page, "01_overview_default_install.png");

  // 2. 卸载 web_multisearch → 即时下线(行消失,无需手动热重载)
  await msRow.getByRole("button", { name: "卸载" }).click();
  await msRow.waitFor({ state: "detached", timeout: 15_000 });
  await shot(page, "02_uninstall_instant_offline.png");
  console.log("STEP2 卸载即时下线: OK");

  // 3. 扫描候选:web_multisearch=官方随包+删除名单(批准恢复);fake_purge_plugin
  await page.getByRole("button", { name: /扫描候选/ }).click();
  await page.getByText("插件目录扫描").waitFor({ timeout: 15_000 });
  const reviveBtn = page.getByRole("button", { name: "批准恢复" });
  await reviveBtn.waitFor({ timeout: 15_000 });
  await shot(page, "03_scan_candidates_tombstoned.png");
  console.log("STEP3 扫描:墓碑候选显示「批准恢复」: OK");

  // 4. 先批准假插件(卡片标题 Fake Purge;为 purge 演示造一条在役记录)
  await page
    .locator('[role="dialog"] div.rounded-lg.border', { hasText: "Fake Purge" })
    .first()
    .getByRole("button", { name: "批准接入" })
    .click();
  await page
    .locator("tr", { hasText: "fake_purge_plugin" })
    .waitFor({ timeout: 15_000 });
  console.log("STEP4 假插件批准即上线(灰,握手失败属预期): OK");

  // 5. 删除假插件:警告栏 → 确认 → 物理删除
  await page
    .locator("tr", { hasText: "fake_purge_plugin" })
    .locator('[data-slot="mcp-purge"]')
    .click();
  await page.locator('[data-slot="mcp-purge-dialog"]').waitFor({ timeout: 10_000 });
  await shot(page, "05_purge_warning_dialog.png");
  await page.locator('[data-slot="mcp-purge-confirm"]').click();
  await page
    .locator("tr", { hasText: "fake_purge_plugin" })
    .waitFor({ state: "detached", timeout: 15_000 });
  const fakeGone = !fs.existsSync(FAKE_CMD);
  console.log(`STEP5 purge 物理删除: 原文件已消失=${fakeGone}`);
  await shot(page, "06_purged.png");

  // 6. 扫描 → 批准恢复 web_multisearch → 自动上线+工具数反馈
  await page.getByRole("button", { name: /扫描候选/ }).click();
  await reviveBtn.waitFor({ timeout: 15_000 });
  await reviveBtn.click();
  await page
    .getByText(/已批准并自动上线/)
    .waitFor({ timeout: 20_000 });
  await page
    .locator("tr", { hasText: "web_multisearch" })
    .locator("span.bg-emerald-500")
    .waitFor({ timeout: 15_000 });
  await shot(page, "07_approve_revive_online.png");
  console.log("STEP6 批准恢复→自动上线(绿点亮+工具数反馈): OK");

  await browser.close();
  if (errors.length) {
    console.log("页面错误:\n" + errors.join("\n"));
    process.exit(2);
  }
  console.log("ALL OK,无页面错误");
};

run().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
