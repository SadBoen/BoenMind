# 全面回头看审计(FULL-REVIEW 2026-09-05)

> 定位:资深工程师视角的第三层(实现面)全面回头看——架构/分层/前后端结合/逐条业务线/安全/冗余/风格/测试健康/工程卫生 14 维。
> 处置:属实且无歧义的缺陷当轮即修(批次一/二/三,见 §8 与 git 提交);需裁决项列 §7 留待逐项拍板;其余登记 `BACKLOG.md`(来源=本文件)。
> 复核口径:全部高危项经源码逐行人工复核确认,非转述侦察;不确定项明确标注。

## 1. 总体结论

- **架构分层健康**:bm-contract ← bm-persist ← bm-core ← {bm-cli, bm-runtime(bm-providers/bm-surface-http)} 单向依赖,无循环;bm-judge 纯只读。World 上帝对象+单写者串行是既有设计,注释包袱重但行为无害。
- **契约纪律罕见地好**:契约 JSON 与 Rust 枚举双向漂移锁(tests/sync.rs ~1100 行)、事件键集注册表断言、`new_unchecked` 全部限于测试。本次全程未动合同面。
- **测试体系真实有效**:invariants/assert_event_stream_wellformed 等被 30+ 测试文件真实消费;`#[ignore]` 仅 11 处且全部合理(性能定标/实网门控)。
- **主要问题集中于**:①HEAD 056b6ee 审批错配根治的回归遗漏(测试未对齐+worker 审批豁免分类被打断);②持久化「宁可拒开」口径在运行期投影写路径不贯彻;③少数真 BUG 与两处前端假数据。

## 2. 已核实并当轮修复(高危)

| # | 位置 | 缺陷 | 修复 |
|---|---|---|---|
| H1 | bm-surface-http/src/portal.rs cookie_session | `strip_prefix(SESSION_COOKIE)?` 在首个非会话 cookie 处短路整个解析,合法会话被静默丢弃(浏览器其他 cookie 排前即触发) | 改逐段局部匹配;单测锁死任意位置可取到/同名前缀不误匹配 |
| H2 | bm-core/src/runtime/task_ops.rs handle_worker_call | 结果流水 `operation_id` 取 `op_capability.keys().last()`(HashMap 无序)= 完成判定证据链张冠李戴 | capability_call_inner 返回本次真实 op_id(tuple);流水直取 |
| H3 | bm-core/src/runtime/handle.rs 恢复路径 | 崩溃恢复无条件 `Resuming→Running`:用户显式取消后、回合边界落定前崩溃 → 重启复活接单+凭输入原文重驱烧模型;契约边 `Resuming→Stopped(turn_was_stopping)` 成为死代码 | 新增 op_cancel_marks 持久标记(schema v8→v9);handle_cancel 写标记(失败入拒写态);恢复端凭标记走 `Interrupted→Cancelled(user_ruling)`+`Resuming→Stopped`;agents_to_resume 补终态守卫;t27 回归 |
| H4 | bm-surface-http/src/about.rs apply_update | let-chain 条件求值内先发一次 `systemctl restart --no-block`,进块后又发一次=双重重启,第二发失败会错误跌落自拉起分支与 systemd 竞争 | 重构为单次触发 |
| H5 | bm-providers/src/mcp.rs StdioMcpTransport::request | `pending.insert` 后 `write_frame().await?` 失败即返回,pending/token_to_id 随失败累积泄漏(长跑守护进程内存无界上爬) | 写失败清账;成功路径幂等 remove 兜底 |
| H6 | webapp MusicPlayer.tsx | 工作区无音频时注入两条虚构曲目(伪造文件大小),点播 404 静默失败 | 删假数据,诚实空态 |
| H7 | webapp PluginsPage.tsx | 探活只回工具数量时伪造 `tool_1..tool_n` 假名清单 | 删伪造,复用「未探测到可用工具」诚实分支 |

## 3. 已核实并当轮修复(回归/行为)

| # | 位置 | 缺陷 | 修复 |
|---|---|---|---|
| R1 | HEAD 056b6ee 回归:task_ops.rs outcome 分类 | worker 审批豁免仍匹配旧 `Semantic(ApprovalRequired)`,结构化 `ApprovalNeeded` 被归为 "error"——等人的时间被 watchdog 计停滞 | 分类对齐 ApprovalNeeded |
| R2 | HEAD 056b6ee 回归:m5/m4/m7/m8/m9 测试 | t52/t53、m4×4、m5_coordinator×2、m9_memory×4、m9_review×2、m7_health×2、m7_mcp、m8×2 等仍断言旧错误形态(main 上实红) | 全部对齐结构化 ApprovalNeeded |

## 4. 已核实并当轮修复(裁决项落地)

| # | 位置 | 裁决 | 落地 |
|---|---|---|---|
| D1 | bm-core/src/team.rs authorization_subset | 「只减不增」按安全侧收紧 | `(child 无 resources, parent 具体)→拒绝`(原实现放行=子任务凭空谓词越出父授权谓词集);`child 具体/parent 全参` 与双方全参保持允许;新增拒绝向量测试。既有测试(双方全参)不受影响 |
| D2 | bm-core budget/runtime.rs fail_turn | 失败回合计入预算 | BudgetState::account_failed_turn(回合数+1,token 未知如实记 0);fail_turn 统一收口调用,回合配额用尽发 budget.exceeded。防劣质网关失败重试绕过 max_turns 烧钱 |
| D3 | bm-surface-http/src/portal.rs | 登录端安全加固(ADR-0009 承兑) | ①失败限速:同源 5 次失败锁 15 分钟(按对端 IP,取不到 connect-info 时退化为全局门);②密码哈希升级 PBKDF2-HMAC-SHA256(10 万次迭代,RFC 7914 向量测试),旧 SHA-256 条目登录成功透明升级;bootstrap/改密全走新格式 |

## 5. 已核实并当轮修复(口径统一/工程卫生)

- **持久化吞错收口**(全仓 `let _ = store.*` 清零):
  - 安全关键路径 → 拒写态:persist_task / persist_grant(Once 消费丢失=静默扩权)/ save_task_budget(计数回退=预算绕过)/ capabilities register/unregister binding;
  - 文档化 T6 宽松路径 → 补错误日志不再静默:persist_approval、outbox_upsert×3、启动期 binding、emit 兜底审计事件。
- **sha256→hex 收归**:bm-contract 新增 `hash` 模块(hex/sha256_hex+已知向量测试),替换 5 个 crate 13 处复制。
- **atomic_write 统一**:secret.rs 复用 bm_persist::util(两份实现逐字节等价,单点维护)。
- **依赖收归**:reqwest(aes-gcm/getrandom 一并)收进 `[workspace.dependencies]`,消除 4 crate feature 面漂移与 bm-surface-http 同文件双声明。
- **前端事件总线收编**:`lib/bus.ts` 类型化 BM_EVENTS 常量+emit/on,替换 5 文件 8 事件裸字符串 CustomEvent。
- **小修**:task.rs add_member 的 `updated_at` no-op 自赋值改真实墙钟;registry.rs async 注释对齐真实行为;task_ops「四门禁」注释对齐现实(并发门禁未实现);bm-judge 延迟门与 BOEN_TURN_TIMEOUT_SECS 同源(原固定 30s 误判合法慢回合)。

## 6. 需裁决项(逐项待拍板,尚未动)

1. **授权子集收紧的破坏面**:D1 已按安全侧收紧;若现网有子任务以空谓词创建成功过,升级后此类 spawn 将被拒——是否需要兼容期提示(待用户确认现网用法)。
2. **失败记账口径**:D2 token 侧失败记 0;若网关对失败调用回执部分 usage,是否接入按实际计(需 provider 侧解析失败响应 usage,合同字段或需 Minor)。
3. **门户无 logout 端点**(前端无退出按钮)+ 会话 Cookie 无 `Secure` 标志(TLS 部署前置):建议随下次发版补齐,涉及前端+portal 两面。

## 7. 遗留登记 BACKLOG(本轮不动,来源=本文件)

- 持久读错误统一 `unwrap_or_default()` 折叠为空(handle.rs 启动恢复 8 处、events_for_session 等):故障被消音成假数据,与「宁可拒开」相悖;损坏 grant 行被跳过会导致 bootstrap 协调权重签发(高候选,需故障注入端到端验证)。
- emit 形状校验失败路径制造事件日志 seq 空洞(runtime.rs:363-397),违反 INV-3(Judge contiguous 可检出但生产无保护)。
- MCP:reload 不强杀旧子进程(僵尸窗口)、respawn 无去抖/无上限、`restart_limit` 配置解析后无消费方(死配置)、HttpMcpTransport 裸 send 无超时。
- fs.write/edit:非原子写(同仓 atomic_write 标准未应用)+ 无大小上限(write/edit 1MB 限制只拦 search)。
- system.exec `cwd` 参数不经 fs_tools 沙箱白名单(schema additionalProperties 默认放行)。
- FileSecretStore 主密钥 `&material[..32]` 截断非 KDF;get/put/delete 每次全量解密重加密 O(n)。
- 前端:w1/context.tsx 手维护 evMap 与 context-log `kind` 字符串无后端契约锚定;`McpCandidatesResult` 双类型声明已漂移(PluginsPage 本地 vs api.ts);webapp 主 App 不查 portal/state,401 后无正向引导;PluginsPage 仍用原生 confirm/alert(与 PLAYBOOK §3 相违);runtime.tsx `document.title` 调试残留。
- openai_compat.rs:非流式分支恒回 default_model 名(按条路由时回包 model 撒谎);glm_http 错误分类一刀切 Unavailable、无单测(feature 门控)。
- 测试:m7_health 两处裸 sleep(200ms/1000ms)依赖调度时序,慢机器易撕破;bm-cli 零单测(wire 错误码映射无回归)。
- 插件:web-multisearch `usage.rs` 手写格里高利历推月(跨月边界±1 天乱)、aggregate 超时无优雅取消;context-inspector 全量读 context-log 进内存(大目录 OOM 面)、stdio 主循环同步阻塞;两插件与主仓 stdio 框架代码三份重抄(每插件独立 exe 原则的既知代价,未来抽共享 crate 需评估)。

## 8. 验证与提交

- 批次一 commit:`c9302c9`(H1..H7 + R1/R2);
- 批次二/三:见当日后续 commit(D1..D3 + §5 全部);
- 回归:cargo fmt/clippy 零警告、非 bin 目标全测试绿(validate.py 全绿、前端 build 绿);bm-runtime e2e 因本机在役 server 锁定 exe 以独立 target 目录另行验证。
- 三个裁决项(§6)等待用户逐项回复后动工。
