# BoenMind 三平台测试矩阵（M0.3）

> 版本 v0_1（2026-08-28 定标；随里程碑回看更新）。
> 目的：落实基线 §18-M0.3「定义 Windows / Linux / macOS 测试矩阵」。
> 原则：**按可运行检查点验收，不按"模块写完"验收**（基线 §18）。
> 平台优先级：P0 = 每次里程碑必须全绿；P1 = 里程碑回看时必须全绿；P2 = 发布前抽测。

## 1. 平台矩阵

| 平台 | 档位 | 优先级 | 执行方式 |
|---|---|---|---|
| Ubuntu 24.04 x64 | CI 主力 | P0 | 每次提交（GitHub Actions / 本地 docker 均可） |
| Windows 11 x64 | CI | P0 | windows-latest 每日 + 里程碑前必跑 |
| macOS 14+ arm64 (Apple Silicon) | CI | P0 | macos-14 每日 + 里程碑前必跑 |
| Ubuntu 22.04 x64 | 兼容档 | P1 | 里程碑回看时跑 |
| macOS 15 x64（Intel，如可得） | 兼容档 | P1 | 里程碑回看时跑；无机器则降为 P2 记录豁免 |
| Windows 10 x64 | 抽测 | P2 | 发布前手工抽测 |
| Linux arm64 | 可选 | P2 | 发布前抽测 |

## 2. 套件 × 平台矩阵

| 套件 | 内容与判定 | Linux | Windows | macOS |
|---|---|---|---|---|
| S1 unit | 语言级单测全绿 | P0 | P0 | P0 |
| S2 contract | `python3 scripts/validate.py` 全绿（R1-R4；实现后校验实现符合合同） | P0 | P0 | P0 |
| S3 golden-trace | M1-GT-01 成功与超时两条场景逐条回放一致 | P0 | P0 | P0 |
| S4 crash-recovery | 对 Runtime 进程注入 SIGKILL / taskkill /F：重启后 Session 可恢复、事件不丢、无半写状态 | P0 | P0 | P0 |
| S5 log-redaction | 对包含密钥样串/隐私原文的回合跑脱敏扫描：普通日志 0 命中 | P0 | P0 | P0 |
| S6 prompt-injection | `m0/prompt-injection-cases.v0_1.md` 用例集：M1 断言（标记+脱敏+不崩溃）全过 | P0 | P0 | P0 |
| S7 perf smoke | `m0/perf-baseline.v0_1.md` 冒烟口径：启动/回合延迟不劣于基线 25% | P1 | P1 | P1 |
| S8 升级回滚演练 | 阶段二启用：generation 切换/回滚脚本演练 | — | — | —（阶段二转 P0） |

## 3. 平台专项注意（实现与测试都要过一遍）

```text
路径：      大小写不敏感(Windows/macOS 默认)与分隔符差异;禁止硬编码 '/'
文件锁：    Windows 独占打开会锁死 SQLite/日志轮转;打开句柄必须可共享或短持有
信号：      SIGTERM 在 Windows 无对应物;优雅停机走应用层协议而非信号
进程树：    Windows job object / POSIX process group,二选一抽象,杀树语义对齐 S4
编码：      一律 UTF-8 无 BOM;Windows 控制台输出显式设码页
时钟：      休眠唤醒后单调时钟重算 deadline(基线 8.3),三平台行为不同,进 S3 边界用例
防火墙/UAC：本地回环监听(Wire API)首次可能弹窗,安装文档说明
```

## 4. 执行与留痕

- 每次里程碑回看（基线 §19）附三平台执行记录（日期/提交号/套件结果/豁免清单）。
- P0 套件存在红项时，不得宣布里程碑完成；豁免必须写明理由与复查里程碑。
- 本矩阵 v0_1 为定标稿；新增套件 = 增行，不改既有行（与合同冻结纪律一致）。

## 5. 增补(2026-08-29,ADR-0009)

平台增行:

| 平台 | 档位 | 优先级 | 执行方式 |
|---|---|---|---|
| 现代浏览器(Chromium/Firefox 最新版) | Web Surface(访问 VPS/本机 Runtime) | P1(自 M3 传输合同落地起) | 手工+Playwright 冒烟 |
| Windows 11 x64(Tauri 壳,复用 Web 前端) | 桌面打包形态 | P1(自 M8 起) | 手工抽测 |

套件增行:

| 套件 | 内容与判定 | Linux | Windows | macOS |
|---|---|---|---|---|
| S9 web-surface 冒烟 | 令牌鉴权通过/未授权被拒;事件流连接与断线重连(游标恢复);审批交互可用 | P0(M3 起) | P0(M3 起) | P0(M3 起) |
