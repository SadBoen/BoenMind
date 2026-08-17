# BoenMind 权限模型与工具面核查（2026-08-17）

> 起因：用户问"为什么 BoenMind 没有执行 shell/卸载软件等能力，而 ZCode agent 能？"
> 结论先行：BoenMind 已有完整的权限门（四档 + 询问链 + 决策记忆 + 内置工具门），
> 但**全栈零提权**（无 UAC/无 sudo/无管理员令牌），且内置工具面其实有 bash——
> 被「专家工具子集」挡在了 coding 场景外面。参考业界（Claude Code/Codex/DSH/pi/
> hermes）后确认：**主流编程 agent 都不做内置提权**，系统级操作走"bash 工具 +
> 命令审批 + 用户手动提权"兜底。下文是逐项核查 + 参考对比 + 对齐建议。

---

## 一、BoenMind 权限门全链路（现状核查，2026-08-17）

### 1.1 档位（extension_policy，四处可见：聊天输入框右下角 / 设置-插件页 / config.toml）

| 档位 | 后端映射 | 行为 |
|---|---|---|
| default | 不设置 | 走上游默认（Prompt 询问） |
| safe | Prompt 模式 | 关键能力询问 |
| balanced | Prompt 模式 | 同 safe（当前未细分） |
| yolo | permissive + `extension_allow_dangerous=true` | 全自动放行 + 放开 `exec`/`env` 危险能力 |

代码：`compat_engine.rs::extension_policy_from_config`（1081-1100 行）。

### 1.2 询问链（ask_capability，插件引擎 + 内置工具门共用单一事实源）

```
决策记忆(extension-permissions.json) 命中 → 直返 allow/deny
        │ 未命中
        ▼
SSE 推 PermissionRequest → 前端 PermissionDialog（允许一次/总是允许/拒绝）
        │ 60s 超时 → fail-closed 拒绝
        ▼
"总是允许" → 回写 extension-permissions.json（跨会话生效）
```

- 决策记忆键：`extension_id × capability`（如 web-search × http / builtin × bash）
- 记忆文件：`~/.boenmind/extension-permissions.json`（app_dir 下）
- 代码：`compat_engine.rs::ask_capability`（88-149 行）、`permission_store.rs`

### 1.3 内置工具权限门（BuiltinGate，审查 P0-2 引入）

- 高权限工具：`bash`（任意命令）、`subagent`（派生子代理）→ 过门询问
- 低权限工具：`ls/find/grep/read/write/edit`（工作区 safe_join 圈禁内）→ 不打扰
- 档位为 permissive/yolo 时直放（与插件引擎同一档位来源）
- 代码：`builtin_gate.rs`

### 1.4 工具面（模型可见）

**内置**（`builtin_tools.rs`，7 个）：read / write / edit / grep / find / ls / bash
**全局**：todo（活任务清单）、subagent（子代理）
**场景**：wiki → wiki_query / wiki_ingest / wiki_add_relation；chat/coding 无场景工具
**专家过滤**：编码场景绑定 expert（如 coding-architect）→ 工具子集过滤（见 1.5）
**插件**：作用域过滤（plugin_scopes）+ 已启用过滤（10 个工具，5 插件）
**MCP**：`mcp__<server>__<tool>`，per-server 作用域 + McpGate 权限门
**Steward**：set_wake 仅管家会话

### 1.5 关键发现：bash 其实存在，被专家子集挡了

- coding 场景默认绑 `coding-architect` 专家，其 `tools: read,grep,find,ls,write`（无 bash/edit）
- `coding-coder` 专家才含 `bash,edit`
- 所以用户问"执行 shell ❌"：一是专家工具子集没放行 bash，二是即便放行，普通权限下很多系统命令也跑不了

### 1.6 提权现状：全栈零提权

| 形态 | 运行身份 | 提权 |
|---|---|---|
| Windows 桌面壳（Tauri） | 当前用户，无 UAC 提升 | ❌ |
| Windows 便携版 | 同上 | ❌ |
| Linux 服务器版（systemd） | 专用低权限用户 `boenmind`（/usr/sbin/nologin） | ❌ |
| Linux 便携/桌面 | 无官方形态 | — |

结论：BoenMind agent 的能力边界 = 当前用户令牌能做什么 × 工具面注册了什么 × 权限门放行什么。三者都满足才可执行。

---

## 二、参考 Agent 对比（2026-08-17 调研）

### 2.1 工具清单

**编程型 agent 的核心工具交集**（Claude Code / Codex / DSH / pi / hermes / ZCode 社区版都有的）：

| 类别 | 参考系统标配 | BoenMind 现状 |
|---|---|---|
| 文件读写 | read / write / edit（string-replace） | ✅ 全有 |
| 搜索 | glob / grep / find / ls | ✅ 全有（注意：无 glob，find 是子串匹配非 glob） |
| 命令执行 | **bash / PowerShell / terminal(PTY) / exec** | ⚠️ 有 bash 内置但被专家子集挡；无 terminal(PTY)；ctx_execute 是 QuickJS 沙箱非 shell |
| 任务 | todo / task（后台任务） | ✅ todo；❌ 无后台任务 job 工具 |
| 子代理 | subagent / task / delegate | ✅ subagent |
| Web | web_search / web_fetch | ✅（插件） |
| MCP | mcp_* | ✅ |
| Skill | skill | ✅ |
| 提问 | ask_user_question / AskUserQuestion / clarify | ❌ 无 |
| 计划 | plan / EnterPlanMode | ❌ 无 |
| LSP | lsp | ❌ 无 |
| 会话检索 | session_search / session_trace | ❌ 无（有 /api/sessions/{id}/events 可查，未做工具） |
| 工具自省 | tool_search / tool_describe / cordis_inspect | ❌ 无 |
| 定时 | cron / schedule | ⚠️ 有 set_wake（仅管家会话） |
| 系统管理（服务/卸载/注册表） | **参考系统都没有专门工具，全靠 bash 表达** | ✅ 与业界一致（没专门工具是常态） |

### 2.2 权限模型（三家代表性）

| 系统 | 档位 | 结构 | 命令安全机制 |
|---|---|---|---|
| Claude Code | default / acceptEdits / plan / bypassPermissions（+auto 分类器） | 单旋钮四档 + per-tool 规则 `Bash(npm run *)` | 独立进程、2min/10min 超时、5GB 输出杀进程、cgroup 内存上限、deny 规则覆盖文件命令 |
| Codex CLI | read-only / auto / full-auto（+granular 细分） | **双旋钮**：SandboxMode × ApprovalPolicy；**单命令提权** `additional_permissions` | **参数级安全白名单** `is_safe_command.rs`（cat/ls/git/find/rg/sed 按参数放行）、复合命令分段评估、危险 wrapper 前缀拦截 |
| DSH | read-only / workspace-write / danger-full-access × ask/never | **双旋钮** + 预设；一次性提权 `sandbox_permissions + justification` | 平台沙箱（bwrap/seatbelt/ACL）+ 审批，无黑名单 |
| pi（上游） | Strict / Prompt / Permissive | 能力粒度（read/write/exec/http/env）+ per-extension 覆盖 + 配额 | **危险命令分类 10 类 × 2 tier + 反混淆归一化**（剥引号/`${ifs}`/转义） |
| hermes | 逐条确认 / headless 自动 / gateway | 会话级审批 + 永久 allowlist + smart approval(LLM) | 危险命令模式正则 + sudo stdin 守卫 + 可选 tirith 预执行扫描 |
| ZCode 官方 CLI | plan / build / edit / yolo + `--disallowed-tools` | 四档 + 工具集级物理移除（先于权限层，yolo 绕不过） | — |

### 2.3 提权：业界共识 = 不做内置提权

| 系统 | 提权 |
|---|---|
| Claude Code | **无**；反向禁止 root/sudo 下启动 bypassPermissions |
| Codex | **无**；单命令权限放宽（网络/读写）只加本次命令所需，不整条逃出沙箱 |
| DSH | **无**；提权=放宽沙箱档位，需用户审批 |
| pi | **无** |
| hermes | **唯一有 sudo**：明文密码或交互提示（45s 超时、session 缓存），文档标注 SECURITY WARNING |
| ZCode 官方 | 无（--disallowed-tools 物理移除工具） |

**结论**：系统级操作（卸载/服务/注册表）在业界不是"agent 内置提权"，而是 **bash/PowerShell 工具 + 命令审批 + 用户手动以管理员身份执行**。BoenMind 现在的能力（bash 过 BuiltinGate 询问）方向正确，缺的是 bash 真正进工具面 + 命令级安全白名单。

---

## 三、对齐建议（工具清单 + 权限模型）

### 3.1 工具面补齐（按优先级）

1. **bash 进 coding 工具面**（最低成本，已内置）：coding-architect 专家工具子集加 `bash,edit`；
   或改默认专家为 coding-coder。同时补 `glob`（参考系统标配，BoenMind find 是子串匹配）
2. **命令级安全白名单**（抄 Codex `is_safe_command.rs`）：cat/ls/git/find/rg/sed 按参数自动放行、
   危险命令拦截、`rm -rf /`/`rm -rf ~` 保底拦截（Claude Code 同款）
3. **PTY 终端工具**（vendor 台账 T1/T2 已选 xterm + portable-pty）：terminal_open/send/read/signal，
   对齐 DSH/Claude Code
4. **后台任务工具**（job_output/job_list/job_kill，对齐 DSH/Claude Code Task 族）
5. **ask_user / 计划模式**（plan）：对齐 Claude Code/Codex
6. **会话检索 / 工具自省**（可选）：session_search、tool_search

### 3.2 权限模型加固

- **保留现四档**（default/safe/balanced/yolo）作为 UI 预设，内部映射到"双旋钮"语义
  （SandboxMode × ApprovalPolicy）——DSH 模型，语义最清晰
- **补"单命令提权"**：PermissionBridge 已存在（bm-compat B5），按 Codex
  `additional_permissions` 实现"按次加权限，不出沙箱"，比整档切换安全
- **补 pi 危险命令分类**：10 类（RecursiveDelete/ForkBomb/PermissionEscalation…）× 2 tier +
  反混淆归一化，作为命令层防火墙（可落地，独立于沙箱）
- **每工具规则**（可选）：`bash(npm run *)` 模式规则，对齐 Claude Code 体验

### 3.3 提权方案（Linux / Win 便携版）

**方向修正**：业界共识是 agent 不做内置提权。BoenMind 提权需求的正解 = **系统工具走 bash +
命令审批 + 用户手动提权**。具体：

**Windows**
- 短期（推荐）：bash 工具进工具面 + 危险命令白名单 + 询问链。系统管理命令（卸载/服务）
  由用户在**终端面板手动以管理员运行**（BoenMind 已有 /api/terminal）
- 中期（可选）：提权 helper 进程（`ShellExecuteW(runas)` 拉起，只执行白名单命令），
  主进程保持普通令牌；helper 每命令经询问链。**不做** manifest 全提权（攻击面最大）

**Linux**
- 桌面/便携：bash 工具经 `pkexec`（polkit 授权对话框，复用系统身份验证）执行特权命令，
  无需 BoenMind 管理密码——与 Windows UAC 对称
- 服务器（systemd）：保持 `User=boenmind` 低权限；如需系统管理，单独部署
  `boenmind-admin` 服务（sudoers 白名单只允许特定命令），属高级部署选项，默认不开

**配置入口（config.toml，后续加）**
```toml
[agent_identity]
mode = "none"        # none=当前用户（默认）；"elevated"=系统工具可用（UAC/polkit 提升）
elevation = "on_demand"  # on_demand=首次调用触发提升（推荐）；auto=启动即提升（不推荐）
```

---

## 四、拍板点

- [ ] P1: bash 是否进 coding 工具面（改 coding-architect 子集 or 换默认专家）——**最低成本、立即可做**
- [ ] P2: 命令级安全白名单（Codex is_safe_command 移植）是否本轮做
- [ ] P3: agent_identity 段（提权配置）现在设计还是等编程应用 M2 一起
- [ ] P4: PTY 终端工具 / 后台任务工具是否纳入下轮（对齐参考系统工具面）
- [ ] P5: yolo 档是否允许跳过系统工具的询问（安全与效率取舍）
- [ ] P6: 服务器版是否做 boenmind-admin 白名单服务

---

*报告日期：2026-08-17。核查代码 commit：e9c6817。参考调研：Claude Code docs / Codex 源码 / DSH 源码 / pi 上游 / hermes 源码 / ZCode 社区（zmccyy/ZCode--CLI--agent、tizerluo/zcode-open-bridge）。*
