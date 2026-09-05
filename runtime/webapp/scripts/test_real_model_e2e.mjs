import { chromium } from "playwright";
import fs from "node:fs";
import path from "node:path";

async function main() {
  const outDir = "D:/milestones/shots-zcode-style-20260905";
  fs.mkdirSync(outDir, { recursive: true });

  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 750 } });
  const page = await context.newPage();

  console.log("1. 打开真实 BoenMind 服务: http://127.0.0.1:8765/ ...");
  await page.goto("http://127.0.0.1:8765/");
  await page.waitForTimeout(1000);

  console.log("2. 设置权限为 YOLO 免弹窗，让工具全自动放行完成端到端...");
  await page.evaluate(() => localStorage.setItem("bm_permission_mode", "yolo"));

  console.log("3. 发送真实提示词给模型...");
  const input = page.getByRole("textbox", { name: "Message BoenMind…" });
  await input.fill("请帮我使用 fs.search 工具搜索工作区中的 README.md，并告诉我搜索结果。");
  await page.locator(".send-btn").click();
  console.log("4. 消息已发送，等待真实模型回复与工具调用执行...");

  // 轮询等待生成完成（stop 按钮消失且文本产出）
  let finished = false;
  for (let i = 0; i < 40; i++) {
    await page.waitForTimeout(1500);
    const stopBtn = await page.locator(".send-btn.stop").count();
    const assistantMsg = await page.locator(".msg.assistant").count();
    const assistantContent = assistantMsg > 0 ? await page.locator(".msg.assistant .content").innerText() : "";
    console.log(`[t=${(i * 1.5).toFixed(1)}s] 助手消息数=${assistantMsg}, stop按钮=${stopBtn}, 内容长度=${assistantContent.length}`);
    if (assistantMsg > 0 && stopBtn === 0 && i > 3) {
      finished = true;
      break;
    }
  }

  // 截取真实对话界面截图
  const shotPath = path.join(outDir, "real_chat_tool_call.png");
  await page.screenshot({ path: shotPath });
  console.log("已保存真实调用截图:", shotPath);

  // 查看 DOM 内容
  const content = await page.locator(".msg.assistant .content").innerText();
  console.log("=== 助手回复内容 ===\n", content);

  const toolGroup = await page.locator('[data-slot="tool-group"]').count();
  console.log("=== 折叠工具卡片数量 ===:", toolGroup);

  // 如果有工具卡片，展开它并再截一张
  if (toolGroup > 0) {
    await page.locator('[data-slot="tool-group"] .tool-group-header').first().click();
    await page.waitForTimeout(500);
    const shotPath2 = path.join(outDir, "real_chat_tool_expanded.png");
    await page.screenshot({ path: shotPath2 });
    console.log("已保存展开工具截图:", shotPath2);
  }

  // 查看左侧 SESSION 列表
  const sessions = await page.locator('[data-slot="session-item"]').count();
  console.log("=== 左侧会话条目数 ===:", sessions);

  // 4. 点击新建对话「+」号
  console.log("5. 点击「+」新建对话...");
  await page.locator('[data-slot="new-chat"]').click();
  await page.waitForTimeout(800);

  // 检查上下文页
  await page.locator('[data-slot="tab-ctx"]').click();
  await page.waitForTimeout(800);
  const shotPath3 = path.join(outDir, "real_chat_new_context.png");
  await page.screenshot({ path: shotPath3 });
  console.log("已保存新建对话后上下文截图:", shotPath3);

  // 再次切回对话页
  await page.locator('[data-slot="tab-chat"]').click();
  await page.waitForTimeout(500);
  const shotPath4 = path.join(outDir, "real_chat_multi_sessions.png");
  await page.screenshot({ path: shotPath4 });
  console.log("已保存新建对话后对话区与多会话并存截图:", shotPath4);

  await browser.close();
  console.log("真实端到端完整测试与视觉截屏完毕！");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
