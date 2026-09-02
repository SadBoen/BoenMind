# 外部 AI 评审(合并版)复核存档 — 2026-09-02

> 来源:用户转交的外部 AI 全面评审报告(前端+后端+接口+插件/技能/App 体系,自评 7.0/10)。
> 本文件只做**逐项复核结论存档**;欠账登记唯一入口仍是 `milestones/BACKLOG.md`(§4 P3 行、§7、§8)。
> 复核方式:逐条对照当日代码(main @ db27dff 之后)。规程依据:审计报告处理流=逐项复核属实再修,严重度自己重估。

## 一、总结论

1. **报告的前端部分基于旧快照**:其 P0/P1 主体(CSS 变量双轨、本地 Button、surfaces.tsx 死代码、
   背景图 XSS、localStorage 魔法字符串、Composer raw fetch)已在同日两轮治理
   (2c53a26 美学整构 + db27dff 工程治理闭环)修掉并登记 HISTORY——报告成稿早于这两笔提交。
2. **后端四项指控全部属实**,本轮处置:修二(roles.rs 循环内重复读盘、openai_compat 静态会话表),
   余登记 BACKLOG(stderr 直通、webadmin 拆分并入 P3)。
3. **架构建议(Skill v0.2/内置能力 MCP 化/App 产品面)是里程碑级设计决策**,不随手实施,
   全部登记 BACKLOG §8 候选,动合同前须用户裁决 + ADR。
4. 报告对安全模型(Broker 七步/风险五级/Grant 语义/Epoch)与代码质量的正面评价与仓内现状相符。

## 二、BUG 清单 11 条逐项复核

| # | 报告指控 | 复核结果(2026-09-02 代码) | 处置 |
|---|---|---|---|
| 1 | styles.css `--radius` 双重定义 | **已修**(db27dff):现全文单一 `:root` 块、单一 `--radius` 源 | 已闭合 |
| 2 | thread.tsx 本地 Button 同名异 API | **已修**(db27dff):本地定义已删,统一 `@/components/ui/button` | 已闭合 |
| 3 | themes.ts 背景图 URL 注入逃逸 | **已修**(db27dff):`sanitizeImageUrl` = 协议白名单(/themes/、http(s)、data:image/)+ 剥离引号/反斜杠/换行 + encodeURI;引号内 `)` 无法逃逸 url() | 已闭合(本轮复核确认修法扎实) |
| 4 | surfaces.tsx 全部死代码 | **已修**(db27dff):113 行死代码已清,现仅 9 行且被 file-tree/model-picker 实际引用 | 已闭合 |
| 5 | McpPage.tsx 714 行需拆分 | 属实(现仍 714 行) | 已登记 BACKLOG §7(上轮) |
| 6 | webadmin.rs 1882 行需拆分 | 属实(现仍 1882 行) | **本轮并入 BACKLOG §4 P3 大文件拆分行**(与 broker/turn/task_ops/sqlite_state 同批缓办) |
| 7 | roles.rs 每个 skill 重复读 skills.json | **属实**:读取+解析位于挂载技能循环体内,读盘次数随技能数放大 | **本轮已修**:提到循环外整体只读/解析一次(语义不变,294 测试绿) |
| 8 | mcp.rs:395 子进程 stderr `inherit()` 泄露敏感信息 | 属实存在,但系 W2 刻意诊断选型(注释写明「子进程报错直通 server.log」);单用户本地形态下 stderr 归用户自有日志,严重度降为低 | 登记 BACKLOG §8(低):后续可采集入插件页日志面 |
| 9 | openai_compat.rs 静态 OnceLock session_map | **属实**:进程级静态绕过 AppState,两处调用点本就持有 `state` | **本轮已修**:归入 `AppState.v1_sessions`(随路由生灭、测试隔离),语义不变 |
| 10 | builtin.rs 编译期硬编码无法热插拔 | 属实,但为阶段一刻意选型(同步快路径,trait 注释写明约束,见 BACKLOG P5) | 不单独动;与「MCP 完整热插拔」「内置能力 MCP 化评估」一并登记 BACKLOG §8 |
| 11 | App.tsx localStorage 魔法字符串 | **已修**(db27dff):统一 `lib/storage.ts` 键常量+安全存取器,余量仅注释 | 已闭合 |

## 三、其余前端/接口指控复核

| 指控 | 复核结果 | 处置 |
|---|---|---|
| Composer raw fetch 绕过 api.ts | 上轮已收敛;现存 raw fetch 仅 4 处且全部正当:SSE 流式(runtime.tsx,JSON 包装器不适用)、停止端点、/v1/models 与 /health 轻量 GET | 不再追 |
| CustomEvent 无类型安全 | 现存 5 文件 `new CustomEvent`、12 文件事件监听 | 低优,随 ESLint/TS 严格化批次(§7)一并 |
| 字体 9 档混乱 | 2c53a26 已收敛;styles.css+theme.css 现存 8 档(11/11.5/12/12.5/13/15/17/20,其中 12.5 系输入框下拉刻意层级) | 低优,后续并档 |
| 硬编码像素绕过 --radius | 现存 0 处(tsx 内 border-radius 硬编码) | 已闭合 |
| theme.css 玻璃段 4 处 !important | 属实(2c53a26 毛玻璃化刻意为之) | 登记 BACKLOG §7(低) |
| /admin/* 未入合同 | 已有裁决:/admin 暂不入合同,F-06 已把管理面回写 C4 模型 | 维持既有裁决 |
| /admin/context 形状无合同 | 同上(admin 面口径) | 维持既有裁决 |

## 四、架构建议处置(里程碑级,全部登记 BACKLOG §8)

| 报告建议 | 复核意见 | 处置 |
|---|---|---|
| 定义 Skill v0.2(references 按需加载/版本/scripts) | **部分有理**:按需加载与版本字段低成本高价值;但脚本执行是新执行面,必须先过 Broker 安全管线(七步管线/风险五级如何覆盖脚本)再动合同——合同冻结纪律,不可顺手做 | BACKLOG §8 候选,**动工前须用户裁决 + ADR** |
| 内置能力全量封装为本地 MCP server | **倾向不采纳**:同步 Rust trait 快路径是刻意选型(零进程开销,P5 已注释约束);全量 MCP 化让每次 memory 写入等内置操作都过子进程,单用户场景属倒退。热插拔的真实诉求由下条满足 | BACKLOG §8 评估项(倾向不采纳,留 M 系列回看) |
| MCP 完整热插拔(修改/删除免重启) | **属实欠账**:现 mcp_reload 仅新增生效,修改/删除仍需重启(MCP 插件轮已知遗留,此前未入册) | BACKLOG §8,**本轮补登记** |
| App 产品面(manifest ui_panels 自动导航/数据域) | 方向与「个人生态」愿景一致,但属远期里程碑(需 App manifest 合同+前端动态路由+数据域隔离) | BACKLOG §8 候选(远期) |
| 所有能力统一走 MCP 三阶段路线图 | 远程 MCP 阶段与既有 DEFERRED 项(使用一周后开工)重合,方向一致 | 维持既有节奏,不另立 |
| MCP 规范对标(elicitation/sampling/tasks/OAuth 缺口) | 对标表有参考价值;缺口多为远期,OAuth 已有认知 | 本文件存档备查,远程 MCP 开工时对照 |
| 立即该做三件事 | ①CSS 统一=已完成;②Skill v0.2 合同=须先裁决(见上);③内置 MCP 化=倾向不采纳(见上) | 见各条 |

## 五、顺带发现(本轮新增)

- 上轮 fs_rename/fs_download 提交未过 `cargo fmt`(本轮 `cargo fmt` 收纳 26 行,纯格式)。
- `cargo clippy --all-targets` 存量警告 14 处(about.rs 12 / webadmin.rs 2,均为 needless_borrow 类,
  非本轮引入,疑在 `#[cfg(test)]` 代码)——CI 矩阵口径与本地 --all-targets 存在盲区。
  登记 BACKLOG §8(低)。

## 六、测试证据

- `cargo test --workspace` 全绿(294 项,含 bm-core 79 / bm-surface-http 25 / 合同 enum 比对 16);
- `cargo fmt` 后无 diff 回流;`cargo clippy` 本轮改动三文件零警告;
- `boenmind-contracts/scripts/validate.py` R1–R4 全绿(无合同变更)。
