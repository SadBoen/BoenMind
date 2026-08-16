import { useEffect, useRef } from "react";
import { useAppStore } from "@/stores/app-store";

/**
 * 统一轮询（2026-08-16 PERF-001 修复：health/MCP/Steward 三处各写 setInterval，
 * 断网仍每 5s 发失败请求、无退避语义）。可选 pauseOnOffline：后端离线时暂停轮询，
 * 恢复在线立即补一次再续跑。挂载即调用一次由调用方负责（hook 只管节奏）。
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
    const start = () => {
      if (!timer) timer = window.setInterval(() => void fnRef.current(), intervalMs);
    };
    const stop = () => {
      if (timer) {
        clearInterval(timer);
        timer = 0;
      }
    };
    if (pauseOnOffline && !online) {
      stop();
    } else {
      if (pauseOnOffline && online) void fnRef.current();
      start();
    }
    return stop;
  }, [intervalMs, pauseOnOffline, online]);
}
