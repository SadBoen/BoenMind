# BoenMind — AI 工作规程(新会话必读)

## 这是什么

BoenMind:个人生态的 AI Runtime / AI OS,当前为**阶段一(跨平台单软件)**。
设计已定稿,经三模型辩论复核(`adr/`)与三真实系统对照验证
(Erlang/OTP、Kubernetes、VS Code,见 `architecture/deepwiki-validation.md`)。
合同库冻结 v1.0(字段只增不破)。

**当前状态(2026-09-02)**:W4b 全部闭合(多角色/Skill 挂载/对话内审批)+
工程债五项(F-01~F-04/F-10)+ 架构模型回写(F-06)+ Playwright 冒烟套件,
291 测试全绿,validate.py 全绿。**下一步 = 真实使用反馈期**(使用一周后解锁
远程 MCP);F-9/回看项随下一里程碑。交付全史见 `milestones/HISTORY.md`;
**欠账唯一入口 = `milestones/BACKLOG.md`**。

## 文件地图(规格分层)

```text
BoenMind-CORE-ARCHITECTURE.md   第 0 层  架构基线:原则/边界/不变量;§17 裁决;§18 里程碑定义;§19 回看制度
adr/                            第 0 层  架构决策记录 ADR-0001..0015(0012 随 M10 dsh 线归档、编号跳空;基线与 ADR 冲突时以更新的 ADR 为准)
architecture/                   第 0 层  C4 模型 boenmind.c4(拓扑唯一权威)+ 辩论转录(debates/)+ 验证报告
boenmind-contracts/             第 1 层  机器可读合同(v1.0 冻结)+ validate.py 校验器 + m0/(测试矩阵/威胁模型/perf-baseline)
milestones/                     第 2 层  实现规格+回看(M1-M9、W1-W4)+ 台账四件:HISTORY(交付时间线)/BACKLOG(未结事项)/PENDING(待裁决,现清零)/AUDIT-2026-08-30(审计)
runtime/                        第 3 层  Rust workspace 9 个 crate(bm-contract/core/persist/providers/cli/surface-http/runtime/judge/testkit)+ webapp(W 序列前端,Vite+React+TS)
apps/                           第 3 层  真实 App:wiki_server/market_server(stdio MCP,Python)+ mcp-config.example.json
shell/tauri/                    第 3 层  Windows 桌面壳(Tauri v2;frontendDist 指 runtime/webapp/dist,手工构建)
scenarios/                      实测    CLI 场景实测清单(S1-S10 与 2026-08-30 实测记录)
PLAYBOOK.md                     附页    实操备忘:启动与环境变量/前端四坑/浏览器自动化怪癖/废止速查——动手前先看
.agents/skills/boenmind-dev/    技能    按任务类型的操作清单(动合同/发 ADR/实现里程碑必加载)
.github/                        CI      contracts-validate + Rust 三平台矩阵(fmt/clippy/nextest)+ release
```

## 新会话工作流

1. 读本文件 → 2. 按手头任务读对应层文件(任务-文件对照见 boenmind-dev 技能)→
3. 动工前看 BACKLOG 确认没有已登记的相关欠账 → 4. 产出后自检
(合同有变更必跑 `python boenmind-contracts/scripts/validate.py`,须全绿)。

## 硬纪律(违反 = 返工)

1. **合同冻结**:boenmind-contracts/ 字段只增不破;删字段/改名/改语义 = Major,走基线 §13.5。
2. **先改模型再改文字**:架构变更先改 `architecture/boenmind.c4`;文字图与模型不一致以模型为准。
3. **决策写 ADR**:新决策在 adr/ 发新文件(编号递增),不修改既有 ADR 的语义;对基线的增补**熔入正文并标注 ADR 编号**,不挂追加式引注块(ADR-0015)。
4. **权限以合同显式化**(ADR-0006):未列入注册合同的权力视为不存在。
5. **里程碑 = 可运行检查点**(§18/§19):P0 测试套件全绿才算完成;完成后按 §19 回看再进下一个;交付状态登记 HISTORY,遗留登记 BACKLOG。
6. **真实进度只认 git**:主干应始终可校验(validate.py 全绿);提交说明写清动机。
7. **用户可见面必须真实浏览器手测**(2026-09-01 用户明示):以页面可见内容/截图为证;接口测试全绿 ≠ 界面交付。

## 工作方法(已固化,每轮沿用)

1. 强耦合任务合批,一轮交付、共享全量回归,依赖链顺序不变;
2. 文档类产物(与代码文件零相交)派后台子代理并行起草,主代理收圈时随手提交;
3. **不做同仓多代理并行写代码**(runtime.rs 单点合并成本 > 收益;Rust target 目录锁/冷编译),防冲突规程与单写者纪律不破。

## 环境与工具

- Rust 1.98 / Node 24 / Python 3.13;gh CLI 已装;tauri-cli 未装(桌面壳 = `shell/tauri` 手工构建,见其 README);
- 合同校验:`python boenmind-contracts/scripts/validate.py`(提交前置);
- 性能测试命令、启动命令与 BOEN_* 环境变量表 → `PLAYBOOK.md` §1/§4;
- context7 MCP 可用(库文档查询;真实 Provider/MCP 接入时优先用);
- MCP 插件源码在独立仓 `D:\96_CoderWorld\boenmind-mcp-servers`(仓外);
- 运行时配置在**数据目录**(默认 `%APPDATA%\Roaming\boenmind\config\`),不在仓库;
- 已确认:libsqlite3-sys bundled 默认启用 SQLITE_ENABLE_FTS5——M5 memory 检索 FTS5 实际生效,LIKE 仅兜底。

## 最高频坑(全表见 PLAYBOOK)

1. 事件信封 JSON 字段名是 `type`(serde rename),不是 `event_type`;
2. 静态页无缓存头,发版后 Ctrl+F5;内联脚本语法错误整页静默失效——改完必须 `node --check`;
3. 大段内联脚本(python heredoc)写文件易静默失败——先 Write 成文件再执行。
