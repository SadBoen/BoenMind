# Steward Windows 系统事件采集器（雏形，2026-08-15）
#
# 职责：采集 Windows 系统事件 → POST /api/steward/inject 投喂管家会话。
# 与 Steward 的关系：宿主侧"OS 层主动汇报通道"（架构 §14.1 三件套 ②），
# 回合语义 = Inject 源，事件文本即 message（模型可见）。
#
# 用法：
#   # 一次性跑（适合配合任务计划程序）：
#   powershell -ExecutionPolicy Bypass -File collector.ps1 -Endpoint http://127.0.0.1:17321 -SessionId <会话ID>
#   # 常驻轮询（每 60s 查一次新事件）：
#   powershell -ExecutionPolicy Bypass -File collector.ps1 -Endpoint http://127.0.0.1:17321 -SessionId <会话ID> -Watch
#
# 事件源（Windows 事件日志）：
#   - Microsoft-Windows-Winlogon/Operational  锁屏/解锁（ID 4800/4801，会话切换 4802/4803）
#   - Microsoft-Windows-Kernel-Power          睡眠/唤醒（ID 42/107/507）
#   - System                                  开机/关机（ID 6005/6006/1074）
#
# 注意：事件日志轮询（Get-WinEvent）需要管理员权限；本机交互事件
#   （当前控制台会话）无权限时静默跳过（Write-Warning 级别，不阻断）。

param(
    [string]$Endpoint = "http://127.0.0.1:17321",
    [string]$SessionId = "",
    [switch]$Watch,
    [int]$IntervalSeconds = 60,
    # 治理层夹区间外的 wake_after_seconds 会被 StewardStore 夹到 [pacing-min, pacing-max]
    [int]$WakeAfterSeconds = 300
)

$ErrorActionPreference = "Stop"
$stateFile = Join-Path $env:TEMP "bm-steward-events-last.txt"

function Get-LastEventTime {
    if (Test-Path $stateFile) {
        $raw = (Get-Content $stateFile -Raw).Trim()
        if ($raw) { return [DateTime]::Parse($raw) }
    }
    # 首次运行：回看最近 5 分钟，避免漏掉刚发生的会话切换
    return (Get-Date).AddMinutes(-5)
}

function Set-LastEventTime([DateTime]$t) {
    $t.ToString("o") | Set-Content $stateFile -Encoding UTF8
}

function Send-Inject([string]$message) {
    if (-not $SessionId) {
        Write-Warning "未提供 SessionId，事件已就绪但不投喂：$message"
        return
    }
    $body = @{
        message            = $message
        wake_after_seconds = $WakeAfterSeconds
    } | ConvertTo-Json -Compress
    try {
        Invoke-RestMethod -Uri "$Endpoint/api/steward/inject" `
            -Method Post -Body $body -ContentType "application/json" -TimeoutSec 30 | Out-Null
        Write-Host "[bm-steward] 已投喂: $message"
    } catch {
        Write-Warning "inject 失败（服务未起/管家未启用?）: $($_.Exception.Message)"
    }
}

function Collect-Events {
    $since = Get-LastEventTime
    $newest = $since
    $events = @()

    # 锁屏/解锁/会话切换（Winlogon，ID 4800=锁屏 4801=解锁 4802=远程连入 4803=远程断开）
    try {
        $events += Get-WinEvent -FilterHashtable @{
            LogName = "Microsoft-Windows-Winlogon/Operational"
            StartTime = $since
        } -ErrorAction SilentlyContinue | ForEach-Object {
            $name = switch ($_.Id) {
                4800 { "session-lock" }
                4801 { "session-unlock" }
                4802 { "session-remote-connect" }
                4803 { "session-remote-disconnect" }
                default { "winlogon-$($_.Id)" }
            }
            [PSCustomObject]@{ Time = $_.TimeCreated; Name = $name; Detail = $_.Message }
        }
    } catch { }

    # 睡眠/唤醒（Kernel-Power）
    try {
        $events += Get-WinEvent -FilterHashtable @{
            LogName = "System"
            ProviderName = "Microsoft-Windows-Kernel-Power"
            StartTime = $since
        } -ErrorAction SilentlyContinue | Where-Object { $_.Id -in @(42, 107, 507) } | ForEach-Object {
            $name = switch ($_.Id) {
                42 { "power-sleep" }
                107 { "power-resume" }
                507 { "power-resume-boot" }
            }
            [PSCustomObject]@{ Time = $_.TimeCreated; Name = $name; Detail = $_.Message }
        }
    } catch { }

    foreach ($e in ($events | Sort-Object Time)) {
        if ($e.Time -gt $newest) { $newest = $e.Time }
        $msg = "[$($e.Time.ToString('HH:mm:ss'))] Windows 事件 $($e.Name) —— $($e.Detail)"
        # 事件正文可能很长（含多行消息），截断保护模型上下文
        if ($msg.Length -gt 500) { $msg = $msg.Substring(0, 500) + "…" }
        Send-Inject $msg
    }
    Set-LastEventTime $newest
}

Write-Host "[bm-steward] 事件采集器启动 Endpoint=$Endpoint Watch=$Watch"
if ($Watch) {
    while ($true) {
        Collect-Events
        Start-Sleep -Seconds $IntervalSeconds
    }
} else {
    Collect-Events
}
