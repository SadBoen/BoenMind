import { chromium } from "playwright";
import fs from "node:fs";
import path from "node:path";

async function main() {
  const outDir = path.resolve("../../milestones/shots-zcode-style-20260905");
  fs.mkdirSync(outDir, { recursive: true });

  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 750 } });
  const page = await context.newPage();

  console.log("1. 访问首页...");
  await page.goto("http://127.0.0.1:8765/");
  await page.waitForTimeout(1000);

  // 截屏 1: 首页新会话列表与扁平无气泡欢迎区
  const shot1 = path.join(outDir, "01_new_session_panel.png");
  await page.screenshot({ path: shot1 });
  console.log("已保存截图 1:", shot1);

  // 2. 切换到「上下文」透视页
  console.log("2. 切换到「上下文」透视页...");
  await page.locator('[data-slot="tab-ctx"]').click();
  await page.waitForTimeout(1000);
  const shot2 = path.join(outDir, "02_context_clean_empty.png");
  await page.screenshot({ path: shot2 });
  console.log("已保存截图 2 (验证当前未发消息时上下文历史不穿透):", shot2);

  // 3. 点击左上角「新建对话」+ 号
  console.log("3. 点击新建对话「+」号...");
  await page.locator('[data-slot="new-chat"]').click();
  await page.waitForTimeout(1000);
  const shot3 = path.join(outDir, "03_click_plus_switch_back.png");
  await page.screenshot({ path: shot3 });
  console.log("已保存截图 3 (验证点击 + 自动切回对话页且聚焦输入框):", shot3);

  // 4. 测试流式问答与 ZCode 风格折叠与无气泡展示
  console.log("4. 测试工具折叠与无气泡展示...");
  // 我们通过 mock 一次回合展示工具折叠与无气泡
  await page.route("**/v1/chat/completions", async (route) => {
    await route.fulfill({
      status: 200,
      headers: {
        "Content-Type": "text/event-stream",
        "x-bm-session": "sess_zcode_demo_01",
      },
      body: [
        `data: ${JSON.stringify({ choices: [{ delta: { content: "好的，我开始检查项目文件：\n[调用 fs_search runtime/webapp]\n[调用 fs_read runtime/webapp/package.json]\n[调用 system_exec git status]\n已完成检索，项目状态正常。" } }] })}`,
        "",
        "data: [DONE]",
        "",
      ].join("\n"),
    });
  });

  const input = page.getByRole("textbox", { name: "Message BoenMind…" });
  await input.fill("请帮我查阅并读取 package.json");
  await page.locator(".send-btn").click();
  await page.waitForTimeout(1500);

  const shot4 = path.join(outDir, "04_zcode_style_chat_and_tools.png");
  await page.screenshot({ path: shot4 });
  console.log("已保存截图 4 (无气泡平铺对话 + 查阅折叠卡片):", shot4);

  // 5. 展开折叠工具卡片
  console.log("5. 展开工具折叠卡片...");
  await page.locator('[data-slot="tool-group"] .tool-group-header').click();
  await page.waitForTimeout(500);

  const shot5 = path.join(outDir, "05_tool_group_expanded.png");
  await page.screenshot({ path: shot5 });
  console.log("已保存截图 5 (展开折叠卡片，专属终端/文件格式图标):", shot5);

  // 6. 再次点击「+」新建对话，验证多会话列表产生
  console.log("6. 再次新建对话并生成第二条会话...");
  await page.locator('[data-slot="new-chat"]').click();
  await page.waitForTimeout(800);

  const shot6 = path.join(outDir, "06_multi_session_list.png");
  await page.screenshot({ path: shot6 });
  console.log("已保存截图 6 (多会话列表与新建就绪卡片并存):", shot6);

  await browser.close();
  console.log("所有端到端仿真测试与截屏完成！");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
