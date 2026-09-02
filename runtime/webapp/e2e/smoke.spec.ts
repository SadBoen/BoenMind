// BoenMind 前端冒烟测试:核心交互回归(对话闭环/角色切换/上下文页/设置页)。
// 模型流与 /admin 数据经 page.route mock,确定性断言,不依赖真实模型。
import { test, expect, type Page } from "@playwright/test";

/** 拦截 /v1 流式对话:返回两段 mock 增量后 DONE。 */
async function mockChat(page: Page, reply: string) {
  await page.route("**/v1/chat/completions", (route) =>
    route.fulfill({
      status: 200,
      headers: {
        "Content-Type": "text/event-stream",
        "x-bm-session": "sess_00000000000000000000000e2e",
      },
      body: [
        `data: ${JSON.stringify({ choices: [{ delta: { content: reply.slice(0, 3) } }] })}`,
        "",
        `data: ${JSON.stringify({ choices: [{ delta: { content: reply.slice(3) } }] })}`,
        "",
        "data: [DONE]",
        "",
      ].join("\n"),
    }),
  );
}

/** 拦截 /admin 只读面:角色/能力/上下文快照返回固定数据。 */
async function mockAdmin(page: Page) {
  await page.route("**/admin/roles", (route) =>
    route.fulfill({
      json: {
        ok: true,
        active_id: "assistant",
        roles: [
          { id: "assistant", name: "通用助理", system_prompt: "你是助理" },
          { id: "coder", name: "代码专家", system_prompt: "你是专家" },
        ],
      },
    }),
  );
  await page.route("**/admin/capabilities", (route) =>
    route.fulfill({
      json: {
        builtin: [
          { name: "system.echo", effect: "read-only", idempotent: true },
        ],
        mcp: [{ name: "demo_server", tools: 2, loaded: true }],
      },
    }),
  );
  await page.route("**/admin/context", (route) =>
    route.fulfill({
      json: {
        ok: true,
        steps: [
          {
            seq: 1,
            ts: "2026-09-02T10:00:00Z",
            session_id: "sess_e2e",
            agent_id: "agent_e2e",
            operation_id: "op_e2e",
            turn_index: 1,
            step: 1,
            attempt: 1,
            model_id: "mock.model",
            streaming: true,
            messages: [
              { role: "system", content: "系统提示词内容", content_truncated: false },
              { role: "user", content: "用户提问", content_truncated: false },
            ],
            tools: [{ function: { name: "demo.tool" } }],
            status: "ok",
            error_code: null,
            tokens_in: 120,
            tokens_out: 30,
            latency_ms: 1500,
          },
        ],
      },
    }),
  );
  await page.route("**/admin/logs", (route) =>
    route.fulfill({ json: { ok: true, exec: [], events: [], context: [] } }),
  );
}

test.describe("对话闭环", () => {
  test("欢迎页渲染 + 角色下拉 + 发送与流式回复", async ({ page }) => {
    await mockChat(page, "这是流式回复的完整内容。");
    await mockAdmin(page);
    await page.goto("/");
    await expect(page.getByRole("heading", { name: "个人生态的 AI Runtime" })).toBeVisible();
    await expect(page.getByRole("combobox", { name: "切换当前会话角色" })).toBeVisible();
    await expect(page.getByRole("option", { name: "通用助理" })).toBeAttached();

    const input = page.getByRole("textbox", { name: "Message BoenMind…" });
    await input.fill("你好");
    await page.locator(".send-btn").evaluate((el: HTMLElement) => el.click());
    await expect(page.getByText("这是流式回复的完整内容。")).toBeVisible({ timeout: 8_000 });
  });

  test("新建对话清空聊天视图", async ({ page }) => {
    await mockChat(page, "回复内容。");
    await mockAdmin(page);
    await page.goto("/");
    const input = page.getByRole("textbox", { name: "Message BoenMind…" });
    await input.fill("第一句");
    await page.locator(".send-btn").evaluate((el: HTMLElement) => el.click());
    await expect(page.getByText("回复内容。")).toBeVisible({ timeout: 8_000 });

    await page.locator('[data-slot="new-chat"]').evaluate((el: HTMLElement) => el.click());
    await expect(page.getByRole("heading", { name: "个人生态的 AI Runtime" })).toBeVisible();
  });
});

test.describe("上下文透视页", () => {
  test("组成条与步骤明细渲染", async ({ page }) => {
    await mockAdmin(page);
    await page.goto("/");
    await page.locator('[data-slot="tab-ctx"]').evaluate((el: HTMLElement) => el.click());
    await expect(page.getByText("当前上下文(下一次请求同款)")).toBeVisible();
    await expect(page.getByText("实际输入 120 / 输出 30")).toBeVisible();
    // 展开步骤 → 消息逐项浏览器 + 工具定义清单
    await page.locator('[data-slot="ctx-step-head"]').first().evaluate((el: HTMLElement) => el.click());
    await expect(page.getByText("请求组成(即模型实际收到的内容)")).toBeVisible();
    await expect(page.getByText("工具定义 × 1")).toBeVisible();
    // 展开第一条消息的 details 后断言正文可见
    await page
      .locator('[data-slot="ctx-steps"] details summary')
      .first()
      .evaluate((el: HTMLElement) => el.click());
    await expect(page.getByText("系统提示词内容")).toBeVisible();
  });
});

test.describe("设置中心", () => {
  test("角色页:多角色列表与默认徽标", async ({ page }) => {
    await mockAdmin(page);
    await page.goto("/");
    await page.locator('[data-slot="open-settings"]').evaluate((el: HTMLElement) => el.click());
    // 导航项可访问名含 W4 徽标文本,用前缀匹配
    await page.getByRole("button", { name: /^角色/ }).click();
    await expect(page.getByRole("heading", { name: "角色管理" })).toBeVisible();
    await expect(page.getByText("通用助理")).toBeVisible();
    await expect(page.getByText("代码专家")).toBeVisible();
    await expect(page.getByText("全局默认")).toBeVisible();
  });

  test("插件页:仅系统内置(MCP 不重复展示)", async ({ page }) => {
    await mockAdmin(page);
    await page.goto("/");
    await page.locator('[data-slot="open-settings"]').evaluate((el: HTMLElement) => el.click());
    await page.getByRole("button", { name: "插件", exact: true }).click();
    await expect(page.getByRole("heading", { name: "插件" })).toBeVisible();
    await expect(page.getByText("system.echo")).toBeVisible();
    // MCP 服务器不在此页(统一归 MCP 管理)
    await expect(page.getByText("demo_server")).toHaveCount(0);
  });

  test("日志页:三个页签存在", async ({ page }) => {
    await mockAdmin(page);
    await page.goto("/");
    await page.locator('[data-slot="open-settings"]').evaluate((el: HTMLElement) => el.click());
    await page.getByRole("button", { name: "日志" }).click();
    await expect(page.locator('[data-slot="logs-tab"][data-tab="exec"]')).toBeAttached();
    await expect(page.locator('[data-slot="logs-tab"][data-tab="events"]')).toBeAttached();
    await expect(page.locator('[data-slot="logs-tab"][data-tab="ctx"]')).toBeAttached();
  });
});
