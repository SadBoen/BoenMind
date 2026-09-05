import { chromium } from "playwright";
import fs from "node:fs";
import path from "node:path";

async function main() {
  const outDir = "D:/milestones/shots-zcode-style-20260905";
  fs.mkdirSync(outDir, { recursive: true });

  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 750 } });
  const page = await context.newPage();

  console.log("1. 打开升级后的 BoenMind 服务: http://127.0.0.1:8765/ ...");
  await page.goto("http://127.0.0.1:8765/");
  await page.waitForTimeout(1000);

  console.log("2. 开启完全访问 (YOLO 模式) 免弹窗审批...");
  await page.evaluate(() => localStorage.setItem("bm_permission_mode", "yolo"));

  console.log("3. 提出明确的文件搜索任务：'请搜索工作区中有哪些 README 相关的说明文档文件，并列出路径'...");
  const input = page.getByRole("textbox", { name: "Message BoenMind…" });
  await input.fill("请搜索工作区中有哪些 README 相关的说明文档文件，并列出路径。");
  await page.locator(".send-btn").click();

  console.log("4. 消息已发送，等待模型调用 fs.search 执行...");

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

  // 截取对话界面截图
  const shotPath = path.join(outDir, "08_fs_search_files_result.png");
  await page.screenshot({ path: shotPath });
  console.log("已保存文件搜索截图:", shotPath);

  const content = await page.locator(".msg.assistant .content").innerText();
  console.log("=== 助手回复内容 ===\n", content);

  const toolGroup = await page.locator('[data-slot="tool-group"]').count();
  console.log("=== 折叠工具卡片数量 ===:", toolGroup);

  if (toolGroup > 0) {
    await page.locator('[data-slot="tool-group"] .tool-group-header').first().click();
    await page.waitForTimeout(500);
    const shotPath2 = path.join(outDir, "09_fs_search_files_expanded.png");
    await page.screenshot({ path: shotPath2 });
    console.log("已保存展开卡片截图:", shotPath2);
  }

  await browser.close();
  console.log("测试全部结束！");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
