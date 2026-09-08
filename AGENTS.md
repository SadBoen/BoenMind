# BoenMind — AI 工作规程(新会话必读)

## 这是什么

BoenMind:个人生态的 AI Runtime / AI OS,当前为**阶段一(跨平台单软件)**。
合同库冻结 v1.0(字段只增不破)。

**当前版本 = v0.0.14(已发版)**,此后批次落 main 未打 tag。进度只认 git:
交付全史 = git tag+提交说明(ADR-0027,不另立时间线文件);
欠账唯一入口 = **GitHub Issues**(标签 P0/P1/P2/tech-debt/deferred);
架构铁律唯一查重清单 = `docs/architecture/decisions.md`。

## 文件地图(规格分层)

```text
BoenMind-CORE-ARCHITECTURE.md   第 0 层  架构基线:原则/边界/不变量;§17 裁决;§18 里程碑定义;§19 回看制度
adr/                            第 0 层  架构决策记录 ADR-0001..0029(基线与 ADR 冲突时以更新的 ADR 为准)
architecture/                   第 0 层  C4 模型 boenmind.c4(拓扑唯一权威)
boenmind-contracts/             第 1 层  机器可读合同(v1.0 冻结)+ validate.py 校验器
docs/architecture/decisions.md  第 0 层  架构铁律唯一查重清单(15条,评审/审计前必读)
docs/development/PITFALLS.md    附页    实操备忘+高频坑唯一源(启动/前端四坑/浏览器自动化怪癖)
.ai/context.md                  附页    AI 协作核心纪律+回归清单
GitHub Issues                   台账    未结任务/技术债唯一入口(P0/P1/P2/tech-debt/deferred标签)
runtime/                        第 3 层  Rust workspace 9 个 crate + webapp(Vite+React+TS)
apps/                           第 3 层  真实 App:wiki_server/market_server/music_server(stdio MCP)
plugins/                        第 3 层  官方随包插件:web-multisearch、context-inspector
shell/tauri/                    第 3 层  Windows 桌面壳(手工构建)
.agents/skills/boenmind-dev/    技能    按任务类型的操作清单
.github/                        CI      contracts-validate + apps 冒烟 + webapp lint + Rust 三平台矩阵 + release
```

## 新会话工作流

1. 读本文件 → 2. 读 `.ai/context.md`(硬纪律+回归清单)→
3. 动工前查 `docs/architecture/decisions.md` 避免重复踩已裁决问题、`gh issue list` 确认没有已登记的相关欠账 →
4. 产出后自检(合同有变更必跑 `python boenmind-contracts/scripts/validate.py`,须全绿)。

## 硬纪律(违反 = 返工)

见 `.ai/context.md`(合同冻结/决策写ADR/权限显式化/里程碑=可运行检查点/真实进度只认git/用户可见面真实浏览器手测/规范与叙事分离)。

## 评审纪律(外部评审/回头看/审计任务必读)

**必读清单**:①本文件 → ②`docs/architecture/decisions.md`(架构铁律,勿重复报误报区)→ ③`gh issue list` 查未结项。

1. 新意见提出前先查 `decisions.md`:已有结论不得重提;翻案须带新证据并发新 ADR。
2. 属实才修,误报必驳;每驳回一条视情况补入 `decisions.md`(15条上限,超出淘汰最久未被撞到的)。
3. 评审类一次性报告不入库:结论进 `gh issue` 或 commit message,原文留 git 史。

## 环境与工具

见 `.ai/context.md`。
