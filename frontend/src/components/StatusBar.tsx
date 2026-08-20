import { useEffect, useState } from "react";
import { rpc } from "../client";

// 底部状态栏：与 App 无关的应用外壳层（层级同导航栏）。
// 显示：后台链接状态（RPC 心跳 + WS mux 连接）+ 服务版本 + 升级提示（版本变化时）。
// 升级提示数据源：host.describe 的 version；本地记录上次见到的版本，变化即提示。

const VERSION_KEY = "bm_seen_version";
const HEARTBEAT_MS = 15000;

type LinkState = "connecting" | "connected" | "disconnected";

interface DescribeInfo {
  version?: string;
  provider?: string;
  model?: string;
  attachedSessions?: number;
}

export default function StatusBar() {
  const [link, setLink] = useState<LinkState>("connecting");
  const [info, setInfo] = useState<DescribeInfo | null>(null);
  const [upgrade, setUpgrade] = useState<string | null>(null);

  // 后台心跳：周期性调 host.describe 测可达 + 拿版本/provider。
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;

    const ping = async () => {
      setLink("connecting");
      try {
        const v = await rpc<DescribeInfo>("host.describe", {});
        if (stopped) return;
        setInfo(v);
        setLink("connected");
        // 升级提示：版本变化且从未记录过 → 显示一次"新版本"提示
        const seen = localStorage.getItem(VERSION_KEY);
        if (v.version && seen && seen !== v.version) {
          setUpgrade(`发现新版本 ${v.version}`);
        }
        localStorage.setItem(VERSION_KEY, v.version ?? "");
      } catch {
        if (stopped) return;
        setLink("disconnected");
      }
    };
    ping();
    timer = setInterval(ping, HEARTBEAT_MS);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, []);

  const linkLabel =
    link === "connected" ? "已连接" : link === "connecting" ? "连接中…" : "连接断开";
  const linkClass =
    link === "connected" ? "ok" : link === "connecting" ? "pending" : "down";

  return (
    <div className="bm-statusbar">
      <div className="bm-statusbar-left">
        <span className={`status-dot ${linkClass}`} title={linkLabel} />
        <span className="status-link">{linkLabel}</span>
        {info?.version && <span className="status-item">v{info.version}</span>}
        {info?.provider && (
          <span className="status-item">
            {info.provider}/{info.model}
          </span>
        )}
      </div>
      <div className="bm-statusbar-right">
        {upgrade && <span className="status-upgrade">{upgrade}</span>}
        {info && info.attachedSessions !== undefined && (
          <span className="status-item">会话 {info.attachedSessions}</span>
        )}
      </div>
    </div>
  );
}
