# 过夜作战台账 2026-09-06(工具协议修复批)

- **状态:进行中** ← 自动化守卫读这里;"已完成"后 5 分钟循环自动化自动变为空转
- **单写者锁**:本台账 mtime + `git log -1 --format=%ct` 即活跃度信号;15 分钟内无活动才算主会话已死
- **守卫自动化 ID**:`automation-940c6a16-1239-48f3-b665-d12ec67cd38a`(每5分钟;完成后置"已完成"即自动空转;用户醒来后可在自动化面板删除)
- 任务来源:docs/agent-tools-payload-comparison-report.md §9 改进方案
- 铁纪律:AGENTS.md 全部硬纪律 + 单写者纪律(同一时刻只许一个代理写代码)

## 任务清单(按序执行)

- [ ] T1 合同 Minor:bm-contract Message 增可选 tool_call_id / tool_calls(只增不破)+ validate.py 全绿
- [ ] T2 openai_http.rs:WireMessage 增字段;Role::Tool → 原生 role:"tool";assistant 消息透传 tool_calls
- [ ] T3 turn.rs:回喂 assistant 消息携带 tool_calls;工具结果带 tool_call_id;删成功路径"不要再次调用"禁令(审批拒绝定向引导保留;防死循环交给 MAX_TOOL_ROUNDS)
- [ ] T4 描述治理:chat_tools 带出 manifest description;MCP 工具用自描述;审批 UI 措辞移出工具描述
- [ ] T5 fs_edit 升级 edits 数组批量替换(向后兼容单处 old_string/new_string)
- [ ] T6 全量回归:fmt + clippy --all-targets + nextest/cargo test 全绿
- [ ] T7 ADR-0022 + HISTORY/BACKLOG 登记
- [ ] T8 浏览器仿真 E2E 验收:真模型对话 + 工具链式调用 + 审批流 + 截图留档
- [ ] T9 收宫:ledger 置"已完成"、最终提交、push origin/main

## 下一步(主会话已死时,自动化从这里接手)

从第一个未勾选项继续。动手前:读 AGENTS.md → 读本台账 → 查 git log 确认已完成项 → 单写者检查。

## 决策记录(过程中补充)

- (空)

## 环境备忘

- 启动三件套:run_in_background + CWD 仓库根 + env 内联;BOEN_MODEL_STREAM=1 必带;重启前 taskkill 旧进程
- server.exe 在跑时测试用 CARGO_TARGET_DIR=target-regress 绕开 target 锁
- 提交前必跑 fmt;clippy 口径 --all-targets;合同变更必跑 validate.py
