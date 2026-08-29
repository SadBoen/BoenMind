# M3 实现规格 v1.0(实现者自主冻结)

> 第 2 层工件:M3(统一 Wire API、CLI 与跨平台启动)的技术栈、crate 划分与
> 任务分解。地位在基线(第 0 层)与合同库(第 1 层)之下;冲突以上两层为准。
> 上游输入:基线 §18-M3、ADR-0009(部署形态:VPS 托管 + Web/交互式 TUI Surface
> + Windows Tauri 壳;M3 增 HTTP 传输+鉴权合同)、M1/M2 遗留条件账本。
> 状态:**v1.0(2026-08-29 实现者自主冻结)**。治理变更(经用户确认):
> 技术规格不再送用户评审——基线已锁定"做什么"(§18-M3 六子项与验收标准、
> ADR-0009 部署形态),本规格只记录"怎么做",属实现者职权;用户仅裁决
> 产品体验与方向层面的议题(以大白话提出)。开放裁决点按 §9 默认路径执行,
> 全部记入 PENDING 供事后知情。

## 1. 范围与形态裁定

基线 §18-M3 六子项 + ADR-0009 增项。核心形态变化:**Runtime 从进程内库
升级为守护进程服务**(`boenmind-server`,持有 L2 Core + HTTP 端点),
Surface(CLI / Tauri 壳)是独立进程客户端,经 Surface Protocol 访问——
这是基线 §14「Surface 与核心解耦」的第一次真实落地。

| 子项 | M3 交付 | 裁定说明 |
|---|---|---|
| M3.1 Surface Protocol | HTTP 传输合同(POST 映射 7 个 Wire 方法)+ SSE 事件流 | 合同库增发 surface 传输合同(Minor) |
| M3.2 CLI 命令 | `boenmind` CLI:session / agent 命令组全量;**task / approval 命令组随 M4/M5 增发**(对象尚不存在,CLI 框架预留子命令位) | 基线四组命令的对象分属 M4/M5,这是对象边界事实,不是范围缩水 |
| M3.3 watch + resume cursor | SSE 订阅事件流(增量自 resume cursor)+ session.resume 跨进程(M2 已备) | 见 §5.2 |
| M3.4 Tauri Desktop 最小界面 | Tauri 2 壳:会话列表/创建、发送输入、事件流视图(聊天形态) | 最小可用,非最终 UX |
| M3.5 三平台打包启动 | CLI + server 三平台二进制(CI 制品);Tauri Windows 安装包 | 其余平台安装包随 M8 发行质量 |
| M3.6 跨平台适配 | 路径(dirs crate)/进程信号(优雅停机统一走应用层协议)/编码(UTF-8) | 测试矩阵 S 系列对应 |
| ADR-0009 增项 | HTTP 传输合同 + 鉴权合同(bearer 令牌) | 见 §5.3 |

非目标:Web 正式 UI(M8)、语音/通知 Surface(后续)、TLS 进程内终止
(默认经反向代理终止,见 §5.3)、多用户/账号体系(基线 4.4 单用户)。

## 2. 技术栈

| 项 | 选择 | 理由 |
|---|---|---|
| HTTP 框架 | axum(tokio 生态事实标准) | 与现有 tokio 栈同构;SSE 支持内建 |
| CLI 框架 | clap v4(derive) | 标准;子命令位预留 task/approval |
| CLI→服务端客户端 | reqwest(rustls;已在依赖树) | 复用 |
| 事件订阅 | SSE(text/event-stream) | 单向推送最小实现;HTML/CLI 通用 |
| 桌面壳 | Tauri 2(Windows 优先,ADR-0009) | 复用 Web 前端代码基座 |
| 前端 | 静态 TypeScript + Vite(无框架起步) | 最小界面不需要重框架;M8 再演进 |

## 3. 仓库结构(增量)

```text
runtime/crates/
  bm-surface-http/     # Surface Protocol HTTP 绑定:axum 路由 ↔ 7 个 Wire 方法 + SSE
  bm-cli/              # boenmind CLI(客户端):clap 子命令 → reqwest → 信封
  bm-runtime/          # 增量:bin 拆分 boenmind-server(守护进程组装)
web/                   # Tauri 壳 + 前端(M3.4;Windows 优先)
boenmind-contracts/    # 增发:surface/transport 合同(Minor,见 §5.3)
```

Server 单二进制 `boenmind-server`:组装 Runtime(bm-core 全套)+ bm-surface-http,
默认绑定 `127.0.0.1:7531`(端口可配)。CLI 同名 `boenmind`(M8 前与 server
同仓库异二进制)。

## 4. HTTP 传输映射(M3.1)

| Wire 方法 | HTTP | 说明 |
|---|---|---|
| 7 个方法统一 | `POST /rpc/{method}` | body = RequestEnvelope,响应 = ResponseEnvelope(信封逐字节复用,不另造协议) |
| events 流 | `GET /events/{session_id}?since_seq=N`(SSE) | 每条 event_envelope 为一个 event;心跳注释行保活;断线客户端以 since_seq 重连(等价 resume cursor) |
| 健康探针 | `GET /health` | 无鉴权;返回版本与 runtime 状态(供 L0 形态预演) |
| 错误映射 | HTTP 200 恒定 + 信封 ok/error | 传输层不重复定义业务错误语义(信封已是合同);仅 401(鉴权)/404(未知 method 路径)/503(非 running)走 HTTP 状态码 |

## 5. 关键设计决策

### 5.1 鉴权合同(Minor 增发) **[裁决]**

- 单用户 bearer 令牌:server 启动时若无令牌文件则生成 256bit 随机令牌,
  写 `data_dir/token`(Windows 限制 ACL 为当前用户;POSIX 0600)。
- 所有 `/rpc` 与 `/events` 请求须 `Authorization: Bearer <token>`;
  CLI 首次连接读取令牌文件(本机场景)或用户配置(远程场景)。
- 令牌轮换:`boenmind server rotate-token`(server 子命令,重写文件并热加载)。

### 5.2 watch 语义(M3.3) **[裁决]**

`GET /events/{session_id}?since_seq=N` 即 watch:连接期间增量推送,
断线后以任意 since_seq 重连——resume cursor 与 watch 共用同一事件序语义,
无独立订阅状态(服务端无 per-client 状态,重启无损)。SSE `id:` 字段
即 event_seq,浏览器 EventSource 自动重连带上 Last-Event-ID。

### 5.3 TLS 终止位置 **[已裁决:回环绑定 + 反代终止]**

默认绑定回环 + 反向代理(如 Caddy)终止 TLS 后转发——单用户场景最简、
不把 rustls 证书管理引入 M3。进程内 rustls 终止随 Web Surface 上 VPS
的 M8 部署形态再定。已定案(M3 范围);进程内 rustls 随 M8 部署形态再议。

### 5.4 CLI 退出不取消任务(M3.2 验收,基线原文) **[裁决]**

CLI 是无状态客户端:退出只断 HTTP 连接,server 端 operation 继续运行
(INV-6 的传输层复刻)。验收 = 发起回合后立即退出 CLI,重连 watch 可见
回合完成事件。

### 5.5 合同增发清单(Minor,只增) **[裁决]**

1. `surface/transport.v0_1.schema.json`:HTTP 绑定(SSE 帧格式、
   method→路径映射表、鉴权头)。
2. `surface/auth.v0_1.schema.json`:令牌格式与存储位置约定。
3. 注册表事件:无新增(M3 复用 M1/M2 事件集;surface 生命周期事件
   随 M4 权限体系再议)。

## 6. 任务分解与顺序

```text
T0  合同增发:transport + auth schema(含 validate.py 通过 + 镜像同步)
T1  bm-surface-http:axum 服务,/rpc/{method} 七方法映射 + Bearer 中间件
T2  boenmind-server bin:组装 + 令牌生成/加载 + /health + 优雅停机
T3  bm-cli:clap 骨架 + session create/resume/close + agent send/cancel +
    operations get(信封逐字节断言测试)
T4  SSE /events 流 + 断线重连;CLI watch 子命令(M3.3)
T5  端到端测试:CLI ↔ server 跨进程(信封、退出不取消、watch 重连)
T6  web/ Tauri 2 壳 + 最小三视图(会话/发送/事件流),Windows 包
T7  CI 增发布 job:三平台 CLI/server 制品 + Tauri Windows 包(M3.5)
T8  M3.6 跨平台路径/信号/编码适配核对(测试矩阵 S 系列逐条)
T9  全量回归 + 性能冒烟复跑(P-01/02/03 vs 基线劣化<25%)
T10 §19 回看 + PENDING 裁决清账 + AGENTS.md + tag m3-surface-cli
```

依赖:T0 → T1 → T2 → T3 →(T4,T5)→ T6 → T7 → T8 → T9 → T10。

## 7. 验收面(基线 M3 通过条件)

- GUI(Tauri)与 CLI 使用同一套 Runtime API:两者均只经 HTTP Surface,
  无第二条通路(代码结构保证:bm-cli 与 web 均为纯客户端);
- CLI 退出不默认取消任务(t5 断言);
- 三平台安装、启动、执行、退出、恢复(CI 制品 + S 系列矩阵);
- 状态和日志语义一致(信封/事件逐字节 = M1/M2 已冻结形态)。

## 8. 合同解读条款(实现期裁决,回看复核)

1. **HTTP 状态码恒 200**(业务语义全在信封):Wire 信封已是错误合同,
   传输层不二次翻译——避免两套错误语义漂移。
2. **SSE 断线语义 = resume cursor 语义**:订阅无服务端状态,重连完全由
   客户端 since_seq 驱动;服务端重启后客户端重连自然衔接(M2 持久化收益)。
3. **task/approval 子命令位预留但不实现**:对象在 M4/M5;提前实现必然
   产生对不存在对象的猜测性接口(违背"开工时写"纪律)。

## 9. 裁决定案(2026-08-29,原开放项)

- D-M3-3 TLS 终止:回环绑定 + 反代终止,已定案。
- D-M3-1 watch 形态:SSE,已定案。
- D-M3-2 Tauri 范围:最小三视图(会话/发送/事件流),已定案;UX 演进归 M8。
- 合同增发(T0):Minor 纯追加,照常执行。
