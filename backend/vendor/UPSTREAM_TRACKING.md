# 上游生态吸收台账（UPSTREAM TRACKING REGISTRY）

本文件是 **BoenMind 吸收外部生态资产的唯一权威记录**（2026-08-15 用户定调）：
> 能插件化的、网上已有的 skill/MCP/库，就用网上的——找到最优解，转成**自己的官方插件**，
> 做好上游跟踪台账。

区别于 `UPSTREAM_PATCHES.md`（vendored 代码补丁台账）：本台账记录**选型决策与上游资产引用**
（直接依赖 / 转官方插件 / 包装适配），每项记录上游来源、版本基线、许可、落地方式与跟踪项。

吸收纪律（沿用 [[pi-vendor-patch-policy]] 与 [[ecosystem-adapter-principle]] 精神）：
- **核心格式自研，思路照学**：机制吸收进架构，代码按 BoenMind 实际落（学 dsh 不抄 dsh）
- **转官方插件 = 适配层**：上游能力包装成官方插件/宿主组件，不动内核；上游升级 = 换适配层版本
- **上游问题优先提 issue**，本地改动最小化并在此登记

---

## 台账条目

### T1 终端渲染：@xterm/xterm（前端）
- **上游**：https://github.com/xtermjs/xterm.js（npm `@xterm/xterm`）
- **基线**：6.0.0（2026-08-15 查证；旧包名 `xterm` 5.3.0 已弃用）
- **许可**：MIT
- **地位**：终端渲染事实标准（VS Code 同源），无争议最优解
- **落地**：前端直接依赖 + 宿主组件 TerminalPane 封装（编程壳等嵌入）；不做 fork，版本升级走 npm
- **跟踪**：版本升级注意 xterm 5→6 API 变动（addon 包名同步 `@xterm/addon-*`）

### T2 终端后端：portable-pty（Rust pty）
- **上游**：https://github.com/wezterm/wezterm/tree/main/pty（crate `portable-pty`）
- **基线**：0.9.x（2026-08-15 调研）
- **许可**：MIT
- **地位**：跨平台 pty 事实标准（Windows ConPTY + Unix openpty），wezterm/cockpit 等验证
- **落地**：bm-server 直接依赖（blocking API → spawn_blocking/读线程包裹，wezterm 同款路线）
- **跟踪**：**`xpty`**（2026-03 发布，portable-pty 0.9.0 的 async fork，tokio 原生 + 更好 Windows ConPTY 控制）
  ——值得跟踪，成熟后换装；上游 issue 可提 async 支持需求

### T3 代码图谱：code-graph-mcp（@sdsrs/code-graph）
- **上游**：https://github.com/sdsrss/code-graph-mcp（npm `@sdsrs/code-graph`）
- **基线**：0.116.0（2026-08-14 已装为 ZCode MCP，见 [[tooling-evaluation]]）
- **许可**：MIT
- **地位**：19 语言 tree-sitter AST 知识图谱 MCP（get_call_graph/impact_analysis/dependency_graph/
  find_dead_code/trace_http_chain 等），本机已实际使用验证
- **落地**：**转 BoenMind 官方插件**（TS 插件包：调用图/影响分析/依赖图工具进编程场景工具面，
  scopes=["coding"]——正好验证场景工具面按 app 组装）；实现 = 适配层调其核心逻辑或进程，不 fork 内核
- **备选**：Codebase Memory MCP（158 语言单二进制零依赖，2026 开源）——重引擎，未来语言覆盖不足时评估
- **跟踪**：上游 0.x 快速迭代期，转插件时锁定版本基线

### T4 浏览器仿真（编程壳"浏览器"子窗口）
- **落地形态**（用户拍板 B 方案）：可视化现有 web 工具（web-search/web-scraping 官方插件链）
  的调用与结果——无新增外部依赖
- **备选**（真浏览器引擎，后续评估）：browser-use / Playwright MCP 转官方插件
- **跟踪**：web-scraping 插件链成熟后评估真浏览器

### T5 应用布局系统：dockview（前端 dock layout）
- **上游**：https://github.com/mathuo/dockview（npm `@dockview/core` + `@dockview/react`）
- **基线**：8.1.0（2026-08-15 调研确认）
- **许可**：MIT
- **地位**：2026 调研最优 React dock 布局库——功能全（停靠/悬浮/弹出窗口/Tab 叠放/最大化/
  分界线拖拽/布局序列化/主题）、最活跃、零依赖核心 + 多框架绑定；候选对比：flexlayout-react
  （React-only 老牌但功能弱）、rc-dock（维护停滞）
- **落地**：架构 §四·B 补充 2（v0.23 用户拍板"VS Code workbench 模型"）；封装为宿主组件
  `DockLayout` + 视图注册表 VIEWS，不直接散用上游 API；对话视图单实例绑定场景、其他视图多开
- **跟踪**：版本升级注意 API 变动（dockview 迭代快，封装层隔离）

---

## 更新记录

| 日期 | 条目 | 动作 |
|---|---|---|
| 2026-08-15 | T1-T4 | 建台账：用户定调"网上有的就用网上的，转官方插件 + 台账"；三功能选型定案 |
| 2026-08-15 | T5 | 应用布局系统选型：dockview 8.1 定案（用户拍板 VS Code workbench 模型，§四·B 补充 2 v0.23） |
