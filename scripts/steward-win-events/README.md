# Steward Windows 系统事件采集器（雏形）

宿主侧"OS 层主动汇报通道"的第一版（架构 §14.1 三件套 ②），配套 bm-server 的
`POST /api/steward/inject` 路由（Steward 轮 d6ba73d 已落地）。桌面壳侧的正式
实现（壳内集成采集）等壳侧代码就位后替换本脚本——本脚本是独立可用的过渡形态。

## 采集什么

| 事件 | 日志源 | 事件 ID | 管家收到的文本 |
|---|---|---|---|
| 锁屏 / 解锁 | Microsoft-Windows-Winlogon/Operational | 4800 / 4801 | `session-lock` / `session-unlock` |
| 远程会话连入/断开 | 同上 | 4802 / 4803 | `session-remote-connect` / `-disconnect` |
| 睡眠 / 唤醒 | System（Kernel-Power） | 42 / 107 / 507 | `power-sleep` / `power-resume` / `power-resume-boot` |

投喂文本带时间戳 + 截断（500 字符上限，防撑爆模型上下文）；事件按时间排序，
`$env:TEMP/bm-steward-events-last.txt` 记录游标（跨次运行去重，首次回看 5 分钟）。

## 用法

```powershell
# 一次性（配合任务计划程序，如系统启动后跑一次）
powershell -ExecutionPolicy Bypass -File collector.ps1 `
  -Endpoint http://127.0.0.1:17321 -SessionId <管家会话ID>

# 常驻轮询（每 60s 查一次新事件；配合 -Watch）
powershell -ExecutionPolicy Bypass -File collector.ps1 `
  -Endpoint http://127.0.0.1:17321 -SessionId <管家会话ID> -Watch -IntervalSeconds 60
```

### 任务计划程序注册（开机常驻示例）

```
schtasks /Create /TN "BoenMindStewardEvents" /TR "powershell -ExecutionPolicy Bypass -File D:\96_CoderWorld\BoenMind\scripts\steward-win-events\collector.ps1 -Endpoint http://127.0.0.1:17321 -SessionId <ID> -Watch" /SC ONSTART /RU SYSTEM /RL HIGHEST
```

## 前置条件

- **管理员权限**：`Get-WinEvent` 读事件日志需管理员；无权限时静默跳过（不阻断主流程）。
- **bm-server 已起** + 管家已启用（`BM_STEWARD_SESSION` env 指定管家会话），否则
  inject 返回 400（管家未启用）并在脚本侧 Warning 提示。
- **wake_after_seconds**（默认 300）：治理层夹区间 [pacing-min, pacing-max] 兜底，
  管家回合内 `set_wake` 会覆盖它（自调优先=设计如此，见 HANDOFF §〇·五 30）。

## 与桌面壳的交接

正式实现 = 桌面壳内监听系统会话事件（壳是常驻进程，无需任务计划程序），
事件形状与本脚本一致（名称 + 时间戳 + 截断正文）即可，inject 契约不变。
