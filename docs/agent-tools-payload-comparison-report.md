# 四大 Agent → LLM 底层报文对比调研报告(v2 校对版)

- 调研日期:2026-09-06(初版 v1 由前一轮对话产出;本版为逐条校对、补充实证、优化后的 v2)
- 调研动机:用户实测 BoenMind Agent"工具使用非常别扭"。现代 LLM 在训练期已对 Agent 工具形态做了专项优化——谁的工具指令最贴合模型本能、谁的描述最清晰、谁最省 token,谁的调用就最流畅。
- 调研对象:
  1. **BoenMind**(自家,v0.0.8 后主干)
  2. **DSH**(DeepSeek 官方 Harness,本机 `127.0.0.1:8080` 在役)
  3. **Pi Agent**(Mario Zechner 极简核心,`badlogic/pi-mono`)
  4. **Hermes Agent**(Nous Research,VPS 实机在役 v0.20.6)
- 本文自包含,可直接交给任何 AI 做交叉审核,不依赖原对话上下文。

## 0. 证据分级与 v2 校对声明

证据标签:
- ✅【源码核实】= 逐字读到源码/配置原文并引用
- 🔎【实机核实】= 在运行实例上只读检查所得
- ⚠【未核实】= 有线索但未复核,如实标注

v2 相对 v1 的主要修正(防错误信息扩散,后续 AI 审稿请以本节为准):
1. v1 称 DSH 出厂 5~6 个工具 → 实测标准 code preset 为 **7 个基础工具**(read/write/edit/read_image + glob/grep + pwsh/bash),另按 preset 挂载 jobs/web/skill/todo/ask_user/subagent 等;
2. v1 称 Pi 仅 4 个工具 → 源码 tools 目录实为 **8 件**(bash/powershell 平台变体 + read/write/edit + grep/find/ls 只读三件套);v1 的"edits 数组"核心结论核实无误;
3. v1 暗示"中文描述 = token 冗长" → **不成立**。中文按 token 计通常更省;真正的问题是描述内容混入了 UI/审批流程噪音,不是语言本身;
4. v2 新增两项 v1 未发现的结构性问题:①模型看不到自己发起的 tool_call(回喂时 assistant 消息丢失 tool_calls 数组);②全部 MCP 外挂工具的描述被内核丢弃,模型只看到"只读直通工具"一句套话;
5. v1 的"DSH edit 失败返回就近上下文辅助纠错"⚠未复核;Hermes"9 种模糊匹配回退"⚠未复核(源码仅确认 fuzzy matching 存在)。

## 1. 一页结论(大白话)

**排名(工具调用流畅度视角):Pi ≈ DSH > Hermes > BoenMind。**

各家一句话:
- **Pi Agent** = 极简派天花板。系统提示只有"工具单行清单 + 几条守则"两小节,工具描述每条一句话,编辑工具一次调用可打多个补丁(edits 数组)。它赌的是"模型本来就聪明,少废话"。
- **DSH** = 工程派天花板。平台感知(Windows 挂 pwsh、Linux 挂 bash),工具结果自动截断保护,还有更狠的 Code Mode:把 5 次工具往返合并成 1 次跑 TypeScript 脚本。
- **Hermes Agent** = 兼容派天花板。核心六件套(read_file/write_file/patch/search_files/terminal/process),描述全在教模型"别用 cat/sed/awk,用我";还带按模型裁剪 schema(源码注释:仅 patch 一项条件裁剪就为非 OpenAI 模型每次调用省约 148 token)和海量工具场景的 tool_search 渐进式发现。
- **BoenMind** = 设计思想不差(审批流、能力合同都扎实),但底层协议四处漏气,模型被折腾得晕头转向。

**BoenMind"别扭"的五大根因(全部✅源码核实,证据见 §4):**
1. 工具结果被伪装成用户发言(`Role::Tool => "user"`,openai_http.rs 两处);
2. 模型看不到自己刚才发起了什么调用(回喂时 assistant 消息丢失 tool_calls 数组);
3. 成功结果后面强贴"不要再次调用该工具"禁令,把"搜→读→改→测"的链式调用一刀斩断;
4. 全部 MCP 外挂工具(联网搜索、上下文透视等)的描述被内核丢弃,模型只看到"只读直通工具";
5. 审批这种前端 UI 行为被写进工具描述("调用后会弹出审批卡片"),角色错位。

## 2. 数据来源与核实方式

| 对象 | 核实方式 | 位置 |
|---|---|---|
| BoenMind | 直接读本仓源码 + context-log.jsonl 真实快照辅助 | `runtime/crates/bm-providers/src/openai_http.rs`、`runtime/crates/bm-core/src/runtime/turn.rs`、`runtime/crates/bm-core/src/registry.rs` |
| DSH | 直接读本机 npm 安装包源码(defineTool 定义逐字提取) | `C:\Users\Boen\AppData\Roaming\fnm\aliases\default\node_modules\@deepseek-ai\dsh\`(内嵌 @deepseek-ai/dsh-tool-* 包);会话原文 `C:\Users\Boen\.dsh\sessions\**\session.jsonl.zstd` ⚠未解包(本机无 zstd 解码器) |
| Pi Agent | GitHub 源码逐文件核实 | github.com/badlogic/pi-mono → packages/coding-agent/src/core/tools/*、system-prompt.ts |
| Hermes Agent | VPS SSH 只读检查实机安装源码(v0.20.6) | /usr/local/lib/hermes-agent/(tools/file_tools.py、tools/terminal_tool.py、tools/tool_search.py、toolsets.py、model_tools.py) |

方法说明:不看宣传文档,只看"真正发给 LLM 的东西"——系统提示组装逻辑、工具 JSON Schema 原文、工具结果回传的消息角色与字段。

## 3. 横评矩阵

| 维度 | Pi Agent | DSH | Hermes Agent | BoenMind |
|---|---|---|---|---|
| 基础工具集 | bash/powershell(平台二选一)+ read/write/edit + grep/find/ls(8 件)✅ | read/write/edit/read_image + glob/grep + pwsh/bash(7 件)✅ | read_file/write_file/patch/search_files/terminal/process(核心 6 件,另有 80+ 扩展按需挂载)✅ | fs_read/fs_write/fs_edit/fs_search + system_exec(5 件)+ MCP 外挂 |
| 工具描述风格 | 每条一句话,正向指示("Use read instead of cat")✅ | 一句话 + 行为契约(超限落盘、mtime 排序、正向导流)✅ | 描述直接教替代关系("Use this instead of sed/awk")+ 输出格式 + 预算说明 ✅ | fs 四件中文描述尚详尽;system_exec 与全部 MCP 工具是"审批卡片/只读直通"套话 ✅ |
| 工具结果回传 | role:tool + tool_call_id(标准原生)✅ | role:tool + tool_call_id(标准原生)✅ | OpenAI 风格 function calling,tool_call_id 全链路 ✅(XML `<tool_call>` 是 Hermes 微调模型的训练格式,与运行时是两回事) | role:"user" 伪装 + 无 tool_call_id + assistant 丢 tool_calls ✅ |
| 回传附加语 | 零附加,纯原样 ✅ | 零附加;超长结果前 4096 + 后 1024 优雅截断(pruner)✅ | 正向确认("verified:true,内容哈希已确认")✅ | 每次成功后强贴"(该调用已完成,请直接基于此结果回答用户,不要再次调用该工具。)"✅ |
| 编辑契约 | edit = **edits 数组**,一次多处,基于原文件快照匹配 ✅ | edit = 单处 old_string/new_string + replace_all(与 BoenMind 同形)✅ | patch = 单处替换 + fuzzy matching(策略数⚠未核)+ 按模型条件裁剪 schema(省 148 tok/次)✅ | fs_edit = 单处替换 ✅ |
| 终端与搜索关系 | 终端是万能钥匙,guidelines 明示文件操作用 bash;无终端环境才提供 grep/find ✅ | 平台感知双终端 + glob/grep 专职工具并存,描述互不踩脚 ✅ | terminal + search_files 并存,描述互相导流 ✅ | fs_search(6 参数)与 system_exec 并存,无导流无分工说明 ✅ |
| 输出防爆 | truncate.ts + output-accumulator ✅ | pruner 阈值 8192/4096/1024 ✅ | read 100K 字符预算 + next_offset 续读 | system_exec 输出截断 16K;fs_read 有 offset/limit ✅ |
| 系统提示规模 | 工具单行清单 + 6~8 条守则,极小 ✅ | persona 一句话 + 按模式注入段落(plan mode/compaction 等)✅ | 模块化(记忆/技能/工具集分层注入)✅ | 角色基底 + 附加技能 + 工作目录注入,紧凑 ✅ |
| 特色 | edits 数组原子多补丁;prompt caching 友好 | Code Mode:5 次往返合并 1 次 TypeScript 脚本;模型自写 UI 摘要(description 参数) | tool_search 渐进披露;模型感知 schema 裁剪 | 审批流(effect=external-side-effect → needs_approval)安全模型扎实 |

## 4. BoenMind 详档(问题方,证据最全)

### 4.1 实际发给 LLM 的请求结构 ✅

组装链路:`bm-core/src/roles.rs::compose_role_prompt`(系统提示 = 角色基底 + 附加技能段 + [工作目录] 注入)→ `bm-core/src/runtime/turn.rs::execute_turn`(消息历史 + 本轮输入 + 工具清单)→ `bm-providers/src/openai_http.rs`(序列化发往 /chat/completions)。

工具清单来源:`bm-core/src/registry.rs::chat_tools()` 只返回 `(能力名, 参数schema, 是否需审批)` 三元组——**能力 manifest 里的详细描述在这一步就被丢掉了,turn 组装时拿不到**。

wire 层消息结构(`openai_http.rs`,WireMessage 结构体只有 role + content 两个字段):

```rust
role: match m.role {
    Role::System => "system",
    Role::User => "user",
    Role::Assistant => "assistant",
    Role::Tool => "user",   // ← openai_http.rs:265(invoke)与 :400(invoke_stream)两处
},
content: &m.content,
```

WireMessage 没有 `tool_call_id` 字段,也没有 assistant 侧的 `tool_calls` 数组字段。

### 4.2 工具循环的真实时序 ✅(turn.rs)

模型发起调用后,turn 循环(上限 `MAX_TOOL_ROUNDS = 5`,turn.rs:201)做三件事:

1. 把模型刚才的回复作为 assistant 消息回喂——**但只回喂 content 文本,tool_calls 数组被丢弃**(turn.rs:374-377);
2. 执行工具,把结果作为 Role::Tool 消息追加(序列化时变成 "user");
3. 成功结果统一强贴防复读后缀(turn.rs:586):

```rust
format!("{tool_result}\n(该调用已完成,请直接基于此结果回答用户,不要再次调用该工具。)")
```

审批路径的定向文案(turn.rs:530 成功 / :536 拒绝):
- 成功:`"用户已批准,工具执行成功。返回结果: {payload}。该调用已完成,请直接基于此结果回答用户,不要再次调用该工具。"`
- 拒绝:`"用户拒绝了能力 {capability} 的本次审批请求,工具未执行。请直接向用户说明情况,不要再次调用该工具。"`

所以模型在工具循环第二轮看到的**有效报文**是:

```text
[system]    系统提示
[user]      用户问题
[assistant] (只有它当时说的话;它发起的 tool_call 结构不见了)
[user]      工具结果……(末尾贴着"不要再次调用")
```

### 4.3 模型实际看到的工具描述 ✅(turn.rs:237-249)

fs 四件套有硬编码中文描述(尚详尽):
- **fs_search**:两段式中文说明(mode=content/files 双模式 + path_pattern),约 240 字符;
- **fs_read**:说明 1-based 行号 / offset / limit / total_lines;
- **fs_write**:"向工作区中的文件写入完整内容(需要用户审批)…";
- **fs_edit**:"…old_string 必须是在文件中唯一匹配的原文(含准确缩进)…建议修改前先用 fs.read 确认最新原文"。

其余所有工具一律套模板:
- 需审批工具:`"{cap} — 需要用户审批的业务工具(调用后会弹出审批卡片)"` → **system_exec 就是这条**;
- 非审批工具(全部 MCP!):`"{cap} — 只读直通工具"` → `mcp_web_multisearch_search`(联网搜索)、`mcp_context_inspector_*`(上下文透视)在模型眼里只有这一句,功能全靠工具名猜。

### 4.4 五大根因的机理分析

1. **role 伪装(openai_http.rs:265/:400)**:现代模型(DeepSeek-V3、GPT-4o、Claude 系)对 `role:"tool"` + `tool_call_id` 有专项训练的注意力模式;结果被伪装成 user 发言后,模型把它当成"用户突然插话发来一段终端输出",因果链错位。代码动机是兼容不支持 tool 角色的第三方网关,但代价是牺牲了所有现代模型的调用质量。
2. **tool_calls 丢失(turn.rs:374-377 + WireMessage 无字段)**:第二轮里模型看不到"这条工具结果对应我刚才发起的哪个调用",甚至不知道它是一条工具结果。这比 role 伪装更隐蔽,是 v2 新发现。
3. **成功即贴禁令(turn.rs:586)**:复杂任务必然链式调用(搜→读→改→测);第一步刚完成就被告知"不要再次调用",模型在"继续干"与"服从禁令"之间打架,表现为半途而废、答非所问。注意:拒绝路径那句"请直接向用户说明情况"是合理的定向引导;错在成功路径一刀切。
4. **MCP 描述全丢(registry.rs chat_tools 三元组 + turn.rs 套话)**:随包 MCP 工具(联网搜索/上下文透视)对模型不可描述,模型要么不敢用、要么乱用——这是"工具用得别扭"最直接的解释之一。
5. **审批 UI 入描述(turn.rs:249)**:"会弹出审批卡片"是前端表现层信息,写进工具描述后模型会在回复里向用户复述 UI 行为,角色错位。对照组:DSH 把 UI 需求做成**必填参数**(pwsh 的 description 字段,模型为命令写一句给用户看的摘要),需求进了参数层而不是描述层。

另注:BoenMind 系统提示本身紧凑(角色 + 技能 + 工作目录),工具参数 schema 也保留完整——问题集中在上述协议与描述层,不在系统提示规模。

## 5. DSH 详档(本机源码逐字提取 ✅)

安装根:`C:\Users\Boen\AppData\Roaming\fnm\aliases\default\node_modules\@deepseek-ai\dsh\`(Cordis 微内核 + 内嵌 @deepseek-ai/dsh-tool-* 插件包);用户数据 `C:\Users\Boen\.dsh\`;preset 配置 `config/agent-presets/code/agent.cordis.yml`。

### 5.1 标准工具集(code preset 挂载,7 件基础)

平台感知:`tool-bash` 在 win32 上 disabled,`tool-pwsh` 在非 win32 上 disabled——同一会话永远只看到一个终端工具,不给模型平台选择题。

**fs 四件套**(dsh-tool-fs 的 defineTool 原文):
- **read**:"Read a UTF-8 text file and return line-numbered content." 参数 file_path(必填)/ offset(1-based)/ limit;
- **write**:"Create or fully replace a UTF-8 text file." 参数 file_path / content;
- **edit**:"Edit an existing UTF-8 text file by replacing literal text." 参数 file_path / old_string / new_string / replace_all——**与 BoenMind fs_edit 参数同形,但描述是英文一句话,零噪音**;
- **read_image**:独立图片读取工具。

**搜索两件套**(dsh-tool-fs-search):
- **glob**:"Find files matching glob patterns (like ls or dir but with wildcards and full recursion)…" 附带行为契约:返回绝对路径、不含目录、mtime 排序、结果超上限自动落盘到文件并返回文件路径;
- **grep**:"Search file contents using regex patterns (like grep/ripgrep)…" 行为契约:结果上限、超限落盘、以及一句正向导流:**"Use the read tool on the matched file to see the full context."**(教模型下一步用 read 接力)。

**终端**(dsh-tool-pwsh)参数设计最值得 BoenMind 借鉴:
- command(必填)、**description(必填!模型要为这次命令写一句 UI 摘要给用户看)**、timeoutMs、workdir、run_in_background、sandbox_permissions(需要提权时说明用途)。
- 即:UI/权限需求全部进了**参数层**,模型填表即可;工具描述保持纯净。这正是 BoenMind 把"弹出审批卡片"写进描述的反面教材。

### 5.2 系统提示(persona 一句话起步)

`agent-presets/code/agent.cordis.yml`:persona = `You are a coding agent powered by the {{model}} model. Your working directory is {{cwd}}.` 其余按需注入:plan mode 专用段落、compaction 段、**工具结果 pruner**(thresholdChars 8192 / head 4096 / tail 1024,超长结果保留头尾中间打点)、以及 subagent / todo / web / skill / goal / ask_user 等按 preset 挂载。

### 5.3 Code Mode(DSH 最狠的一招)

preset 的 tool-presentation(mode: code):模型不再逐个调用工具,而是写一段 TypeScript 程序对着生成的 SDK 一次跑完。原文注释:"a sequence that would be five round trips becomes one"——5 次往返变 1 次,token 与延迟双省。

### 5.4 ⚠未核实项

- 会话原文(`C:\Users\Boen\.dsh\sessions\**\session.jsonl.zstd`)因本机无 zstd 解码器未解包;
- v1 曾称"DSH edit 失败返回就近上下文辅助纠错"未复核。
- 以上不影响 5.1~5.3 的源码级结论。

## 6. Pi Agent 详档(GitHub 源码核实 ✅)

仓库:`badlogic/pi-mono`(monorepo:pi-agent-core / pi-coding-agent / pi-ai)。

### 6.1 工具集(packages/coding-agent/src/core/tools/,8 件)

bash.ts / powershell.ts(平台变体)、read.ts、write.ts、edit.ts、find.ts、grep.ts、ls.ts,另有 truncate.ts / output-accumulator.ts(输出截断管家)。

- **read**:path / offset(1-indexed) / limit;文本分页截断;图片文件自动转 vision 附件;
- **write**:path / content,自动创建父目录;
- **edit**:path + **edits: Array<{oldText, newText}>**,工具参数描述原文:"Multiple edits to apply to the file. Each edit must be unique and non-overlapping within the file… All oldText values must match the original file content exactly, not the result of previous edits in the same call. This lets you make multiple changes in a single tool call."——单次调用多处原子补丁,全部基于**原始文件快照**匹配,无中间态行号漂移;
- **bash**:"Execute a bash command exactly as provided without wrapping, escaping, or interpretation."(命令逐字执行,零包装)。

### 6.2 系统提示结构(system-prompt.ts ✅)

`systemPrompt = header + personality + toolSnippets + guidelines`。toolSnippets 是当前激活工具的**单行摘要清单**;guidelines 含(原文):
- "Use read to examine files instead of cat or sed."
- "Use edit for precise changes (edits[].oldText must match exactly)."
- "Keep edits[].oldText as small as possible while still being unique in the file."
- "Use bash for file operations like ls, rg, find."(仅无专职工具时出现)
- "Show file paths clearly when working with files."
- "Be concise in your responses."

整份系统提示 + 8 件工具 schema 合计约千余 token 量级,是四家中最省的;每轮工具结果原样回传,前缀稳定,prompt caching 命中率天然最高。

## 7. Hermes Agent 详档(VPS 实机 v0.20.6 只读核实 ✅)

安装:`/usr/local/lib/hermes-agent`(Python 3.11 venv,editable install,git 仓库)。`tools/` 目录 80+ 文件;核心集(`toolsets.py::_HERMES_CORE_TOOLS`)= **read_file / write_file / patch / search_files / terminal / process**,其余(browser/vision/memory/cronjob/kanban/skills…)按需挂载。

- **read_file**:"Read a text file with line numbers and pagination. Use this instead of cat/head/tail in terminal. Output format: 'LINE_NUM|CONTENT'…" 约 100K 字符预算,超限返回 next_offset 续读;docx/xlsx/pptx/pdf/odf/rtf/epub/ipynb 自动解析;
- **write_file**:"Write content to a file… Use instead of echo/cat heredoc… Creates parent directories. OVERWRITES entire file — use patch for targeted edits. Auto-runs syntax checks…" 结果带 `verified:true`(内容哈希已确认),描述明说不要回读验证——用**正向确认**替代 BoenMind 式负向禁令;
- **patch**:"Targeted find-and-replace edits in files. Use this instead of sed/awk in terminal. Uses fuzzy matching…"(模糊匹配存在✅;具体策略数⚠未核);源码注释实锤**模型感知裁剪**:"advertising it to everyone cost every other session ~148 tok/call"(dynamic_schema_overrides 按 OpenAI / 其他模型切换 schema 形态);
- **terminal**:command 参数,前台/后台会话;
- **tool_search.py**:海量工具场景下低频工具转为 deferred,只暴露 tool_search / tool_describe 等网关元工具按需拉取,列表预算 = min(上下文的 5%, listing_max_tokens),核心六件永不 deferred——这是 MCP 工具树膨胀场景的标准解法;
- tool_call_id 在 model_tools.py 全链路存在,工具结果走标准 function calling 回传(XML `<tool_call>` 是 Hermes **微调模型**的训练格式,与 hermes-agent 运行时是两回事,勿混淆)。

## 8. BoenMind 缺点清单(按修复优先级)

| # | 问题 | 证据位置 | 对模型的影响 | 三家的做法 |
|---|---|---|---|---|
| 1 | Role::Tool → "user" 伪装 | openai_http.rs:265 / :400 | 工具输出被当作用户插话,工具因果链断裂 | 全部原生 role:tool + tool_call_id |
| 2 | 回喂丢 tool_calls(assistant 只回 content) | turn.rs:374-377 + WireMessage 无字段 | 模型看不到自己发起过什么调用,只能猜这条 user 消息是什么 | assistant 消息携带 tool_calls 数组 |
| 3 | 成功即贴"不要再次调用" | turn.rs:586 | 链式调用被斩断,复杂任务做一半 | 零附加语;循环上限兜底(BoenMind 已有 MAX_TOOL_ROUNDS=5) |
| 4 | MCP 工具描述全丢("只读直通工具") | registry.rs chat_tools 三元组 + turn.rs 套话 | 联网搜索/上下文透视只能盲猜乱用 | 各家均有真实一句话描述 + 行为契约 |
| 5 | 审批 UI 入工具描述 | turn.rs:249 | 模型对用户复述"我给你弹出审批卡片" | DSH:UI 摘要做成必填参数由模型自填 |
| 6 | 工具名非常规(fs_read / system_exec) | openai_name 点号转下划线 | 主流模型对 read/edit/bash 命名有训练亲和 | 各家直接用常规短名 |

## 9. 改进方案(分阶段;遵守本仓 AGENTS.md 合同冻结纪律)

**P0 协议还原(收益最大,优先做):**
1. bm-contract 的 Message 增加可选 `tool_call_id` / `tool_calls` 字段(合同 Minor:只增不破,走 validate.py 全绿);
2. openai_http.rs:模型挂载 tools 时 `Role::Tool` 直出 `role:"tool"` 并对齐 tool_call_id;仅为已知不兼容网关保留降级开关;
3. turn.rs 回喂 assistant 消息时携带 tool_calls 结构;
4. 删除 turn.rs:586 成功路径的防复读后缀;审批成功/拒绝两句定向引导可保留;死循环防护交给已有的 MAX_TOOL_ROUNDS。

**P1 描述与命名:**
5. chat_tools 把能力 manifest 的 description 带出来,MCP 工具用自描述,淘汰"只读直通工具"套话;
6. 审批提示从工具描述移到回传结果文本(拒绝路径已在这样做,方向正确);
7. 评估 wire 层主流短名(read/edit/write/exec)与合同点号名并存展示(合同名不动,只改对外序列化名)。

**P2 体验升级(可另开里程碑):**
8. fs_edit 升级 edits 数组(对标 Pi;向后兼容保留单处字段);
9. 长输出统一 pruner(对标 DSH 的 8192/4096/1024);
10. 远期:Code Mode 式"脚本合并往返"(DSH)与 tool_search 渐进披露(Hermes)。

## 10. 附录

- **Token 口径**:本文只做数量级判断,不做精确 tokenizer 统计(中文约 1~2 token/字,英文约 0.25 token/字符;中文描述在 token 上并不吃亏)。v1 的"中文描述 = token 冗长"说法在此正式撤回。
- **未核事项清单**(后续 AI 勿当已证事实引用):DSH 会话原文未解包(本机无 zstd);Hermes patch 模糊匹配的具体策略数;Hermes providers 侧 role:"tool" 最终序列化未逐行核;Hermes 系统提示全文未逐字提取。
- **本报告不修改任何产品代码**;改进项落地须走 BACKLOG 登记 → 合同 Minor 评审 → 里程碑实现的既有流程(AGENTS.md 硬纪律 1/5/8)。
- 报告版本:v2(2026-09-06)。v1 为同日初版,其未核实表述已在本版 §0 逐条修正。
