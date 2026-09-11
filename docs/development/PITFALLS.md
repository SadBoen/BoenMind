# 环境怪癖与已知坑（PITFALLS）

> 定位：做过错、修过、验证过的坑清单。只收录"下次遇到还会踩"的环境怪癖，
> 已修复的 bug、已废止的旧事物不再收录（溯 git 历史）。

## 启动与配置

- **单进程铁律**：同数据目录绝不跑两个 `boenmind-server`——双进程 = 持久层毒化。启动前先确认旧进程已停（`Get-Process boenmind-server`）。
- **环境变量内联**：必须与命令同一行，不能提前 export 后分行跑（Git Bash 会话级变量在子进程不可见）。
- **MCP 启动参数**：必须带 `--mcp-config <数据目录>\mcp.json`，漏带 = 管理面报「服务器未启用 MCP 配置文件」。
- **Node 环境**：Git Bash 里用 fnm，命令前需 `source ~/.bashrc` 激活。
- **webapp 包管理器**：只用 npm（唯一权威），pnpm 残留已入 `.gitignore` 防污染。

## 前端真浏览器实测抓出的坑

1. **事件信封字段名**：JSON 字段名是 `type`（serde rename），不是 Rust 字段名 `event_type`。
2. **EventSource 无 Auth 头**：SSE 无法携带 Authorization → `/events` 被 401 拒绝，前端改用 `events.poll` 轮询（1.5s）。
3. **静态页缓存**：浏览器缓存旧页，发版后必须 Ctrl+F5 或带查询串强刷。
4. **内联脚本语法错误静默失效**：整页按钮无反应且无报错，改完必须 `node --check` 验证。
5. **Playwright chromium CDN 装不上**：本机跑 `npm run test:smoke` 报 Executable doesn't exist、`npx playwright install` 下载失败时，改用本机 Edge 通道：`npx playwright test --config playwright.smoke.local.config.ts`（旁路配置已入库）。CI 正常装浏览器，不受影响。
6. **content-visibility 虚拟化干扰 Playwright 点击**：`.msg` 开了 CSS 虚拟化后，assistant 消息内元素的 locator click 会卡 actionability 超时——测试里点这类元素改走坐标路径（`cua.click`）或先断言文本即可。

**教训**：229 个测试全绿测不出这类用户可见面 bug——用户可见面必须真实浏览器手测（硬纪律 7）。

## 内置浏览器面板（IAB）怪癖

- **CDP press 字符注入失效**：在内置面板无效（原生框也收不到），真键盘路径只能 playwright-core + 真实浏览器。
- **旧 tab id 会 unavailable**：须重新 list。
- **evaluate/截图会话级坏死**：用 title 探针 + 快照代替。
- **验收证据 = 页面可见内容/截图**：接口绿 ≠ 界面好，milestones/shots-*/ 留档为证。

## 后台任务陷阱

- **ZCode 后台任务带走 server**：在 ZCode 会话内 `run_in_background` 拉起的实例，会话退出时被静默杀掉（2026-09-07/08 两次同签名：无 panic、日志无收尾、退出码 1）。
  - **解法**：长驻运行须独立于 ZCode 启动（用户自开终端/计划任务），AI 会话内拉起的实例只当临时测试。
  - **排障**：发现 `/health` 失联先看进程在不在（`Get-Process`），重启即可。

## Rust 与测试

- **`cargo fmt` 重排代码**：文本替换前先看当前实际内容（Edit 前必须 Read 或在上下文）。
- **时间基准**：对照 `MockClock` 实际基准值换算，别拿直觉时间写断言。
- **Provider 执行通道**：同步 `CapabilityProvider`（进程内快路径）与异步 `AsyncCapabilityExecutor`（MCP 等慢外部，spawn + 超时）共用 Broker 决策管线，耗时能力必须注册为异步，否则占死单写者循环。

## 文档操作

- **大段内联脚本静默失败**：Python heredoc 写文件易失败，先 Write 成文件再执行。
- **基线编号是硬锚点**：§1-§24 被 adr/、milestones/ 大量引用（§13.5/§17/§18/§19 最密），重排前先 `grep -rn "基线 §"`。

## 已废止速查（查旧资料前先看这里）

| 旧事物 | 现状 |
|--------|------|
| dsh 前端 runtime/web | 已删，归档分支 `archive/m10-dsh-frontend`；继任 = runtime/webapp |
| code-tools 随包插件 | 已退役（ADR-0021 内置化为 fs.* 工具集） |
| context-mode 插件 | 已更名 context-inspector |
| 天机阁/deepseek-v4-flash 中转 | 已清；现用 OpenCode Go mimo-v2.5(zen 网关) |
