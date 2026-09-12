// W2 管理面 REST 客户端(壳子私用;/admin/* 见后端 webadmin.rs 模块注释:
// 暂不入冻结合同,行为规格 = bm-surface-http tests/webadmin_tests.rs)。
// 错误形状统一 {error:{message}};探针/连通类结果走 200 + ok 布尔。
//
// 类型漂移锚定(issue #18 裁决:先夹具后生成):本文件类型为手写,与
// 后端实际响应的顶层字段集由 webadmin_tests.rs 的
// t_admin_response_shape_anchors 夹具锁定——后端改任何顶层字段,CI 先红;
// 同步顺序 = 后端有意变更 → 更新夹具期望 → 改本文件类型。
// 后续路线:管理面稳定入册后再评估 Rust 类型自动生成。

// W10(ADR-0024/0025):运行时限制配置 + 后台作业
export type LimitKey = {
  key: string;
  group: string;
  label: string;
  min: number;
  max: number;
  editable: boolean;
  value: number;
  default: number;
  source: "default" | "file" | "env";
};

/** 会话权限模式(ADR-0030):状态权威在服务端会话,前端仅为选择器 */
export type PermissionMode = "ask" | "plan" | "yolo";

/**
 * 待裁决审批单(GET /admin/approvals;后端 serialized `Approval`,顶层夹具
 * 锚定 webadmin_tests.rs::t_admin_response_shape_anchors)。前端只用
 * approval_id/capability/args_summary 三个字段,其余随对话面需要再补。
 */
export type ApprovalEntry = {
  approval_id: string;
  capability: string;
  args_summary: string;
  state: string;
};

export type JobInfo = {
  id: string;
  command: string;
  status: "running" | "succeeded" | "failed";
  exit_code: number | null;
  elapsed_ms: number;
  log_path: string;
};

import { redirectToLogin } from "@/lib/utils";

export type Provider = {
  id: string;
  name: string;
  baseUrl: string;
  models: string[];
  /** W6 常用清单(对话输入框候选来源;⊂ models) */
  modelsCommon?: string[];
  /** 模型窗口登记(模型 → 上下文窗口 token 数;context-inspector「真实水位」数据源,未登记 = 未知) */
  modelWindows?: Record<string, number>;
  defaultModel?: string;
  secretSet: boolean;
};

export type ProviderInput = {
  name: string;
  baseUrl: string;
  apiKey?: string;
  models?: string[];
  modelsCommon?: string[];
  modelWindows?: Record<string, number>;
  defaultModel?: string;
};

export type ProbeResult = {
  ok: boolean;
  status?: number;
  latencyMs: number;
  models?: string[];
  error?: string;
};

export type McpServer = {
  name: string;
  transport: "stdio" | "sse" | "http" | "streamable-http";
  url?: string;
  bearer_token?: string;
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  tool_timeout_ms?: number;
  restart_limit?: number;
  trust?: "explicit-config";
  sha256?: string;
};

/** web_multisearch providers 配置里单家的结构(manifest 模板/已存配置通用) */
export type ProviderManifestItem = {
  id: string;
  /** 停用开关(缺省 = 生效);停用家不参与搜索轮转,真搜测试仍可用 */
  enabled?: boolean;
  /** 删除内置的墓碑标记(抑制插件「缺失内置回填」);自定义家删除即整条移除 */
  deleted?: boolean;
  name: string;
  builtin?: boolean;
  endpoint: string;
  method: string;
  auth: string;
  auth_name: string;
  key?: string;
  query_param: string;
  limit_param: string;
  results_path: string;
  title_field: string;
  url_field: string;
  desc_field: string;
  parse?: string;
  quota?: number;
};

export type McpListResult = {
  file: string;
  servers: McpServer[];
  entries?: {
    server: McpServer;
    manifest?: {
      title?: string;
      description?: string;
      config_schema?: {
        key: string;
        label: string;
        hint?: string;
        type: "string" | "secret" | "range" | "select" | "providers";
        default?: string | number;
        min?: number;
        max?: number;
        unit?: string;
        options?: { value: string; label: string }[];
        /** providers 类型专用:内置供应商默认模板(编辑时下拉展示+预填) */
        items?: ProviderManifestItem[];
      }[];
    } | null;
    config?: Record<string, unknown>;
    /** ADR-0023:来源(bundled=官方随包;data=数据目录手动放置) */
    origin?: "bundled" | "data" | "unknown";
    /** 官方随包来源但最新官方清单已不含 → 建议删除 */
    deprecated?: boolean;
  }[];
  loadedAtBoot: string[];
  note: string;
};

export type McpReloadResult = {
  ok: boolean;
  registered: string[];
  updated?: string[];
  uninstalled?: string[];
  failed: { name: string; error: string }[];
  note: string;
};

export type Capability = {
  name: string;
  provider?: string;
  effect?: string;
  idempotent?: boolean;
  approval?: string;
  /** ADR-0045:provider 声明的插件身份(tool/connector);缺省 = 未声明 */
  plugin_kind?: string | null;
  plugin_id?: string | null;
  plugin_version?: string | null;
};

/** ADR-0042:通用 wasm 插件声明条目(boenmind-contracts/plugin/wasm-plugin.v0_1)。 */
export type WasmPlugin = {
  capability: string;
  provider?: string;
  version?: string;
  wasm: string;
  effect?: string;
  approval?: string;
  idempotent?: boolean;
  timeout_ms?: number;
  description?: string;
  scopes?: string[];
};

export type FsEntry = { name: string; kind: "dir" | "file"; size: number | null };

// 任意目录浏览(工作目录选择器专用;只读、仅目录、只报名字)
export type BrowseEntry = { name: string; path: string };

export type RoleItem = {
  id: string;
  name: string;
  description?: string;
  system_prompt: string;
  skills?: string[];
};

// W4b Skill 知识包(合同 capability/skill.v0_1;只是数据,加载不改变权限)
export type SkillItem = {
  skill_id: string;
  name: string;
  version?: string;
  description?: string | null;
  instruction: string;
  allowed_capabilities?: string[];
  references?: { name: string; path: string }[];
  /** 合同 v0_1 scripts 清单(Skill v0.2 执行面预留);后端原样透传技能文件 */
  scripts?: unknown[];
};

export type RolesResponse = {
  ok: boolean;
  active_id: string;
  roles: RoleItem[];
};

// W8 常规(ADR-0018):工作区注册表条目 + 运行环境探针结果
export type WorkspaceEntry = {
  id: string;
  name: string;
  path: string;
  exists: boolean;
  isDefault: boolean;
};

export type RuntimeToolInfo = {
  installed: boolean;
  version: string | null;
  program: string | null;
  /** 探测命中候选的参数列表(与 program 拼成完整命令;两分支恒写入) */
  argv: string[];
  error: string | null;
};

export type RuntimeEnv = { python: RuntimeToolInfo; node: RuntimeToolInfo };

// W5:一次模型调用的上下文快照(/admin/context 行;服务端已做凭据脱敏与
// 单条内容 16K 字符截断)
export type CtxStep = {
  // W9:kind 存在 = 轨迹事件行(tool_call/tool_result/assistant_final/
  // turn_end),data 携带事件载荷;kind 缺失 = W5 模型调用快照行。
  kind?: string;
  data?: Record<string, unknown>;
  seq: number;
  ts: string;
  session_id: string;
  agent_id: string;
  operation_id: string;
  turn_index: number;
  step: number;
  attempt?: number;
  model_id: string;
  streaming: boolean;
  messages: { role: string; content: string; content_truncated?: boolean }[];
  tools: { function?: { name?: string; description?: string } }[];
  status: "ok" | "error" | "cancelled";
  error_code?: string | null;
  tokens_in?: number | null;
  tokens_out?: number | null;
  /** 推理思考消耗(提供商如实上报才有,不报为 null——界面显示「未上报」,不估算冒充) */
  tokens_reasoning?: number | null;
  /** 提示词缓存命中(同上,提供商上报才有) */
  tokens_cached?: number | null;
  /** 首字延迟 ms(仅流式可测;非流式为 null) */
  ttft_ms?: number | null;
  /** 组装本次请求时已被双上限丢弃的历史轮数(0 = 无遗忘) */
  evicted_turns?: number | null;
  latency_ms?: number | null;
};

async function req<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init);
  const body = await res.json().catch(() => null);
  // 门户会话失效(2026-09-06):统一正向跳登录,门户口自身除外
  if (res.status === 401 && !url.startsWith("/api/portal/")) {
    redirectToLogin();
  }
  if (!res.ok) {
    const msg =
      (body as { error?: { message?: string } } | null)?.error?.message ??
      `HTTP ${res.status}`;
    throw new Error(msg);
  }
  return body as T;
}

const json = (method: string, body: unknown): RequestInit => ({
  method,
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify(body),
});

export const api = {
  limits: {
    get: () => req<{ ok: boolean; keys: LimitKey[] }>("/admin/limits"),
    put: (values: Record<string, number>) =>
      req<{ ok: boolean; note: string }>("/admin/limits", json("PUT", { values })),
  },
  jobs: () => req<{ ok: boolean; jobs: JobInfo[] }>("/admin/jobs"),
  providers: {
    list: () => req<{ providers: Provider[] }>("/admin/providers"),
    create: (b: ProviderInput) =>
      req<{ provider: Provider }>("/admin/providers", json("POST", b)),
    update: (id: string, b: ProviderInput) =>
      req<{ provider: Provider }>(`/admin/providers/${id}`, json("PUT", b)),
    remove: (id: string) =>
      req<{ ok: boolean }>(`/admin/providers/${id}`, { method: "DELETE" }),
    /** issue #13:软删除历史(墓碑清单 + 恢复) */
    history: () =>
      req<{
        ok: boolean;
        history: {
          id: string;
          name: string;
          baseUrl: string;
          models: string[];
          defaultModel: string;
          secretSet: boolean;
          deleted_at: number;
        }[];
      }>("/admin/providers/history"),
    restore: (id: string) =>
      req<{ ok: boolean; provider: Provider }>(
        "/admin/providers/history/restore",
        json("POST", { id }),
      ),
    probe: (baseUrl: string, apiKey?: string) =>
      req<ProbeResult>("/admin/providers/probe", json("POST", { baseUrl, apiKey })),
    /** issue #12:熔断健康快照(healthy/unavailable + 连败/冷却) */
    health: () =>
      req<{
        ok: boolean;
        health: {
          provider: string;
          status: string;
          fail_streak: number;
          cooldown_until: string | null;
        }[];
      }>("/admin/providers/health"),
    setActive: (providerId: string, modelId?: string) =>
      req<{ ok: boolean; restartRequired: boolean; note: string }>(
        "/admin/model/active",
        json("PUT", { providerId, modelId }),
      ),
  },
  activeModel: () =>
    req<{ values: Record<string, unknown>; secret_set: Record<string, boolean> }>(
      "/admin/model/active",
    ),
  portal: {
    state: () => req<{ configured: boolean; authed: boolean }>("/api/portal/state"),
    login: (password: string) =>
      req<{ ok: boolean }>("/api/portal/login", json("POST", { password })),
    changePassword: (old: string, next: string) =>
      req<{ ok: boolean; note?: string }>(
        "/api/portal/password",
        json("POST", { old, new: next }),
      ),
  },
  // 无鉴权探针(GET /health;门户墙与就绪轮询共用)。不触发 401 跳登录。
  health: () => req<{ ok: boolean; version: string; state: string }>("/health"),
  // W1 对话面模型标签(GET /v1/models):仅取默认模型 id。
  models: () =>
    req<{ object: string; data: { id: string; object: string }[] }>("/v1/models"),
  // 待裁决审批队列轮询面(W4b;POST /admin/approvals/{id}/respond 与 /rpc 同执行体)。
  // 裁决权在服务端(ADR-0030),前端只呈现与转发。
  approvals: {
    list: () =>
      req<{ approvals: ApprovalEntry[] }>("/admin/approvals"),
    respond: (id: string, decision: "approve" | "deny") =>
      req<{ ok: boolean }>(
        `/admin/approvals/${encodeURIComponent(id)}/respond`,
        json("POST", { decision, scope: decision === "approve" ? "once" : undefined }),
      ),
  },
  mcp: {
    list: () => req<McpListResult>("/admin/mcp"),
    create: (b: Partial<McpServer>) => req<{ ok: boolean }>("/admin/mcp", json("POST", b)),
    update: (name: string, b: Partial<McpServer>) =>
      req<{ ok: boolean }>(`/admin/mcp/${name}`, json("PUT", b)),
    remove: (name: string) =>
      req<{ ok: boolean }>(`/admin/mcp/${name}`, { method: "DELETE" }),
    reload: () =>
      req<McpReloadResult>("/admin/mcp/reload", { method: "POST" }),
    candidates: () =>
      req<{
        ok: boolean;
        dir: string;
        /** 官方随包插件目录;null = 未检出(cfg.bundled_plugins_dir 缺省) */
        bundled_dir: string | null;
        candidates: {
          file: string;
          name: string;
          title: string;
          description: string;
          registered: boolean;
          /** bundled=官方随包;data=数据目录手动放置(后端恒写入) */
          source: string;
          /** 在删除名单(墓碑)中:批准接入即恢复 */
          tombstoned?: boolean;
        }[];
        note: string;
      }>("/admin/mcp/candidates", { method: "POST" }),
    approve: (name: string) =>
      req<{
        ok: boolean;
        note: string;
        /** ADR-0023:批准即自动热重载;ok=false 时 skipped 说明原因(如测试态) */
        reload?: { ok: boolean; tools?: number | null; failed?: unknown[]; skipped?: string };
      }>("/admin/mcp/approve", json("POST", { name })),
    // ADR-0023:卸载并物理删除插件文件(警告栏确认后调用)
    purge: (name: string) =>
      req<{
        ok: boolean;
        deleted: string[];
        renamed_aside: string[];
        errors: string[];
        note: string;
      }>(`/admin/mcp/${name}/purge`, { method: "POST" }),
    status: () =>
      req<{
        status: {
          name: string;
          ok: boolean;
          tools?: number;
          tool_list?: { name: string; description?: string }[];
          error?: string;
        }[];
      }>("/admin/mcp/status"),
    getConfig: (name: string) =>
      req<{ name: string; values: Record<string, unknown> }>(
        `/admin/mcp-config/${name}`,
      ),
    saveConfig: (name: string, values: Record<string, unknown>) =>
      req<{ ok: boolean; note: string }>(
        `/admin/mcp-config/${name}`,
        json("PUT", { values }),
      ),
    // web_multisearch 扩展:真搜索测试(返回某家真实结果)
    testSearch: (name: string, providerId: string, query: string, limit?: number) =>
      req<{
        ok: boolean;
        name: string;
        result?: {
          success: boolean;
          provider_id?: string;
          provider_name?: string;
          timing_ms?: number;
          count?: number;
          results?: { title?: string; url?: string; description?: string }[];
          error?: string;
        };
        error?: string;
      }>(`/admin/mcp/search-test/${encodeURIComponent(name)}`, json("POST", {
        provider_id: providerId,
        query,
        limit: limit ?? 5,
      })),
    // web_multisearch 扩展:读月度用量(画进度条)
    getUsage: (name: string) =>
      req<{
        ok: boolean;
        name: string;
        usage?: { month?: string; providers?: Record<string, number> };
        error?: string;
      }>(`/admin/mcp/usage/${encodeURIComponent(name)}`, { method: "GET" }),
    // issue #28:子进程 stderr 环形缓冲尾部(跨 respawn 带代标记)
    getStderr: (name: string, lines = 100) =>
      req<{
        ok: boolean;
        name: string;
        lines?: { generation: number; text: string }[];
        error?: string;
      }>(
        `/admin/mcp/stderr/${encodeURIComponent(name)}?lines=${lines}`,
        { method: "GET" },
      ),
  },
  roles: {
    get: () => req<RolesResponse>("/admin/roles"),
    save: (role: Partial<RoleItem> & { id: string; set_active?: boolean }) =>
      req<{ ok: boolean; note: string; active_id: string }>("/admin/roles", json("POST", role)),
    delete: (id: string) =>
      req<{ ok: boolean; note: string; active_id: string }>(`/admin/roles/${encodeURIComponent(id)}`, {
        method: "DELETE",
      }),
    setActive: (id: string) =>
      req<{ ok: boolean; note: string; active_id: string }>(
        `/admin/roles/active/${encodeURIComponent(id)}`,
        { method: "PUT" },
      ),
  },
  skills: {
    list: () => req<{ ok: boolean; skills: SkillItem[] }>("/admin/skills"),
    save: (skill: SkillItem) =>
      req<{ ok: boolean; note: string }>("/admin/skills", json("POST", skill)),
    remove: (id: string) =>
      req<{ ok: boolean; note: string }>(
        `/admin/skills/${encodeURIComponent(id)}`,
        { method: "DELETE" },
      ),
  },
  logs: () =>
    req<{ ok: boolean; exec: string[]; events: string[]; context: string[] }>("/admin/logs"),
  // issue #14:Turn 内调试日志(状态+尾读 / 热开关)
  debugTurns: () =>
    req<{ ok: boolean; enabled: boolean; lines: string[] }>("/admin/debug/turns"),
  setDebugTurns: (enabled: boolean) =>
    req<{ ok: boolean; enabled: boolean }>("/admin/debug/turns", json("POST", { enabled })),
  // W7 关于与在线升级(apply 仅回环;发新版本必须用户明说,此处只消费)
  about: {
    get: () =>
      req<{ version: string; platform: string; dataDir: string; repo: string }>("/admin/about"),
    checkUpdate: () =>
      req<{
        ok: boolean;
        current: string;
        latest?: string;
        updateAvailable?: boolean;
        asset?: { name: string; url: string };
        /** 有更新但缺本平台资产的提示语(勿与发布说明 notes 混淆) */
        note?: string | null;
        notes?: string;
        error?: string;
      }>("/admin/about/check-update", { method: "POST" }),
    applyUpdate: () =>
      req<{ ok: boolean; restarting: boolean; note: string }>("/admin/about/apply-update", {
        method: "POST",
      }),
  },
  // W5 上下文透视:模型调用请求快照(context-log.jsonl 尾部,最旧在前)
  context: () => req<{ ok: boolean; steps: CtxStep[] }>("/admin/context"),
  // 会话删除(2026-09-06 A+B):墓碑+对话原文擦除,不可恢复
  sessionDelete: (sid: string) =>
    req<{ ok: boolean; session_id: string; deleted_at: string; purged_lines: number }>(
      `/admin/sessions/${encodeURIComponent(sid)}`,
      { method: "DELETE" },
    ),
  // 会话权限模式读取(ADR-0030):服务端权威,前端仅选择器显示面
  sessionMode: (sid: string) =>
    req<{ ok: boolean; session_id: string; permission_mode: PermissionMode }>(
      `/admin/sessions/${encodeURIComponent(sid)}/mode`,
    ),
  // 会话权限模式变更(ADR-0030):发指令改服务端会话状态并落事实事件;
  // 裁决在服务端——关闭网页/换设备不影响在跑任务,审批自动放行可审计
  sessionModeSet: (sid: string, mode: PermissionMode) =>
    req<{ ok: boolean; session_id: string; permission_mode: PermissionMode }>(
      `/admin/sessions/${encodeURIComponent(sid)}/mode`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ mode }),
      },
    ),
  // 会话目录(2026-09-08 三端一致批):服务端权威列表,SessionPanel 唯一
  // 数据源(此前 bm_sessions 存浏览器本地,三设备各记各账不一致的根因)。
  // 按最近活跃倒序
  sessionList: () =>
    req<{
      ok: boolean;
      /** 目录总会话数(服务端权威;分页后 sessions 只是其中一页) */
      total: number;
      /** 当前页之后还有更多会话 */
      truncated: boolean;
      sessions: {
        id: string;
        state: string;
        title: string | null;
        created_at: string;
        updated_at: string | null;
      }[];
    }>("/admin/sessions"),
  // 会话历史回放(2026-09-06):切会话/刷新后按 sid 拉历史消息(最旧在前)。
  // 分页(2026-09-06 二改):limit 默认 50 上限 200;skip = 从最新一条往回
  // 跳过的条数(已加载越多 skip 越大);has_more 指示是否还有更早——
  // 防长会话一口气载入卡界面。游标不用 seq(历史文件 seq 跨重启重数)
  sessionMessages: (
    sid: string,
    opts?: { limit?: number; skip?: number },
  ) => {
    const p = new URLSearchParams();
    if (opts?.limit) p.set("limit", String(opts.limit));
    if (opts?.skip) p.set("skip", String(opts.skip));
    const qs = p.toString();
    return req<{
      ok: boolean;
      session_id: string;
      has_more: boolean;
      messages: { seq: number | null; ts: string | null; role: "user" | "assistant"; content: string }[];
    }>(`/admin/sessions/${encodeURIComponent(sid)}/messages${qs ? `?${qs}` : ""}`);
  },
    contextSearch: (q: string) =>
      req<{ ok: boolean; hits: CtxStep[]; total: number }>(
        `/admin/context/search?q=${encodeURIComponent(q)}&limit=50`,
      ),
  capabilities: () =>
    req<{
      builtin: Capability[];
      mcp: { name: string; tools: number | null; loaded: boolean; pendingRemoval?: boolean }[];
    }>(
      "/admin/capabilities",
    ),
  // ADR-0041/0042:通用 wasm 插件(config/plugins.json;增删改即热重载)
  plugins: {
    list: () => req<{ ok: boolean; plugins: WasmPlugin[] }>("/admin/plugins"),
    set: (p: WasmPlugin) => req<{ ok: boolean; note: string }>("/admin/plugins", json("POST", p)),
    remove: (capability: string) =>
      req<{ ok: boolean; note: string }>(
        `/admin/plugins/${encodeURIComponent(capability)}`,
        { method: "DELETE" },
      ),
    reload: () => req<{ ok: boolean; note: string }>("/admin/plugins/reload", { method: "POST" }),
  },
  fs: {
    list: (path: string) =>
      req<{ path: string; entries: FsEntry[]; root: string }>(
        `/admin/fs/list?path=${encodeURIComponent(path)}`,
      ),
    // 工作目录选择器:全盘只读目录浏览(空 path = 根视图,Windows 盘符)
    browse: (path: string) =>
      req<{
        path: string;
        parent: string | null;
        entries: BrowseEntry[];
        truncated: boolean;
        note?: string;
      }>(`/admin/fs/browse?path=${encodeURIComponent(path)}`),
    file: (path: string) =>
      req<{ path: string; name: string; size: number; content: string }>(
        `/admin/fs/file?path=${encodeURIComponent(path)}`,
      ),
    // W7 目录树右键菜单
    rename: (path: string, name: string) =>
      req<{ ok: boolean }>(`/admin/fs/rename`, json("POST", { path, name })),
    downloadUrl: (path: string) => `/admin/fs/download?path=${encodeURIComponent(path)}`,
    // 2026-09-07 目录树批次:新建目录(选择器全盘)与批量删除(工作区)
    mkdir: (parent: string, name: string) =>
      req<{ ok: boolean; path: string }>(`/admin/fs/mkdir`, json("POST", { parent, name })),
    delete: (paths: string[]) =>
      req<{
        ok: boolean;
        deleted: number;
        results: { path: string | null; ok: boolean; error?: string }[];
      }>(`/admin/fs/delete`, json("POST", { paths })),
  },
  // W8 常规(ADR-0018):工作区注册表 + 运行环境探针
  workspaces: {
    list: () => req<{ workspaces: WorkspaceEntry[] }>("/admin/workspaces"),
    create: (b: { name: string; path: string }) =>
      req<{ workspace: WorkspaceEntry }>("/admin/workspaces", json("POST", b)),
    update: (id: string, b: { name?: string; path?: string }) =>
      req<{ workspace: WorkspaceEntry }>(`/admin/workspaces/${id}`, json("PUT", b)),
    remove: (id: string) =>
      req<{ ok: boolean }>(`/admin/workspaces/${id}`, { method: "DELETE" }),
    check: (id: string) =>
      req<{ ok: boolean; path?: string; error?: string }>(
        `/admin/workspaces/${id}/check`,
        json("POST", {}),
      ),
  },
  runtimeEnv: () => req<RuntimeEnv>("/admin/runtime/env"),
};
