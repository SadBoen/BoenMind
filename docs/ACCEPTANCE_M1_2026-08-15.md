# M1 验收报告（2026-08-15）

> 编程应用 M1 = 现有能力编排一次真实编程任务验收：**读 → 改 → 测 → 提交**。
> 结论先行：**M1 通过**——BoenMind 用自身运行时（bm-loop + MiniMax-M3 + 内置工具）
> 在自身仓库上独立完成了一个真实 bug 的定位、修复、回归测试与提交；全程事件日志可审计。
> 验收过程中暴露 6 个真实问题（见 §四），全部有修复建议，不推翻结论。

## 一、验收设定

| 项 | 值 |
|---|---|
| 任务 | 修复代码回看报告遗留项"prompt_hash 不覆盖注入面"（原定 M1 前处理）：on_request 改写 payload（记忆插件注入 facts）后审计锚点 hash 失真 |
| 目标文件 | backend/crates/bm-loop/src/engine.rs + tests/engine_tests.rs |
| 运行环境 | 真实用户环境 ~/.boenmind（boenmind.db/插件/记忆全用真实数据）；working_dir 临时指向 D:\96_CoderWorld\BoenMind，验收后已恢复 |
| 引擎/模型 | bm-loop 默认引擎 + MiniMax-M3（用户配置默认；deepseek 备用 key 是占位符 sk-test-invalid，不可用） |
| 驱动方式 | 纯 API 驱动（POST /api/sessions + /api/chat SSE），无前端；操作者只发任务描述，不代写代码 |
| 任务描述 | 英文纯 ASCII 2350 字符（规避 Windows curl 中文 JSON 坑）；第二回合续接指令 1286 字符 |

## 二、过程实录

**回合 1**（10 分钟，失败收场——非任务失败，是步数预算耗尽）：

- 模型自行探索：ls/cat Cargo.toml → read 21 次 → grep/bash 定位 EpochHeader/prompt_hash 全库引用
- 7 次 edit 完成核心修复：RequestHeader 从"回合开头预计算"迁移到"首步 on_request 改写后落盘"，hash 覆盖最终模型可见输入；新增 InjectingHooks 测试钩子 + 回归测试
- 64 步预算耗尽（`步数超上限（64）`）时：修复与测试已写完，但**测试未跑、未提交**
- 过程中因"read 工具无行区间参数"反复用 powershell/findstr 技巧看行号区间，燃烧大量步数

**回合 2**（3 分钟，done 干净收尾）：操作者发续接指令（"检查 diff → 跑测试 → 修错 → 提交"，并提示路径/超时要点）→ 模型审查 diff → cargo test 迭代修复编译告警 → git add + commit 成功 → git log 自验。

**统计**：回合 1 工具调用 75 次（bash 33 / read 21 / grep 11 / edit 7 / ctx_execute 2 / ctx_search 1）；
回合 2 工具调用 48 次（bash 36 / read 4 / edit 5 / grep 2 / write 1）；合计模型步 111 步、
输入 3,584,456 tokens、输出 32,418 tokens。

## 三、结果

- **提交 8254bd7**（作者 sadinsun，运行时自身提交）：engine.rs +79/-34、engine_tests.rs +148/-1，恰好两个目标文件，无夹带
- **修复正确性**（§五独立复核确认）：on_request ctx 的 prompt_hash 传 None（改写前未定型，注释如实说明）→ 改写后按 system+tools+payload 重算 → RequestHeader 首步落盘该 hash → on_request_error 携带逐步重算后的 hash
- **回归测试**：InjectingHooks 模拟 bm-memory 注入形态；断言"落盘 hash = 覆盖注入后的 payload 的 sha256"且"与注入前 hash 必不同"（双向防漂移）；既有事件链测试同步更新 header 位置
- **独立复核**：cargo test -p bm-loop 33 测试全绿（exit 0 独立捕获，非管道退出码）；clippy 零告警

## 四、验收发现（6 个真实问题，按影响排序）

| # | 问题 | 现象 | 建议 |
|---|---|---|---|
| 1 | **单回合 64 步预算不足以完成真实任务** | 回合 1 在"修完待测"处被砍断，需人工续接才收尾 | max_steps 提到 128+，或"回合接近上限时引擎自动提示压缩/续接"；M2 面板顺带做 |
| 2 | **grep/find 不尊重 .gitignore 且无超时** | 全库 grep path="." 遍历 backend/target 数万文件，单次调用卡 ~4 分钟（walk_files 纯 std 递归，无 ignore 也无 timeout 参数） | walk_files 接入 ignore crate（仓库内 ripgrep 库族偏好已有先例）+ grep/find 加 timeout 参数 |
| 3 | **read 工具无行区间参数** | 模型为看 510-525 行反复折腾 powershell/findstr 技巧（10+ 次调用），是步数燃烧主因 | read 加 offset/limit（协议已有类似字段先例）；模型系统提示里补一句"read 支持 offset/limit" |
| 4 | **bash 经 cmd /C 与 Git Bash 习惯冲突** | `/d/...` 路径、`cd "x" && pwd` 在 cmd 下无效，模型为 git status 浪费 ~8 次调用；commit 需 -F 文件技巧才过引号关 | 引擎侧补"Windows 路径规范"系统提示段；长提交信息走 -F 已是可行 workaround |
| 5 | **ctx-compactor 索引写入服务进程 cwd** | 验收后仓库根出现 .boenmind/ctx-index/（未跟踪垃圾） | 插件数据一律落 BOENMIND_HOME（config.working_dir 之外）；已删残留 |
| 6 | **模型路径幻觉** | 回合 1 尾部模型一度以为仓库在 C:\Users\Boen\backend（与记忆注入/长上下文漂移有关） | 任务级系统提示锚定工作目录；压缩水线已在收敛此现象 |

## 五、独立复核（操作者视角）

1. `git show 8254bd7` 逐行审读 engine.rs 与测试 diff：语义正确、注释与代码库风格一致、无夹带改动
2. `cargo test -p bm-loop` 独立重跑：12+2+19 共 33 测试全绿，exit code 直接捕获（不经管道）
3. `cargo clippy -p bm-loop`：零告警
4. 全工作区测试（bm-server/bm-kernel/bm-core/bm-compat）：全绿（见 pre-push 门禁日志）
5. 事件日志审计面：/api/sessions/{id}/messages 全链路工具调用/结果可回溯（验收会话保留在真实 DB）

## 六、结论

**M1 达成**：运行时自主完成"读→改→测→提交"全链路，产物质量经独立复核合格。开销真相：
一次真实小 bug 修复 ≈ 两回合 111 模型步 / 358 万输入 tokens——**单回合预算、工具无超时、
行区间读取**是三大效率闸门，修掉前两者后真实任务可望单回合收尾。

下一步按拍板推进：**M2 = 独立壳应用起步**（文件树/编辑器/分支图 + 活任务清单 todo 事件投影）；
§四问题 1-3 随 M2 修复（工具面是壳的基座，先于壳交付）。
