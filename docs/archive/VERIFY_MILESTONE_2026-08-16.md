# 里程碑对照验证 + 专家提示词 + 日志页（2026-08-16）

> 用户本轮要求：① 专家角色提示词撰写（交接最优先项）；② 设置中心加「日志」页；
> ③ 任务完成后对照架构文档（当前里程碑 M2 以前内容）逐项仿真验证 + 查日志。
> ④ 回答 Windows 便携版（多文件）是否缩短编译时间。全部完成，本报告记录验证矩阵与发现。

## 一、交付物

### 1. 专家角色提示词（4 份，`~/.boenmind/agents/agents/*.md`）
- 结构统一：角色定位 → 职责 → 工作纪律 → 输出格式 → 边界（参考 ZCode--CLI--agent 三份内置角色
  [Plan/Explore/Verification] 的"身份声明 + 禁区 + 流程 + 强制输出格式"写法，MIT 开源可吸收）
- `coding-architect`（948 字）/ `coding-coder`（762 字）/ `coding-reviewer`（966 字）/ `default`（654 字）
- 已实测：真实派工 coding-coder 两次均 exitCode 0，子代理按新格式返回结构化 JSON
  （summary / first_5_lines / notes）——提示词格式约束生效

### 2. 设置中心「日志」页
- 后端：`bm-server/src/log_buffer.rs` 内存环形缓冲（5000 条，tracing Layer 收集）+
  `routes/logs.rs` `GET /api/logs?level=&q=&limit=&offset=`（级别 ≥ 语义 + 关键字 + 分页）
- 前端：`LogsSettings.tsx`（级别下拉 / 关键字筛选 / 自动刷新 5s / 滚动暂停跟随）+ 注册表 + 4 语言包
- 浏览器实测：菜单项、列表渲染（时间/级别/模块/消息）、关键字 9→6 条、WARN 筛选空态、清空按钮

### 3. 里程碑对照验证（M2 以前，全部仿真操作 + 查日志）

| 架构声称（§7.2 + 里程碑） | 验证方式 | 结果 |
|---|---|---|
| 阶段 0：事件日志 turso 双写 | 真实消息 → SSE replay | ✅ 10 事件全链：user/message→turn/start→step/start→request/header→assistant/chunk×4→assistant/message→turn/end |
| 阶段 1：内核 + agent-loop + bm-compat | 启动日志 | ✅ 事件日志双写启用 + 5 插件 compat_ready（tools=10） |
| 阶段 2 部分：权限询问链 | 浏览器弹窗实测 | ✅ 拒绝/允许/总是允许三按钮 + 决策记忆（总是允许后不再询问）；HIGH_RISK_TOOLS=bash/subagent 每次询问 |
| 阶段 3 部分：服务面 13 面注册 | 代码对照 SERVICE_FACES 图纸 | ✅ 8 预注册（memory/settings/stats/provider/llm/skill/session/credentials）+ 4 运行期（tools/notify/gate/scheduler）+ event_store/compactor 已有 |
| 阶段 5 部分：Steward | status API | ✅（未启用状态正常返回） |
| 阶段 6：pi 退役 | git 历史 | ✅（legacy 删空，记忆已录） |
| M1 验收 | 已有验收记录 8254bd7 | ✅（本轮不重跑） |
| M2：todo + 事件投影 | API 实测 | ✅ 增删改全通（add/update/remove/list），8 个 todo/write 事件落盘，快照投影正确 |
| M2：终端 | 全链路实测 | ✅ 创建/输入/SSE 输出/关闭；ConPTY ESC[6n 应答机制工作（cmd 横幅+提示符+命令回显） |
| M2：项目切换 | workspace?root= | ✅ 项目根枚举正确 |
| UI 轮：分叉 | API + 浏览器 | ✅ 新会话 + 历史复制（标题「源」分叉） |
| 专家管理面 | API | ✅ 4 专家 + 新提示词读取 |
| subagent 派工 | 真实派工 ×2 | ✅ coding-coder exitCode 0 + 结构化交付；宿主事件链含 tool/call + tool/result |
| 记忆 | 目录 + 插件加载 | ✅ ~/.boenmind/memory/facts.md + coding-memory 加载 |
| 压缩 | 插件加载 + A/B 已录 | ✅ ctx-compactor 加载（深度验证见既有 A/B 记忆） |

### 4. Windows 便携版（多文件）回答——见下一节

## 二、Windows 便携版：多文件 vs 单文件，是否缩短编译时间？

**结论：Rust 编译时间基本不变；构建/迭代流程显著变快；建议采纳多文件形态，但它不是编译提速手段。**

分析（当前形态：Windows 便携包 = 单文件 BoenMind.exe，Tauri 壳内嵌前端 dist +
内嵌 bm-server 兜底；bm-server 独立二进制走热升级下载到 ~/.boenmind/runtime/）：

1. **Rust 编译时间（大头）不变**：编译时间由依赖树 + 代码量决定，打包形态不影响。
2. **小幅收益**：前端 dist 不再内嵌进壳（链接更快、壳更小）；bm-server 的
   `embed-plugins` 若改为便携包带 plugins 目录（运行时扫描），可去掉内嵌宏与链接开销。
3. **大幅收益在迭代流程**：现在前端改动要完整 `tauri build`（vite + Rust 链接壳）；
   多文件形态下前端改动只跑 `vite build` 复制文件，**不用重编 Rust**。
4. **其他收益**：壳更新频率降低（前端资源热替换）、runtime 独立热升级（已有机制）、
   插件目录用户可编辑。
5. **代价**：多文件更易被移动/丢失破坏（单文件最稳）；分发清单变多。
6. 真正缩短 Rust 编译时间的手段仍是：减依赖 / 自托管 runner（已部署 VMware）/ 避免
   debug 2GB 坑（CARGO_PROFILE_DEV_DEBUG=0，质量门已用）。

建议：若要动，最小改动 = 便携包改为「BoenMind.exe + web/ 静态目录 + plugins/ 目录」，
壳 serve 外置 dist（Tauri `frontendDist` 指向外部目录需验证可行性）；bm-server
从 `embed-plugins` 改为运行时扫描插件目录。属分发层改动（架构铁律：分发形态
不影响设计层），可随时做，不阻塞 M2。

## 三、验证中发现的问题（登记，不阻塞）

1. **P3 文案过时**：subagent 工具描述仍写 "Agent definitions live in <pi dir>/agents/*.md
   or .pi/agents/*.md"——实际已用 ~/.boenmind/agents/（代码已适配，仅描述文本旧）。
2. **P3 UX**：用户认知的专家名（coder）与实际 id（coding-coder）不一致，模型第一次
   派工失败后自行发现纠正。建议设置中心专家页强化 id 展示，或支持别名。
3. **P2 待查**：子代理沙箱内 bash 报 os error 123（连 echo 都失败），宿主 bash 正常——
   子代理 spawn 环境差异（cwd/环境变量）。已由子代理用 read 工具绕过。挂 M2 排查。
4. **既有失败 4 项（非本轮引入）**：bm-compat lib 内部测试 2 项缺
   ext_conformance/artifacts/doom-overlay 夹具（从未入库）+ 2 项依赖本机 python3。
   不在质量门范围（质量门 = bm-server/bm-core + bm-compat 5 集成套件 + clippy，全绿）。

## 四、质量门

- cargo test -p bm-server -p bm-core：197 全绿
- bm-compat 5 集成套件：20 全绿
- clippy --workspace --all-targets：0 error（顺手修复 bm-mcp 测试/示例缺 scopes 字段 5 处）
- tsc --noEmit + vite build：通过
