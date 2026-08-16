import { useEffect, useRef } from "react";
import { useAppStore } from "@/stores/app-store";

/**
 * 统一轮询（2026-08-16 PERF-001 修复：health/MCP/Steward 三处各写 setInterval，
 * 断网仍每 5s 发失败请求、无退避语义）。setTimeout 链（节奏可变）：
 * - pauseOnOffline：后端离线时降频到 max(interval×3, 15s) 保持恢复探测，
 *   恢复在线立即回正常节奏（online 状态由 fn 自身更新，链上每次重算）。
 * - 挂载首拉由调用方负责（hook 只管节奏）。
 */
export function usePolling(
  fn: () => void | Promise<void>,
  intervalMs: number,
  pauseOnOffline = false,
) {
  const online = useAppStore((s) => s.online);
  const fnRef = useRef(fn);
  fnRef.current = fn;

  useEffect(() => {
    let timer = 0;
    // offline 时降频（不能停死：否则后端恢复后检测不到，状态栏永远"未连接"）
    const tick = pauseOnOffline && !online ? Math.max(intervalMs * 3, 15000) : intervalMs;
    const loop = () => {
      timer = window.setTimeout(loop, tick);
      void fnRef.current();
    };
    timer = window.setTimeout(loop, tick);
    return () => clearTimeout(timer);
  }, [intervalMs, pauseOnOffline, online]);
}
