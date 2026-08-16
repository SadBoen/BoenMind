# 前端专项三工具交叉审查（2026-08-16）

> 本轮对**前端**做专项评估（上轮 2026-08-16 为全库三工具交叉审查，见 `REVIEW_TOOLS_CROSS_2026-08-16.md`）。
> 三个审查工具各自独立出报告，本文件为交叉验证结论 + 合并修复清单。
> 独立报告：`docs/review-frontend-2026-08-16/`（report-01 code-review / report-02 code-architecture / report-03 ln-24-audit）

## 一、工具与分工

| 工具 | 技能 | 报告 | 发现数 | 侧重 |
|---|---|---|---|---|
| 工具A | code-review（codebase-reviewer） | report-01-code-review.md | 52 条（含精简专项 14 条） | 穷尽式质量审查 + **精简代码专项**；实证跑了 tsc/oxlint/grep |
| 工具B | code-architecture | report-02-code-architecture.md | 6 担忧 + 10 条 Simplicity Check | 架构评估（Step 3A）+ §四·C 插件化适配度 |
| 工具C | ln-24-architecture-auditor | report-03-ln24-audit.md | 10 条（P1×1/P2×5/P3×4） | 架构契约/边界/所有权审计；Verdict: FAIL |

三工具互相独立（未看彼此报告），本文件作者对关键结论做了独立实证（tsc 实跑、grep 全量核对、后端 SQL 追踪）。

## 二、交叉验证矩阵（按主题）

| # | 主题 | 工具A | 工具B | 工具C | 独立实证 | 结论 |
|---|---|---|---|---|---|---|
| 1 | **`tsc -b` 构建断裂**（ja/ko 缺 namePrefixHint + skin.ts 死常量） | BUG-001 Critical | — | — | ✅ **实测退出码 2，3 个 error** | **P0，坐实**。主分支 `pnpm build` 必失败，发布被卡。仅 A 抓到（B/C 未跑编译） |
| 2 | **en/ko `uninstall: "卸载"` 中文漏译** | BUG-002 High | — | — | ✅ 实测坐实，**且 ja.ts 同为"卸载"** | **P1，坐实但修正**：A 说"ja 正常"有误，实为 **en/ko/ja 三处**全漏 |
| 3 | **markdown 用 `prose` 但未装 @tailwindcss/typography** | BUG-003 High | — | — | ✅ 三处使用坐实，package.json 无此包 | P1，排版样式从未生效 |
| 4 | **死依赖 react-rnd / react-resizable-panels**（+ @tauri-apps/plugin-updater/process） | S-1 | P3 | F7 | ✅ src 全库零 import | P3，三工具全同意；清理轮 |
| 5 | **死组件 ChatWindow / ExpertTeamDocs** | S-2 | P3 | F9 | ✅ 全库零引用 | P3，三工具全同意（上轮 A-14 遗留未删） |
| 6 | **APPS.plugins/steward 应用不可达**（注册表 6 项 vs 导航 4 项） | ARCH-001 High | P3 | F8 | ✅ setActiveNav 唯一调用方在 ClassicShell，导航不暴露这两项 | P3 但架构口径问题，B 建议定调收口 |
| 7 | **viewMode/setViewMode 死状态**（桌面壳退役残留） | S-6 | P3（§1.4） | F8 | ✅ setViewMode 零调用者 | P3，三工具全同意；注意 localStorage 残留可把用户带到不可达视图 |
| 8 | **APP_LIST / AppEntry.defaultSize/gradient 死导出** | S-3 | P3 | F8 | ✅ 全库零消费 | P3，三工具全同意 |
| 9 | **#system-ui 宿主容器死引用**（dialog.tsx 回退 body） | BUG-012 Medium | — | — | ✅ 全库仅 dialog.tsx 提及，无创建者 | P3（行为正常但注释误导 + 未来踩坑面） |
| 10 | **reduceMotion 对特效无效（死分支）** | BUG-004 Medium | — | F2 P2 | ✅ 两分支体相同（A/C 独立引用同一行） | P2，A/C 双工具一致；30fps 照跑 + 与无障碍意图相悖 |
| 11 | **"蓝色波纹"动画不生效**（未解 bug） | BUG-006（新候选 A' backdrop-filter / L float32 精度 / B' 对比度 / C' 静默降级） | P5（按交接 2D 重写） | F5 P2（H1 合成层 / **H2 无帧心跳观测面** / H3 首帧 0 尺寸） | ✅ 与 HANDOFF_BG_EFFECT_ANIMATION 证据链一致 | P2 未决。三工具从不同角度补充候选；**H2（帧心跳）是唯一可由代码直接修的架构项**；默认开启坏功能（B 建议修前默认 none） |
| 12 | **SSE 解析三处复制**（chat/subscribeEvents/subscribeTerminal） | QUAL-002 | P1 | F6 P2 | ✅ 行号核实（728/810/858），401 行为已分叉 | P2，三工具全同意（上轮已报，本轮确认仍存在） |
| 13 | **同会话双 SSE 订阅 + 无重连**（TodoPanel/FilePanel） | BUG-009 Medium | — | F11 P3 | ✅ 两个面板各自 subscribeEvents；TodoPanel 无重连无手动入口 | P2（A 判 Medium、C 判 P3，合并 P2：web 部署下静默陈旧） |
| 14 | **fork 分叉把乐观消息 id 泄漏进服务器契约** | — | — | F1 **P1** | ✅ **坐实但修正失败模式**（见下 §三） | **P1（修正后）**，仅 C 抓到——本轮最有价值的独有发现 |
| 15 | **主题双轨**：config.theme 只写不读，next-themes localStorage 独立 | — | — | F3 P2 | ✅ loadConfig 只校正 lang 不校正 theme | P2，仅 C 抓到；与 lang 单源规则不对称 |
| 16 | **API 契约零校验**（纯注释对齐，已有 42eea24 实机故障先例） | — | — | F4 P2 | ✅ 无 zod/OpenAPI；注释对齐 | P2，仅 C 抓到 |
| 17 | **前端零测试 + 无 build CI 门槛** | IMP-001 Medium | — | — | ✅ src 无 test 文件；CI 未跑前端 build（否则 P0 早暴露） | P2（修复 P0 的治本手段：CI 加 `pnpm build`） |
| 18 | 权限模式映射两处实现不同步（设置页 vs 聊天工具条） | ARCH-003 | — | — | 未实证 | P3，仅 A |
| 19 | 终端大粘贴 `...data` 展开栈溢出静默丢数据 | BUG-007 | — | — | 未实证 | P3，仅 A |
| 20 | 三处 5s 轮询无退避（health/MCP/Steward） | PERF-001 | — | — | 未实证 | P3，仅 A |
| 21 | 死 i18n key ~24 个 ×4 语言 | S-9 | P6 | — | 未全量实证（A 做了静态引用核对） | P3 清理轮 + CI 加 key 完整性检查 |
| 22 | 皮肤 localStorage 键散落 10+ 无集中清单 | ARCH-005 | — | — | 未实证 | P3，仅 A |
| 23 | dockview 布局重置无确认（一键清用户布局） | BUG-011 | — | — | 未实证 | P3，仅 A |
| 24 | 特效无 visibilitychange 暂停（后台 1s 全屏 WebGL 重绘） | BUG-013/PERF-002 | — | — | 未实证 | P3，仅 A |
| 25 | 死分支 LogsSettings `paused ? "" : ""` | QUAL-007 | — | — | 未实证 | P4 顺手项 |

## 三、交叉验证修正（本文件作者的独立实证结论）

1. **修正工具C F1 的失败模式**（P1 保留，描述更正）：fork 传的是客户端乐观 id（`Date.now()`，≈1.75e12），后端按 `messages WHERE session_id = ?1 AND id <= ?2` 定位（服务器自增 id 为小整数）——`id <= 时间戳` 恒真，**不会"必然失败"，而是"静默复制整个会话"**。对"答复末尾分叉"（最常见操作）恰巧等于意图；但**对会话中途的乐观消息分叉会过度复制**（用户发多条消息后分叉较早的答复 → 整段历史全被复制进新会话）。历史消息（getSession 加载，服务器 id）路径正常 → 同功能双路径行为不一致的结论不变。修复方向不变：流结束后补 `getSession` 重拉使 id 与服务器一致，或 fork 契约改为按会话内序号定位。
2. **修正工具A BUG-002 的范围**：en.ts:571 与 ko.ts:571 之外，**ja.ts:571 同样是中文"卸载"**（三处非中文包全漏，非 A 说的"ja 正常"）。
3. **补充工具A BUG-001 的证据细节**：实测 `tsc -b` 退出码 2，3 个 error 均在预期位置（ja.ts:597 / ko.ts:597 缺 `namePrefixHint`、skin.ts:111 `SKIN_AUTO_KEY` 未使用）。上轮修复轮只跑了后端 cargo 检查，前端构建断裂未被发现——CI 缺 `pnpm build` 门槛是根因。

## 四、合并修复清单（按优先级）

| 优先级 | 项 | 工作量 | 说明 |
|---|---|---|---|
| **P0** | 修 `tsc -b`：ja/ko 补 `namePrefixHint` + 删 skin.ts 死常量 | 3 行 | 构建当前断裂，任何发布被卡 |
| **P0 配套** | CI/质量门加 `pnpm build` | 小 | 治本，防同类再漏 |
| **P1** | fork 乐观 id：流结束重拉 getSession（或后端改序号定位） | 小 | 功能语义错误（静默过度复制） |
| **P1** | en/ko/ja 三处 `uninstall` 漏译修正 | 3 行 | 顺带加"非 zh 包中文残留"扫描脚本 |
| **P1** | markdown 装 @tailwindcss/typography（或自写排版类） | 小 | prose 三处从未生效 |
| **P2** | 波纹动画：按交接路径 2D canvas 重写 + **补帧心跳观测面（H2）**；修前默认特效 none | 中 | 需真实浏览器验证（IAB 截图无效） |
| **P2** | reduceMotion 静态帧落实（3 行）+ visibilitychange 暂停特效 | 小 | 与上项同文件 |
| **P2** | 抽 `parseSSEStream` 统一三处（401 行为以 chat 为准） | 小 | 纯重构 |
| **P2** | 特效/订阅事件流补重连（指数退避）或订阅上移单源 | 中 | TodoPanel/FilePanel 断连静默陈旧 |
| **P2** | loadConfig 校正 theme + next-themes storageKey 收编命名空间 | 小 | 与 lang 单源对称 |
| **P2（可后置）** | API 契约防线：后端出 OpenAPI 或关键端点轻量校验 | 大 | 需与后端协商 |
| **P3 清理轮** | 死依赖 4（react-rnd/react-resizable-panels/@tauri-apps/plugin-updater/plugin-process）+ 死组件 2 + 死导出（APP_LIST/gradient/defaultSize/separator/DialogTrigger/DialogClose/viewMode）+ 不可达应用收口（plugins/steward 定调二选一）+ #system-ui 注释 | 半天 | 纯删除，三工具全部同意，无争议 |
| **P3** | 死 i18n key ~24×4 清理 + key 完整性 CI 脚本 | 半天 | 工具A 有完整清单 |
| **P3** | 权限映射收口 store（跨页同步）+ 设置对话框抽共享 hook | 中 | 重复逻辑 ×2 |
| **P3** | 皮肤 localStorage 键收口单文件 | 小 | 10+ 键散落 |
| **P3** | 三处 5s 轮询统一 usePolling（offline 降频） | 小 | health/MCP/Steward |
| **P4** | 终端大粘贴分块编码、布局重置加确认、无障碍 aria-label、LogsSettings 死分支、.oxlintrc 扩规则面 | 顺手 | 单项均小 |

## 五、结论

1. **前端整体质量高于同规模个人项目平均线**（B 的 8 条亮点：注册表模式、DockLayout 上游隔离、TodoPanel 事件投影实证、皮肤系统可逆工程、零 Tauri 代码双部署、状态单件化、i18n/令牌体系、设置分级）。**无大型过度设计**——B 的 Simplicity Check 判定最大的两笔投入（dockview 布局系统、皮肤系统）都有真实消费者与用户拍板背书，真正的"过度"是**残留**（死依赖/死条目/死组件）与**一处抢跑**（特效注册表，YAGNI 相悖）。
2. **当前最痛的是"没人跑过前端构建"**：P0 构建断裂 + 零测试 + CI 无前端门禁，三者同根。前端 1.6 万行目前唯一的静态防线是 oxlint（22 warnings 未清零）。
3. **对远期 §四·C（界面插件化）的适配度：容器与注册表已就位，契约与加载器未就位**。VIEWS/DockLayout 是意外的好起点（声明式视图槽雏形）；真正的工程量在 @boenmind/client 抽取（api/client.ts 947 行 + app-store.ts 898 行双单体的 SDK 化）与 bundle 加载形态。建议同域 bundle 起步（iframe 需整体迁出 APPS，成本差一个量级）。
4. **三工具互补性验证**：本轮 25 个主题中仅 1 项被三工具同时命中（SSE 复制），2 项被 A+C 同时命中（reduceMotion、波纹动画），多数为单工具独有——**穷尽式审查（A）、架构评估（B）、契约审计（C）三者缺一不可**；C 的 fork 缺陷（P1）与 theme 双轨（P2）是 B/A 均未覆盖的盲区，交叉验证修正了 A（ja 漏译范围）与 C（fork 失败模式）各一处描述。
