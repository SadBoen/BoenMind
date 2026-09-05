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
  await page.route("**/admin/model/active", (route) =>
    route.fulfill({
      json: {
        values: {
          modelId: "mock.model",
          contextWindows: { "mock.model": 128000 },
        },
        secret_set: { apiKey: true },
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
  await page.route("**/admin/skills", (route) =>
    route.fulfill({ json: { ok: true, skills: [] } }),
  );
  // W8:工作区注册表(6 条 = 触发列表滚动)+ 运行环境探针
  await page.route("**/admin/workspaces", (route) =>
    route.fulfill({
      json: {
        workspaces: [
          { id: "default", name: "默认工作区", path: "C:/ws", exists: true, isDefault: true },
          { id: "ws_a", name: "项目甲", path: "D:/proj/a", exists: true, isDefault: false },
          { id: "ws_b", name: "项目乙", path: "D:/proj/b", exists: true, isDefault: false },
          { id: "ws_c", name: "项目丙", path: "D:/proj/c", exists: false, isDefault: false },
          { id: "ws_d", name: "项目丁", path: "D:/proj/d", exists: true, isDefault: false },
          { id: "ws_e", name: "项目戊", path: "D:/proj/e", exists: true, isDefault: false },
        ],
      },
    }),
  );
  // 统一插件中心:外部 MCP 清单与探活
  await page.route("**/admin/mcp", (route) =>
    route.fulfill({
      json: {
        ok: true,
        servers: [
          { name: "demo_server", transport: "stdio", command: "demo", args: [] },
        ],
      },
    }),
  );
  await page.route("**/admin/mcp/status", (route) =>
    route.fulfill({
      json: {
        status: [
          { name: "demo_server", ok: true, tools: 2, tool_list: [], error: null },
        ],
      },
    }),
  );
  await page.route("**/admin/runtime/env", (route) =>
    route.fulfill({
      json: {
        python: {
          installed: true,
          version: "Python 3.13.0",
          program: "python --version",
          error: null,
        },
        node: {
          installed: false,
          version: null,
          program: null,
          error: "未检测到可用的命令",
        },
      },
    }),
  );
}

test.describe("对话闭环", () => {
  test("欢迎页渲染 + 角色下拉 + 发送与流式回复", async ({ page }) => {
    await mockChat(page, "这是流式回复的完整内容。");
    await mockAdmin(page);
    await page.goto("/");
    await expect(page.getByRole("heading", { name: "个人生态的 AI Runtime" })).toBeVisible();
    await expect(page.locator('[data-slot="role-select"]')).toBeVisible();

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
    // 验证第一层看板：模型窗口水位与余量
    await expect(page.getByText("模型窗口水位")).toBeVisible();
    await expect(page.getByText(/剩余安全余量/)).toBeVisible();

    // 验证 DSH 视觉化吸收 (阶段一)：时序演进趋势图与单步速报
    await expect(page.getByText("上下文演进趋势图 (时序堆叠)")).toBeVisible();
    await expect(page.getByText(/📌 单步速报/)).toBeVisible();
    await expect(page.getByText("【本轮】用户原始提问")).toBeVisible();

    // 验证第二层配方拆解：人设与根本规矩双栏 (左卡片与右原文均存在)
    await expect(page.getByText("🎭 AI 的人设与根本规矩")).toBeVisible();
    await expect(page.getByText("系统提示词内容").first()).toBeVisible();
    // 切换至工具背包双栏 Tab
    await page.getByRole("tab", { name: /^工具背包/ }).click();
    await expect(page.getByText("🛠️ 随身装备的工具箱 (双栏联动透视)")).toBeVisible();
    await expect(page.getByText("demo.tool").first()).toBeVisible();
    // 验证右侧专家模式联动代码块存在
    await expect(page.getByText("专家模式：OpenAI Function JSON 定义")).toBeVisible();
    // 切换至全局专家模式 (Raw)
    await page.getByTitle("切换查看全部发给模型的原始 JSON 报文").click();
    await expect(page.getByText("底层完整请求报文 (OpenAI API 格式)")).toBeVisible();
  });
});

test.describe("设置中心", () => {
  test("角色页:多角色列表与默认徽标", async ({ page }) => {
    await mockAdmin(page);
    await page.goto("/");
    await page.locator('[data-slot="open-settings"]').evaluate((el: HTMLElement) => el.click());
    // 导航项可访问名含 W4 徽标文本,用前缀匹配
    await page.getByRole("button", { name: /^角色/ }).click();
    await expect(page.getByRole("heading", { name: "角色与技能" })).toBeVisible();
    await expect(page.getByText("通用助理")).toBeVisible();
    await expect(page.getByText("代码专家")).toBeVisible();
    await expect(page.getByText("全局默认")).toBeVisible();
  });

  test("插件页:统一插件中心(内置能力 + 外部插件同页)", async ({ page }) => {
    await mockAdmin(page);
    await page.goto("/");
    await page.locator('[data-slot="open-settings"]').evaluate((el: HTMLElement) => el.click());
    await page.getByRole("button", { name: "插件", exact: true }).click();
    await expect(page.getByRole("heading", { name: "插件" })).toBeVisible();
    // 内置能力列出(名称列 + 工具列各出现一次,取 first)
    await expect(page.getByText("system.echo").first()).toBeVisible();
    // 统一插件中心:外部 MCP 服务器同页呈现,不再与内置割裂
    await expect(page.getByText("demo_server").first()).toBeVisible();
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

// ---- W8:常规设置(工作区 + 环境探针)、composer 工作区选择、空气泡修复 ----

test.describe("常规设置页(W8)", () => {
  test("导航、环境探针卡与工作目录五行滚动列表", async ({ page }) => {
    await mockAdmin(page);
    await page.goto("/");
    await page.locator('[data-slot="open-settings"]').evaluate((el: HTMLElement) => el.click());
    await page.getByRole("button", { name: /^常规/ }).click();
    await expect(page.getByRole("heading", { name: "常规" })).toBeVisible();

    // 环境探针:Python 已安装(带版本),Node 未检测到
    const py = page.locator('[data-slot="runtime-tool"][data-tool="Python"]');
    await expect(py).toContainText("已安装");
    await expect(py).toContainText("Python 3.13.0");
    const nd = page.locator('[data-slot="runtime-tool"][data-tool="Node.js"]');
    await expect(nd).toContainText("未检测到");

    // 工作目录列表:6 条目、固定五行高度、可滚动
    const list = page.locator('[data-slot="workspace-list"]');
    await expect(list.locator('[data-slot="workspace-row"]')).toHaveCount(6);
    const box = await list.boundingBox();
    expect(box).toBeTruthy();
    expect(Math.round(box!.height)).toBe(285); // 5 行 × 57px
    const scrollable = await list.evaluate(
      (el) => el.scrollHeight > el.clientHeight,
    );
    expect(scrollable).toBe(true);
    await expect(page.locator('[data-slot="workspace-row"][data-id="default"]')).toContainText("默认");
  });
});

test.describe("composer 工作目录选择(W8)", () => {
  test("选择后请求携带 workspace,默认不携带", async ({ page }) => {
    await mockAdmin(page);
    const bodies: Array<Record<string, unknown>> = [];
    await page.route("**/v1/chat/completions", async (route) => {
      bodies.push(route.request().postDataJSON() as Record<string, unknown>);
      await route.fulfill({
        status: 200,
        headers: {
          "Content-Type": "text/event-stream",
          "x-bm-session": "sess_00000000000000000000000e2e",
        },
        body: [
          `data: ${JSON.stringify({ choices: [{ delta: { content: "目录回复。" } }] })}`,
          "",
          "data: [DONE]",
          "",
        ].join("\n"),
      });
    });
    await page.goto("/");
    const input = page.getByRole("textbox", { name: "Message BoenMind…" });

    // 第一条:未选择工作区 → 请求不带 workspace 字段
    await input.fill("第一条");
    await page.locator(".send-btn").evaluate((el: HTMLElement) => el.click());
    await expect(page.getByText("目录回复。")).toBeVisible({ timeout: 8_000 });
    expect(bodies[0].workspace).toBeUndefined();

    // 打开上拉菜单,选「项目甲」
    await page.locator('[data-slot="workspace-select"]').evaluate((el: HTMLElement) => el.click());
    await page.getByRole("option", { name: /项目甲/ }).click();

    // 第二条:请求携带 workspace id
    await input.fill("第二条");
    await page.locator(".send-btn").evaluate((el: HTMLElement) => el.click());
    await expect(page.getByText("目录回复。").nth(1)).toBeVisible({ timeout: 8_000 });
    expect(bodies[1].workspace).toBe("ws_a");
  });
});

test.describe("空气泡修复(W8)", () => {
  test("空正文回复不出现空气泡,tag 仍在", async ({ page }) => {
    await mockAdmin(page);
    await page.route("**/v1/chat/completions", (route) =>
      route.fulfill({
        status: 200,
        headers: {
          "Content-Type": "text/event-stream",
          "x-bm-session": "sess_00000000000000000000000e2e",
        },
        body: [
          `data: ${JSON.stringify({ choices: [{ delta: { role: "assistant", content: "" } }] })}`,
          "",
          `data: ${JSON.stringify({ choices: [{ delta: {}, finish_reason: "stop" }] })}`,
          "",
          "data: [DONE]",
          "",
        ].join("\n"),
      }),
    );
    await page.goto("/");
    const input = page.getByRole("textbox", { name: "Message BoenMind…" });
    await input.fill("空回复测试");
    await page.locator(".send-btn").evaluate((el: HTMLElement) => el.click());
    // 用户消息在,助手 tag 在,但空气泡被隐藏
    await expect(page.getByText("空回复测试")).toBeVisible();
    await expect(page.getByText("BoenMind Agent").first()).toBeVisible();
    await expect(page.locator(".msg.assistant .text").first()).toBeHidden();
  });
});
