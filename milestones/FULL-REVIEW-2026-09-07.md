# 全面回头看审计 v2(FULL-REVIEW 2026-09-07)

> 定位:资深工程师视角的第二轮全面回头看——架构/分层/前后端结合/逐条业务线/冗余/测试健康/工程卫生。
> 范围:全仓(含自带插件 `plugins/mcp/web-multisearch`、`plugins/mcp/context-inspector`)。
> 处置:属实且无歧义的缺陷当轮即修(批次见 §8 与 git 提交);需裁决项列 §7;其余登记 `BACKLOG.md`(来源=本文件)。
> 复核口径:全部修复经源码逐行人工复核 + 构建/测试验证;未动合同面(boenmind-contracts 冻结 v1.0)。

## 1. 总体结论

- **架构分层继续健康**:bm-contract ← bm-persist ← bm-core ← {bm-cli, bm-runtime(bm-providers/bm-surface-http)} 单向依赖成立;F-12 依赖倒置已在上一轮收口(bm-core 不再依赖 bm-persist);bm-persist → bm-core 的单向复用弧合理。
- **契约纪律持续良好**:合同 JSON 与 Rust 枚举双向漂移锁(sync.rs)在;`new_unchecked` 限测试;本轮未触碰合同面。
- **真实缺陷收敛**:相比 09-05 轮(7 高危),本轮高危面已大幅缩减;主要问题集中在**前端静态分析真空**(ESLint 规则未真正加载导致的静默失效)与**测试环境确定性**。
- **前端是本轮主战场**:react-hooks 插件此前未安装,`eslint-disable` 注释引用了不存在的规则(静默失效);启用后暴露 30 个真实问题,其中 1 个 `rules-of-hooks` 违规(条件调用 Hook)为**潜在运行时崩溃级缺陷**。

## 2. 已核实并当轮修复(高危/行为)

| # | 位置 | 缺陷 | 修复 |
|---|---|---|---|
| H1 | bm-runtime/tests/m3_e2e.rs t32 | 测试对宿主环境 `BOEN_MODEL_STREAM` 敏感:宿主设流式=1 时事件流多发 `model.content.delta`,跨进程断言 7 条变 8 条失败——CI/本机构建环境差异即触发 | spawn_server 增加 `sanitized_env()`:剔除全部 `BOEN_*` 变量显式继承,测试确定性锁定;流式选项由测试自身可控参数决定 |
| H2 | webapp thread.tsx:195 | **rules-of-hooks 违规**:`useAuiState` 在 JSX 条件渲染分支内联调用;会话/消息切换时可触发「Rendered more hooks than during the previous render」运行时崩溃 | 提取到组件顶层 `const agentRunning = useAuiState(...)`,JSX 引用变量 |
| H3 | webapp eslint.config.js | `eslint-plugin-react-hooks` 未安装,但代码里有 `// eslint-disable-next-line react-hooks/exhaustive-deps` —— 规则不存在 = disable 静默失效,exhaustive-deps 实际从未把关 | 安装插件并启用 recommended 规则;`set-state-in-effect`/`immutability` 按「React Compiler 迁移预告」降 warn(存量合法惯用法如 effect 拉数据不盲目重构,避免行为风险) |
| H4 | webapp thread.tsx | 死代码残留:`parseAssistantText`/`ToolGroupCard`(旧解析器,已被 parser.ts + ToolTreeGroup 取代)、`getFileIcon`(职责已被 FileBadge 取代) | 删除死函数 + 冗余图标导入 |

## 3. 已核实并当轮修复(前端静态分析—启用后暴露的真实问题)

| # | 文件 | 问题 | 修复 |
|---|---|---|---|
| F1 | ToolTreeGroup.tsx | 未使用 `useRef/useEffect/UIEvent/FileCode`;接口 `isRunning` 解构未用;`isSearch` 声明未用 | 清理导入;`_isRunning`;删 isSearch |
| F2 | AgentStatusBar.tsx | 未使用 `CheckCircle2` | 清导入 |
| F3 | TerminalBlock.tsx | `isRunning` 解构未用;`handleScroll(e)` 的 e 未用 | `_isRunning`;`_e` |
| F4 | ThinkingBlock.tsx | `handleScroll(e)` 的 e 未用 | `_e` |
| F5 | thread.tsx | 未使用 `ChevronRight/Search/Terminal/FileText/FileCode/FileJson/FileImage/File` | 清导入 |
| F6 | WorkspaceFiles.tsx | 解构了 `expanded` 未用(useLazyTree 返回值) | 移除解构 |
| F7 | file-tree.tsx | `childrenRef.current = children` 渲染期写 ref(React Compiler 规则) | 移 useEffect 同步 |
| F8 | PluginsPage.tsx | `colWidthsRef.current = colWidths` 渲染期写 ref | 移 useEffect 同步 |
| F9 | ServerConfigDialog.tsx | effect 内调用后声明函数 `finalizeSelection`(编译器 immutability)+缺依赖警告 | 内联到 effect,消除声明序与依赖问题 |
| F10 | context.tsx | `stats` useMemo 复杂派生,React Compiler 无法保留 memo(信息性) | 标注 `preserve-manual-memoization` disable + 注释原因 |

## 4. 已核实并当轮修复(持久化/文件面)

| # | 位置 | 缺陷 | 修复 |
|---|---|---|---|
| P1 | bm-providers/fs_tools/ops.rs `write` | `std::fs::write` 原地覆写:进程崩溃/断电留半截文件(关闭 BACKLOG「fs.write/edit 原子写+大小上限」) | 改 `bm_persist::atomic_write`(临时文件+fsync+rename,全仓标准);新增 `MAX_WRITE_BYTES=16MB` 写入上限(与读取上限对等) |
| P2 | bm-providers/fs_tools/ops.rs `edit` | 同上:编辑结果直接 `std::fs::write` 覆写 | 改 `bm_persist::atomic_write` |

## 5. 工程卫生

- **清理临时脚本**:`runtime/_f12_step1.py`(F-12 一次性重构脚本)、`runtime/_split_generic.py`(文件拆分工具)——标注「用完即删」且使命已完成,从仓库移除。
- **测试/源码分离现状确认**:`bm-testkit` 为专用测试 crate(不进生产二进制);`webapp/e2e/` 独立 Playwright 冒烟套件;各 crate `#[cfg(test)]` 与 `tests/` 隔离——符合要求,未改动。
- **e2e 测试对齐新 DOM**:冒烟测试 `工具调用结构化折叠渲染` 锚定已删除的旧组件 `ToolGroupCard`(`data-slot="tool-group"`);对齐为当前 `ToolTreeGroup` 的实际文本/结构(read 分类渲染 FileBadge 显示路径、search 分类显示工具名)。

## 6. 本轮验证矩阵(全部通过)

| 验证 | 结果 |
|---|---|
| `cargo check --workspace` | ✅ 0 错误 |
| `cargo clippy --workspace --all-targets` | ✅(既有基线) |
| bm-core 87 tests / bm-providers 56 tests / bm-persist / bm-contract / bm-judge | ✅ 全绿 |
| bm-runtime m3_e2e t32(带 BOEN_MODEL_STREAM=1 宿主) | ✅ 修复后稳定通过 |
| bm-surface-http 22 tests(webadmin+web_serve) | ✅ 全绿 |
| bm-testkit m7_mcp 4 tests | ✅ 全绿 |
| fs_tools 28 tests(含 write/edit 路径) | ✅ 全绿 |
| webapp `npm run build` | ✅ 构建成功(1MB chunk 为既有大小,见 §7) |
| webapp `npm run lint` | ✅ 0 errors、15 warnings(均为刻意保留的 set-state-in-effect 合法惯用法) |
| webapp `npm run test:smoke`(Playwright 10 用例) | ✅ 10/10 通过 |
| plugins/web-multisearch check+test | ✅ 全绿 |
| plugins/context-inspector check+test | ✅ 全绿 |

## 7. 遗留与需拍板项(本轮不动)

1. **前端主 chunk 1MB**(>500KB 警告):未做代码分割。属「不增加功能」边界内可做的打包优化,但改动 vite 配置有风险,登记候选;建议下一轮用 `manualChunks` 或动态 import 拆分。
2. **set-state-in-effect 15 处**:React Compiler 迁移预告警告,对「effect 内拉数据后 setState」「计时器归零」等官方认可惯用法误报;存量不改(避免行为风险),新代码按编译器建议收敛。
3. **BACKLOG 既有 OPEN 项**(system.exec cwd schema 显式化、MCP 装配下沉、async 执行器排队假性超时等):延续既有台账,不在本轮重复处置。
4. **门户 logout / Cookie Secure**:延续 BACKLOG §4 DEFERRED,建议随下次发版。

## 8. 修复批次

- 批次一:测试环境确定性(H1)+ 临时脚本清理;
- 批次二:前端 lint 16 错误 + react-hooks 插件启用(H3/F1-F6);
- 批次三:rules-of-hooks 违规修复(H2)+ 死代码删除(H4);
- 批次四:React Compiler 规则问题(F7-F10);
- 批次五:fs.write/edit 原子写 + 大小上限(P1/P2)。