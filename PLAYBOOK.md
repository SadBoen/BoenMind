# PLAYBOOK — 实操备忘与踩坑清单

> 定位:写代码/测界面时的工具书(ADR-0015 确立,原散落自 AGENTS.md 备忘区)。只放「做过错、修过、验证过」的条目;新坑按节追加,定期去重。规则类内容不在本文件,看 AGENTS.md。

## 1. 启动与运行(本机 Windows)

- **单进程铁律**:同数据目录绝不跑两个 boenmind-server——双进程=持久层毒化。启动/重启前先确认旧进程已停。
- 启动形状:环境变量**必须内联**在同一行命令;Web 前端指向 `--web-dir runtime/webapp/dist`(先 `cd runtime/webapp && npm run build`)。密钥类值不入仓,看本地 `.secrets/`(dev.env 不入 git,example.env 入)。
- 生效模型优先级:**config/model.json(数据目录)> 环境变量**。现值 = OpenCode Go(mimo-v2.5,经 zen 网关);天机阁/deepseek-v4-flash 中转已清。
- 配置文件都在**数据目录**(默认 `%APPDATA%\Roaming\boenmind\`,可用 `--data-dir` 改),不在仓库:`config/model.json`(当前模型)、`config/providers.json`(W2 起=界面 provider 库)、`config/roles.json`(W4 角色配置)、`mcp.json`(MCP server 配置)。
- **MCP 插件目录 = `<数据目录>\mcp\`**(2026-09-02 规定):插件可执行文件放这里 → 管理界面「扫描插件」发现候选(候选以 `--self-describe` 自报家门)→「批准接入」落盘 mcp.json + manifests/ →「重载 MCP」免重启上线。扫描只认该目录内可执行文件;显式批准=安装(ADR-0005/0006)。web_multisearch Rust exe 已现役于此。
- 启动必须带 `--mcp-config <数据目录>\mcp.json`(漏带 = MCP 管理面报「服务器未启用 MCP 配置文件」,2026-09-02 踩实);
- 环境变量(boenmind-server):

```text
BOEN_MODEL_BASE_URL / BOEN_MODEL_ID / BOEN_MODEL_STREAM / BOEN_MODEL_API_KEY
    模型接线兜底(config/model.json 优先;STREAM=1 开流式;key 首启播种加密密钥库)
BOEN_SECRET_MASTER_KEY   加密 FileSecretStore 主密钥(≥32 字符),真实网关模式必需
BOEN_WORKSPACE_DIR       工作区/文件浏览根,默认 <data-dir>/workspace
BOEN_TURN_TIMEOUT_SECS   回合内每次模型调用 deadline(秒;ADR-0028 起默认 0=不限时;与 config/limits.json 同源,env 设置 >0 时优先级更高)
BOEN_OPENCODE_SESSION_ID OpenCode Go 网关 x-opencode-session 头的固定值(可选;缺省进程内随机;
    2026-09-07 起网关缺失该头即 400「cannot be routed efficiently」,openai_http 已默认带)
    限制总旋钮 = <data>/config/limits.json(设置页「限制与超时」全量可改,保存即热生效;
    exec 命令默认 120s/前台硬顶 600s;超限自动转后台,system.job_output 收取)
测试门控(非启动):BOEN_LIVE(+_BASE_URL/_MODEL/_API_KEY)、BOEN_RELEASE、
    BOEN_MCP_STDIO_TEST、BOEN_APPS_E2E
反向纪律:BOEN_* 前缀变量禁止下发给 MCP 子进程(INV-5)
```

- Node 走 fnm:Git Bash 里 node 命令前需 `source ~/.bashrc`。
- **发版(2026-09-02 铁规矩:必须用户明确说才发)**:打 `v*` tag 推送 → release 工作流自动出 **Linux x86_64 单平台 .tar.gz**(v0.0.3 起用户裁决只要 Linux 服务器版;Windows 资产已移除,Windows 端在线升级会提示缺本平台资产)并建 Release;**发版时同步 bump `runtime/Cargo.toml` 的 workspace version**(与 tag 一致,/health 与「关于」页/在线升级都读它);在线升级端点 `/admin/about/*` 仅回环可 apply。

## 2. 前端实测四坑(2026-08-30 真浏览器实测抓出)

1. **事件信封 JSON 字段名是 `type`**(serde rename),不是 Rust 字段名 `event_type`——前端按后者读永远 undefined;
2. **EventSource(SSE)无法携带 Authorization 头** → /events 被 401 静默拒绝;前端走合同方法 `events.poll` 轮询(1.5s);
3. **静态页无缓存头,浏览器缓存旧页**——发版后必须 Ctrl+F5 或带查询串;
4. 内联脚本的语法错误会**整页静默失效**(所有按钮无反应且无报错),改完必须 `node --check`。

教训:229 个测试全绿测不出这四个 bug——所以有硬纪律 7(用户可见面必须真实浏览器手测)。

## 3. 浏览器自动化怪癖(内置浏览器面板/IAB)

- **指令口径(2026-09-07 用户裁定)**:用户说「仿真模拟/模拟操作/实测」且涉及网页 UI = 一律开真实浏览器(browser-use 内置面板或 computer-use)可视化点击输入+截图为证;接口调用/脚本/隐身浏览器截图不算"仿真",须用户明示「脚本模拟就行」才允许走快路;
- **「任务没跑成功」排障三坑(2026-09-07 全夜实战)**:①同数据目录幽灵双开会把赢家推入粘性拒写态(界面报「Runtime 排空中或持久层故障」),修复=绑定前置到开库前,排障先查 `Get-Process boenmind-server` 是否多只+events.jsonl 序号是否撞车;②旧会话遗留 bash 守护循环会延迟 2~3 分钟重生 server,杀后必须观察 ≥150s;③审批标记只随回合 /v1 流下发,后台续跑回合无流=审批永远无人批——已补 GET /admin/approvals 轮询通道+前端 2.5s 轮询(YOLO 自批/ask 进抽屉);
- **server 挂在 ZCode 后台任务下会被静默带走**(2026-09-07 06:57 与 2026-09-08 08:2x 两次同签名:无 panic、日志无收尾行、任务退出码 1;第二次死于会话中途且当时无任何操作)。排障=发现 /health 失联先看进程在不在,重启即可;**长驻运行须独立于 ZCode 会话启动**(用户自开终端/计划任务),AI 会话内 run_in_background 拉起的实例只当临时测试态,重要时段前先探活;
- evaluate/截图会话级坏死:用 title 探针+快照代替;受控输入=点击+逐字+双击发送,中文进不去;
- **CDP press 的字符注入在内置面板失效**(原生框也收不到)——真键盘路径只能 playwright-core+真实浏览器或交用户实测,注入旁路全是假阴性;
- 旧 tab id 会彻底 unavailable,须重新 list;确定性 E2E 用 `?e2e=` 钩子;界面别依赖原生 modal;
- 验收证据=页面可见内容/截图留档(milestones/shots-*/),接口绿≠界面好。

## 4. Rust 与测试

- 测试先行持续抓真 bug;全量回归:`cargo test`(crate 内 `-p bm-<name>`);
- 性能:`cargo test --release -p bm-testkit --test perf_smoke -- --ignored --nocapture`(perf_m2 同理);
- `cargo fmt` 会重排代码:文本替换前先看当前实际内容;
- 跨字段借用冲突:分阶段作用域解决;
- 时间基准:对照 MockClock 实际基准值换算,别拿直觉时间写断言;
- libsqlite3-sys bundled 默认启用 SQLITE_ENABLE_FTS5——M5 memory 检索 FTS5 实际生效,LIKE 仅兜底;
- 规模参考:9 个 crate 测试覆盖各里程碑,全量回归以 `cargo test` 全绿为准;
- `unsafe_code = "forbid"` 只约束 workspace 自有 crate([lints] workspace = true),**不覆盖依赖内部**(如 libsqlite3-sys bundled 编译的 SQLite C 代码)——这是 lint 机制事实,信任边界=依赖供给链(bundled SQLite 于 M2/M8 验收,FTS5 启用);
- Provider 两条执行通道:同步 `CapabilityProvider`(进程内快路径,内联执行+panic 收容)与异步 `AsyncCapabilityExecutor`(MCP 等慢外部,spawn+超时钳制+可取消)共用 Broker 决策管线,`registry.is_async()` 分道——耗时能力必须注册为异步,否则占死单写者循环。

## 5. 文档操作纪律

- 大段内联脚本(python heredoc)写文件易**静默失败**——先 Write 成文件再执行;
- 基线 §1-§24 编号是被 adr/、milestones/ 大量引用的**硬锚点**(§13.5/§17/§18/§19 最密)——重排基线前先 `grep -rn "基线 §" adr/ milestones/`;
- 给基线增补:**熔入正文+标注 ADR 编号**,不挂追加式引注块(ADR-0015);补丁里不要写行号引用(必然漂移);
- 里程碑/合同文件是历史记录,不回头改写;新事实进 BACKLOG/SETTLED/新 ADR,交付全史=git tag+提交说明(ADR-0027)。

## 6. 已废止/已迁移速查(查旧资料前先看这里)

| 旧事物 | 现状 |
|---|---|
| BOEN_CLI_WEB、cli.html、/admin/cli | 已废止/删除(A-06~A-08 销账) |
| dsh 复刻前端 runtime/web(137 文件) | 已删,归档分支 `archive/m10-dsh-frontend`;继任=runtime/webapp |
| bm-surface-http/src/api_dsh.rs(dsh 宿主协议 /api/*) | 已删(已追认闭合) |
| 桌面壳 web/src-tauri | 已迁 `shell/tauri/src-tauri`(frontendDist 指 runtime/webapp/dist) |
| config_store(dsh 线归档) | W2 起按 ADR-0012 口径接回(config/model.json,文件>env) |
| 天机阁/deepseek-v4-flash 中转 | 已清;现用 OpenCode Go mimo-v2.5(zen 网关) |
| code-tools 随包插件 | 已退役(ADR-0021 内置化为内核 fs.* 工具集) |
| context-mode 插件名 | 已更名为 context-inspector(独立 Rust MCP 项目) |

## 7. 仓外关联资产

- ~~`D:\96_CoderWorld\boenmind-mcp-servers` 独立仓~~ **已移入主仓**(2026-09-02):插件源码=`plugins/mcp/web-multisearch/`,外仓历史归档分支 `archive/boenmind-mcp-servers`,外仓目录已删;
- `.tools/`:本机评估用 assistant-ui 上游克隆(不入 git);
- MCP 插件探活:webadmin `/admin/mcp/status`、`/admin/mcp/reload`(新增免重启,修改/删除需重启)。
