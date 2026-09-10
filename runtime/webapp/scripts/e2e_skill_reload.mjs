// 真机浏览器验收(ADR-0033):用 Edge 真实键鼠驱动前端,验证 skill wasm 脚本
// 端到端执行 + /admin/skills 热重载即时生效。
// 前置:boenmind-server 已在 127.0.0.1:7534 起(隔离数据目录 .tools/e2e-data),
// 上游 mock 模型在 127.0.0.1:7654。
import { chromium } from "playwright";
import fs from "node:fs";
import path from "node:path";

const BASE = "http://127.0.0.1:7534/";
const SHOTS = path.resolve("../../milestones/shots-skill-reload-20260911");
fs.mkdirSync(SHOTS, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log("[e2e]", ...a);

async function apiCapabilities(page) {
  return page.evaluate(async () => {
    const r = await fetch("/admin/capabilities");
    const d = await r.json();
    return (d.builtin || []).map((c) => c.name);
  });
}

// content-visibility 虚拟化会让 innerText 为空(PITFALLS #6)——读 DOM textContent。
async function msgText(page) {
  return page.evaluate(() =>
    [...document.querySelectorAll(".msg")].map((m) => m.textContent || "").join("\n"),
  );
}

// 审计里 skill 成功调用的事件(功能证据:能力是否真在运行期注册表且可执行)。
function skillOkEvents() {
  const p = path.resolve("../../.tools/e2e-data/events.jsonl");
  if (!fs.existsSync(p)) return [];
  return fs
    .readFileSync(p, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l))
    .filter(
      (e) =>
        e.type === "capability.invoked" &&
        e.payload?.capability === "skill.skill_demo.echo" &&
        e.payload?.outcome === "ok",
    );
}

async function main() {
  const browser = await chromium.launch({ channel: "msedge", headless: true });
  const ctx = await browser.newContext({ viewport: { width: 1360, height: 900 } });
  const page = await ctx.newPage();

  const pageErrors = [];
  page.on("pageerror", (e) => pageErrors.push(String(e)));
  const consoleErrors = [];
  page.on("console", (m) => {
    if (m.type() === "error") consoleErrors.push(m.text());
  });

  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  // yolo:服务端放行审批(skill 为 read-only 直通,双保险)
  await page.evaluate(() => localStorage.setItem("bm_permission_mode", "yolo"));
  await page.reload({ waitUntil: "domcontentloaded" });

  // 0) 启动即装载:skill 能力在发现面
  const caps0 = await apiCapabilities(page);
  log("startup skill caps:", caps0.filter((n) => n.startsWith("skill.")));
  if (!caps0.includes("skill.skill_demo.echo")) {
    throw new Error("启动后 skill 能力未注册:" + JSON.stringify(caps0));
  }

  // 1) 真实键鼠对话:sends a message, model returns tool_call to skill, kernel
  //    executes wasm, result feeds back.
  await page.waitForSelector("textarea, [contenteditable], input", { timeout: 15000 });
  const composer = page.getByPlaceholder("Message BoenMind…");
  await composer.click();
  await composer.fill("请调用技能 echo 做一次回声测试");
  await page.locator(".send-btn").click();
  log("message sent");

  // 轮询:等助手消息出现工具调用标记(skill.skill_demo.echo)与终稿文本
  // 轮询:等助手消息出现终稿文本
  const deadline = Date.now() + 60000;
  let sawFinal = false;
  while (Date.now() < deadline) {
    const body = await msgText(page);
    if (body.includes("技能执行完成")) {
      sawFinal = true;
      break;
    }
    await sleep(500);
  }
  if (!sawFinal) throw new Error("未收到助手终稿(回合可能未完成)");

  // 展开工具聚合组(真实点击):子项应显示 skill.skill_demo.echo
  const head = page.locator(".msg.assistant [data-slot='generic-tool-head']").last();
  await head.click();
  await sleep(400);
  const assistantText = await page
    .locator(".msg.assistant")
    .last()
    .evaluate((el) => el.textContent || "");
  const sawTool = assistantText.includes("skill.skill_demo.echo");
  await page.screenshot({ path: path.join(SHOTS, "01-chat-skill-exec.png"), fullPage: true });
  log("sawToolMarker=", sawTool, "sawFinal=", sawFinal);
  if (!sawTool) throw new Error("前端未渲染 skill 工具调用标记(展开后)");
  const finalCount = (assistantText.match(/技能执行完成/g) || []).length;
  if (finalCount !== 1) {
    throw new Error(`终稿应恰出现一次(实际 ${finalCount} 次)——补发去重回归`);
  }
  log("assistant text:", assistantText.replace(/\s+/g, " ").slice(0, 200));

  // 结果内容核对:审计事件应含 skill.skill_demo.echo 的 capability.invoked(ok)
  const events = fs
    .readFileSync(path.resolve("../../.tools/e2e-data/events.jsonl"), "utf8")
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l));
  const invoked = events.filter(
    (e) => e.type === "capability.invoked" && e.payload?.capability === "skill.skill_demo.echo",
  );
  log("capability.invoked(skill) count:", invoked.length, "outcome:", invoked.at(-1)?.payload?.outcome);
  if (!invoked.length || invoked.at(-1).payload.outcome !== "ok") {
    throw new Error("审计未见 skill 调用成功记录");
  }

  // 2) 设置→角色:技能库 UI 可见 skill_demo(先留一份带 scripts 的定义备重载用)
  const skillPayload = JSON.parse(
    fs.readFileSync(path.resolve("../../.tools/e2e-data/config/skills.json"), "utf8"),
  ).skills[0];
  await page.locator('[data-slot="open-settings"]').click();
  await page.getByRole("button", { name: /^角色/ }).click();
  await page.waitForSelector('[data-skill-id="skill_demo"]', { timeout: 10000 });
  const skillRow = page.locator('[data-skill-id="skill_demo"]');
  log("skill row visible:", await skillRow.isVisible());
  await page.screenshot({ path: path.join(SHOTS, "02-settings-skill-list.png"), fullPage: true });

  // 3) UI 删除技能 → 热卸载。活体注册表没有静态查询端点(builtin_caps 是启动
  //    快照),故用功能证据:删除后再对话触发同名工具调用,审计里不得再出现
  //    skill 的成功调用(能力已从运行期注册表摘除)。
  const okBefore = skillOkEvents();
  page.on("dialog", (d) => d.accept()); // window.confirm
  await skillRow.locator('button[title="删除技能"]').click();
  await sleep(1200);
  const fileAfterDelete = JSON.parse(
    fs.readFileSync(path.resolve("../../.tools/e2e-data/config/skills.json"), "utf8"),
  ).skills.map((s) => s.skill_id);
  log("skills.json after UI delete:", fileAfterDelete);
  if (fileAfterDelete.includes("skill_demo")) throw new Error("UI 删除未落盘");
  await page.screenshot({ path: path.join(SHOTS, "03-after-ui-delete.png"), fullPage: true });

  await page.keyboard.press("Escape");
  await sleep(400);
  await page.locator('[data-slot="new-chat"]').click().catch(() => {});
  await sleep(600);
  await page.getByPlaceholder("Message BoenMind…").fill("再调用一次技能 echo");
  await page.locator(".send-btn").click();
  // 等回合收敛(终稿或失败标记均可)
  const d3 = Date.now() + 45000;
  while (Date.now() < d3) {
    const t = await msgText(page);
    if (t.includes("失败") || t.includes("技能执行完成") || t.includes("权限") || t.includes("不存在")) break;
    await sleep(500);
  }
  await sleep(800);
  const okAfterDelete = skillOkEvents();
  log("skill ok-events before delete:", okBefore.length, "after delete:", okAfterDelete.length);
  if (okAfterDelete.length !== okBefore.length) {
    throw new Error("UI 删除后 skill 仍被成功调用(热卸载未生效)");
  }
  await page.screenshot({ path: path.join(SHOTS, "04-after-delete-call-denied.png"), fullPage: true });

  // 4) 管理面热重载新增:UI 表单不暴露 scripts,脚本技能走同一 POST 端点;
  //    断言能力无需重启即回到运行期注册表并真正可执行。
  const postResp = await page.evaluate(async (payload) => {
    const r = await fetch("/admin/skills", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    return { status: r.status, body: await r.json() };
  }, skillPayload);
  log("POST /admin/skills ->", postResp.status, postResp.body.note);
  if (!/热重载/.test(postResp.body.note || "")) {
    throw new Error("POST 未触发脚本热重载: " + JSON.stringify(postResp.body));
  }

  await sleep(500);
  await page.keyboard.press("Escape");
  await sleep(300);
  await page.locator('[data-slot="new-chat"]').click().catch(() => {});
  await sleep(600);
  await page.getByPlaceholder("Message BoenMind…").fill("热重载后再调用技能 echo");
  await page.locator(".send-btn").click();
  const deadline2 = Date.now() + 60000;
  let ok2 = false;
  while (Date.now() < deadline2) {
    const body = await msgText(page);
    if (body.includes("技能执行完成")) { ok2 = true; break; }
    await sleep(500);
  }
  await page.screenshot({ path: path.join(SHOTS, "05-chat-after-reload.png"), fullPage: true });
  if (!ok2) throw new Error("热重载后 skill 调用失败");
  const okAfterReload = skillOkEvents();
  log("skill ok-events after reload:", okAfterReload.length);
  if (okAfterReload.length <= okAfterDelete.length) {
    throw new Error("热重载后 skill 未真正可执行");
  }
  log("post-reload chat ok:", ok2);

  await browser.close();

  log("pageErrors:", pageErrors.length ? pageErrors : "(none)");
  const realConsoleErrors = consoleErrors.filter(
    (t) => !/Failed to load resource|400|404|500|net::ERR/.test(t),
  );
  log("consoleErrors(non-resource):", realConsoleErrors.length ? realConsoleErrors : "(none)");
  if (pageErrors.length) throw new Error("页面存在 JS 异常: " + pageErrors.join(" | "));
  console.log("\nE2E SKILL RELOAD: ALL CHECKS PASSED");
}

main().catch((e) => {
  console.error("E2E FAILED:", e.message);
  process.exit(1);
});
