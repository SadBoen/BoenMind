# 前端交互合同与验收清单(runtime/web,dsh 前端)

> 2026-08-30 起生效(用户裁决:前端交互要像后端一样全量验收)。
> 规则:改动前端或适配层后,本表逐项仿真测试,**全部通过才算完**;
> 发现不可用交互 → 修复或登记,不许静默略过。测试方式:浏览器仿真
> (独立 fresh 页面逐项跑,禁止复用上一项状态;禁用态必须给出原因)。

## 交互清单与当前状态(2026-08-30 全量仿真实测)

| # | 交互点 | 操作 | 期望 | 状态 |
|---|---|---|---|---|
| 1 | 内测声明 | 加载页面 | 不弹(已预置确认状态) | ✅ |
| 2 | 连接稳定性 | 加载后 15s | host.describe 无重连循环 | ✅ |
| 3 | 设置入口 | 点击齿轮 | 面板打开 | ✅ |
| 4 | 设置导航 | 读导航行 | 仅「模型」一节(用户裁决) | ✅ |
| 5 | 设置关闭 | 点关闭 | 面板关闭 | ✅ |
| 6 | 视图选项 | 点击 | 分组/排序菜单出现(分组方式/排序方式) | ✅ |
| 7 | 搜索会话 | 点图标展开 | 搜索输入框可用 | ✅ |
| 8 | 收起侧边栏 | 点击 | 侧栏收起、搜索图标隐藏 | ⚠️ 结果不稳定(语义点击一次成功一次无效;疑页面加载时序) |
| 9 | 添加提供方(模型节) | 面板内点击 | 可点 | ✅(存在且可点) |
| 10 | 添加自定义提供方 | 面板内点击 | 可点 | ❌ **disabled**——llm.providers 数据未满足其启用条件,待对齐 provider 行数据结构 |
| 11 | 发送消息按钮 | 未选工作区 | disabled(合理禁用) | ✅ |
| 12 | 输入卡「+」按钮 | 未装插件 | disabled(合理禁用) | ✅ |
| 13 | 添加工作区 | 点击 | 弹文件夹选择→建工作区 | ❌ **报「无法打开文件夹」**——host.pickDirectory/listDirectory 未实现;单机形态应改为文本输入路径,待实现 |
| 14 | 新会话 | 点击 | 进入会话创建流 | ⚠️ 依赖 13(先有工作区);工作区通后复测 |
| 15 | 命令按钮 | 无会话 | disabled(合理禁用) | ✅ |
| 16 | 主题切换 | 设置内 | 浅/深/跟随 | ❌ 随「通用设置」节一起被移除(用户裁决只留模型节时误伤);待裁决:恢复主题入口或接受深色固定 |
| 17 | 消息发送全链路 | 选工作区→输入→发送 | 流式回答 | ⬜ 待下一批(session 协议适配) |

## 已登记的后端待实现(dsh 方法)

- `host.pickDirectory` / `host.listDirectory`:工作区文件夹选择(单机形态
  建议改为文本路径输入,不走系统目录选择)
- settings 持久化:内存存储重启重置(应接 SQLite)
- provider 行数据结构对齐(解开 #10 的 disabled)

## 测试环境注意(踩坑记录)

- 模块缓存:改 plugins/*.js 后必须更新 index.html 内 __DSH_BOOT__ 的 rev
  值破坏缓存(无缓存头,浏览器咬旧文件)
- 每项交互必须独立 fresh 页面;复用状态会互相污染(收起侧栏后其余按钮
  全部隐藏,造成假失败)
- playwright 语义定位偶发 actionability 超时:IAB 已知问题,兜底用页面
  事件 evaluate(btn.click())


## 通讯审计修复批(2026-08-31)

审计定位 22 项问题,本批已修 14 项(后端 api_dsh.rs 为主,前端 3 处
见 SOURCE.md「BoenMind 功能修订」),全部经编译/测试/端到端冒烟验证:

| 问题 | 修复 | 验证 |
|---|---|---|
| 错误信封 details 形状不合前端封闭枚举(未适配方法缺 `issues`、`model-unavailable` 缺 `{provider,model}`)→ 前端 zod 炸成传输异常,错误文案永远到不了界面 | `not_implemented`/`dsh_error_details` 按 code 生成正确 details | 冒烟:workspace.rename/session.cancel 返回可读错误 ✅ |
| `stream/error` 帧缺 sessionId 且 details 空形 → 前端 zod 弃帧 + 连接层断流重连,模型失败完全无声 | 帧带 sessionId + 正确 details;前端 pump 不再断流,SessionManager 路由到会话错误出口 | 代码审查 + node --check ✅(真实模型失败场景待真网关复测) |
| `running` 恒 false → 无生成中指示、无法停止回合 | AgentTurnStarted/Completed/Failed/Cancelled/Interrupted/SessionClosed → `host/session-status` 帧 + session.list 同步 | 冒烟:prompt 后 running true→false ✅ |
| `session.cancel` 未实现 | 转发器跟踪最近回合 (agent_id, operation_id),映射 runtime agent_cancel;响应 `{accepted:true}` | 冒烟:无在途回合返回可读错误 ✅ |
| `session.create` cwd 恒为服务器 cwd → 前端复用判定永不命中,空会话无限增殖 | cwd 取所属工作区路径 | 冒烟:session.cwd == workspace.path ✅ |
| prompt 先落 user/message 后发输入,失败留幽灵消息、重发即重复 | send_input 成功后才落消息与空队列帧 | 代码审查 + 冒烟 ✅ |
| `session.models` 无配置返回 `current:null` 违反前端非空 schema → 选择器永久卡加载 | 前端 schema `nullable()` + `optionsOf` 护栏(后端 null 语义保留) | node --check ✅ |
| 换模型/改钥后密钥库不播种 → 下一回合静默失败 | AppState 增持 `Arc<dyn SecretStore>`,credentials.set/unset、session.selectModel 实时播种(Connector 每请求现取,免重启) | 编译 + 测试 ✅(真网关复测待) |
| llm-pi-ai `api:"openai"` 不在 schema union → 协议下拉空白 | 视图改取 union 首值 `Chat Completions (/chat/completions)` | 冒烟 ✅ |
| 命名空间视图缺 `user`/`base` → 编辑表单不回填、删除按钮永不出现 | describe/mutate 视图补 `user`(= value)与 `base`(null) | 冒烟 ✅ |
| settings.mutate 忽略 expectedRevision → 双窗口静默互相覆盖 | 乐观锁:不匹配报 `settings-conflict {ns,expected,actual}`;llm-pi-ai revision 真实自增 | 编译 ✅ |
| WS 无 Origin 校验 → 浏览器任意网页可 CSWSH 窃听会话流 | Origin 与 Host 同源校验,不符 403;无 Origin(CLI)放行 | 冒烟:恶意 403/同源 101/CLI 101 ✅ |
| SSE 回退只发心跳不转发 + axum 强提取致非 WS 请求 400(分支不可达) | 事件总线真转发 + Result 提取器;落环不断流 | 冒烟:host/mux 帧均达 ✅ |
| llm-pi-ai `applies:"live"` 失实(baseUrl 变更需重启) | 改 `applies:"restart"` | 冒烟 ✅ |

**遗留(未修,登记)**:
- `/api/*` 无 Bearer 鉴权(任意本机进程可调用;浏览器跨源 POST 已被
  CORS preflight 挡、WS 已有 Origin 校验)。VPS 部署(ADR-0009)前
  必须做鉴权设计 + 前端 token 引导;另:非 loopback 访问时前端设置
  面板被 `isLoopback` 门控整体不可用,远程部署前需一并裁决。
- 审批/工具事件未翻译(approval.requested 等不上屏,回合挂至超时)。
- 图片输入(前端发 image 块,后端只认 text)。
- 会话/工作区/翻译态全内存,重启即失;`turn/start|turn/end` 未发送
  (turn-tail 页脚缺失);`eprintln!` 诊断常驻。
- INTERACTIONS 上表 #10「添加自定义提供方 disabled」:按当前后端形状
  推演应已可点(protocols 非空 + writable),需浏览器复测更新状态。
