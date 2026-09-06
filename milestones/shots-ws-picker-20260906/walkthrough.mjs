// 工作目录选择器真浏览器走查(硬纪律 7:用户可见面必须真实浏览器手测)。
// 直连本机真实 server(127.0.0.1:7531,新二进制+新 dist),零 mock;
// 收尾把演示条目删掉,注册表恢复原状。截图存本目录。
import { createRequire } from "module";

const { chromium } = createRequire(
  "D:/96_CoderWorld/BoenMind/runtime/webapp/package.json",
)("@playwright/test");

const BASE = "http://127.0.0.1:7531";
const SHOTS = new URL(".", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const shot = (page, name) =>
  page.screenshot({ path: `${SHOTS}${name}`, fullPage: false });

const errors = [];
const run = async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(`console: ${m.text()}`);
  });

  await page.goto(BASE);
  await page.locator('[data-slot="open-settings"]').click();
  await page.getByRole("button", { name: /^常规/ }).click();
  await page
    .locator('[data-slot="workspace-add"]')
    .waitFor({ state: "visible", timeout: 10_000 });
  await shot(page, "00_settings_general.png");

  // 1. 打开选择器 → 根视图(盘符)
  await page.locator('[data-slot="workspace-add"]').click();
  await page
    .locator('[data-slot="ws-picker-dir"]', { hasText: "D:\\" })
    .waitFor({ timeout: 10_000 });
  await shot(page, "01_dialog_root_drives.png");
  console.log("STEP1 根视图盘符: OK");

  // 2. 点 D:\ 进入 → 应看到 96_CoderWorld
  await page.locator('[data-slot="ws-picker-dir"]', { hasText: "D:\\" }).click();
  await page
    .locator('[data-slot="ws-picker-dir"]', { hasText: "96_CoderWorld" })
    .waitFor({ timeout: 10_000 });
  const nameVal1 = await page.locator("#ws-name").inputValue();
  console.log(`STEP2 进入 D:\\,名称自动跟随 = "${nameVal1}"(盘符根→"D:",合理)`);
  await shot(page, "02_drive_d.png");

  // 2.5 进入 96_CoderWorld(中间级)
  await page
    .locator('[data-slot="ws-picker-dir"]', { hasText: "96_CoderWorld" })
    .click();
  await page
    .locator('[data-slot="ws-picker-dir"][title="D:\\\\96_CoderWorld\\\\BoenMind"]')
    .waitFor({ timeout: 10_000 });

  // 3. 进入 BoenMind(已登记)→ 徽标 + 保存禁用
  await page
    .locator('[data-slot="ws-picker-dir"][title="D:\\\\96_CoderWorld\\\\BoenMind"]')
    .click();
  await page
    .locator('[data-slot="workspace-dialog"]')
    .getByText("已登记")
    .waitFor({ timeout: 10_000 });
  const saveDisabled = await page.locator('[data-slot="workspace-save"]').isDisabled();
  console.log(`STEP3 已登记徽标出现,保存禁用 = ${saveDisabled}(应 true)`);
  await shot(page, "03_registered_badge.png");

  // 4. 顶栏手输路径 + Enter 跳转 → 名称自动跟随
  await page.locator('[data-slot="ws-picker-input"]').fill("D:\\96_CoderWorld\\boenmind-demo-project");
  await page.locator('[data-slot="ws-picker-input"]').press("Enter");
  await page
    .waitForFunction(
      () => document.querySelector("#ws-name")?.value === "boenmind-demo-project",
      null,
      { timeout: 10_000 },
    )
    .catch(() => {
      throw new Error("跳转后名称未自动跟随 boenmind-demo-project");
    });
  await shot(page, "04_jump_and_autoname.png");
  console.log("STEP4 手输路径跳转+名称自动跟随: OK");

  // 5. 左栏点上级(96_CoderWorld)→ 输入框回跳
  await page
    .locator('[data-slot="ws-picker-ancestor"]', { hasText: "96_CoderWorld" })
    .click();
  await page
    .waitForFunction(
      () =>
        document.querySelector('[data-slot="ws-picker-input"]')?.value ===
        "D:\\96_CoderWorld",
      null,
      { timeout: 10_000 },
    )
    .catch(() => {
      throw new Error("左栏上级点击后未回跳 D:\\96_CoderWorld");
    });
  await shot(page, "05_ancestor_jump.png");
  console.log("STEP5 左栏上级回跳: OK");

  // 6. 跳回演示目录并保存 → 列表出现新条目
  await page.locator('[data-slot="ws-picker-input"]').fill("D:\\96_CoderWorld\\boenmind-demo-project");
  await page.locator('[data-slot="ws-picker-jump"]').click();
  await page.waitForFunction(
    () => document.querySelector("#ws-name")?.value === "boenmind-demo-project",
    null,
    { timeout: 10_000 },
  );
  // 改个短名,便于与目录名区分
  await page.locator("#ws-name").fill("选择器演示");
  await page.locator('[data-slot="workspace-save"]').click();
  await page
    .locator('[data-slot="workspace-row"]', { hasText: "选择器演示" })
    .waitFor({ timeout: 10_000 });
  await shot(page, "06_saved_in_list.png");
  console.log("STEP6 保存落列表: OK");

  // 7. 编辑模式:打开「演示项目」(D:\96_CoderWorld\BoenMind)→ 预填路径与名称
  await page
    .locator('[data-slot="workspace-row"]', { hasText: "演示项目" })
    .locator('[data-slot="workspace-edit"]')
    .click();
  await page
    .waitForFunction(
      () =>
        document.querySelector("#ws-name")?.value === "演示项目" &&
        document.querySelector('[data-slot="ws-picker-input"]')?.value ===
          "D:\\96_CoderWorld\\BoenMind",
      null,
      { timeout: 10_000 },
    )
    .catch(() => {
      throw new Error("编辑模式未正确预填名称/路径");
    });
  await shot(page, "07_edit_mode.png");
  console.log("STEP7 编辑模式预填: OK");
  await page
    .locator('[data-slot="workspace-dialog"]')
    .getByRole("button", { name: "取消" })
    .click();

  // 8. 清理:删除演示条目,注册表恢复原状
  await page
    .locator('[data-slot="workspace-row"]', { hasText: "选择器演示" })
    .locator('[data-slot="workspace-delete"]')
    .click();
  await page
    .locator('[data-slot="workspace-row"]', { hasText: "选择器演示" })
    .waitFor({ state: "detached", timeout: 10_000 });
  await shot(page, "08_cleanup.png");
  console.log("STEP8 清理演示条目: OK");

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
