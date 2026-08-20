import { useEffect, useState } from "react";
import { rpc } from "../client";
import { invokeTauri, isTauri, type UpdateCheckResult } from "../tauri";

// 底部状态栏：与 App 无关的应用外壳层（层级同导航栏）。
// 显示：后台链接状态（RPC 心跳 + WS mux 连接）+ 服务版本 + 升级提示（版本变化时）。
// 升级提示数据源：host.describe 的 version；本地记录上次见到的版本，变化即提示。
// 链接状态指示：WiFi 图标（三弧+圆点），颜色语义沿用三态：绿=已连接 / 黄=连接中 / 红=断开。
//
// 桌面版（Tauri 无边框窗口）：整条状态栏是可拖拽区（data-tauri-drag-region），
// 交互元素（检查更新按钮）声明 no-drag，避免拖拽与点击冲突。

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
  const [updateState, setUpdateState] = useState<"idle" | "checking" | "ready" | "none" | "error">("idle");
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);

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

  const checkForUpdate = async () => {
    setUpdateState("checking");
    const res = await invokeTauri<UpdateCheckResult>("check_update");
    if (res === null) {
      // 非 Tauri 环境（浏览器直连 web-server）：无桌面壳更新逻辑。
      setUpdateState("idle");
      return;
    }
    if (res.available) {
      setUpdateState("ready");
      setUpdateVersion(res.version ?? null);
    } else {
      setUpdateState("none");
    }
  };

  const installUpdate = async () => {
    // 触发 Tauri updater 下载并安装（安装器会替换可执行文件并重启）。
    const ok = await invokeTauri<boolean>("install_update");
    if (ok !== true) setUpdateState("error");
  };

  const linkLabel =
    link === "connected" ? "已连接" : link === "connecting" ? "连接中…" : "连接断开";
  const linkClass =
    link === "connected" ? "ok" : link === "connecting" ? "pending" : "down";

  return (
    <div className="bm-statusbar" data-tauri-drag-region>
      <div className="bm-statusbar-left" data-tauri-drag-region>
        <span className={`status-wifi ${linkClass}`} title={linkLabel}>
          <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M5 12.55a11 11 0 0 1 14.08 0" />
            <path d="M1.42 9a16 16 0 0 1 21.16 0" />
            <path d="M8.53 16.11a6 6 0 0 1 6.95 0" />
            <circle cx="12" cy="19" r="1.4" fill="currentColor" stroke="none" />
          </svg>
        </span>
        <span className="status-link" data-tauri-drag-region>{linkLabel}</span>
        {info?.version && <span className="status-item" data-tauri-drag-region>v{info.version}</span>}
        {info?.provider && (
          <span className="status-item" data-tauri-drag-region>
            {info.provider}/{info.model}
          </span>
        )}
      </div>
      <div className="bm-statusbar-right" data-tauri-drag-region>
        {upgrade && <span className="status-upgrade" data-tauri-drag-region>{upgrade}</span>}
        {isTauri() && (
          <span className="status-item update-item" data-tauri-no-drag>
            {updateState === "checking" ? (
              <span className="update-btn-disabled">检查更新…</span>
            ) : updateState === "ready" ? (
              <a className="update-btn" onClick={installUpdate} role="button">
                下载更新 v{updateVersion}
              </a>
            ) : updateState === "none" ? (
              <span>已是最新</span>
            ) : (
              <a className="update-btn" onClick={checkForUpdate} role="button">
                检查更新
              </a>
            )}
          </span>
        )}
        {info && info.attachedSessions !== undefined && (
          <span className="status-item" data-tauri-drag-region>会话 {info.attachedSessions}</span>
        )}
      </div>
    </div>
  );
}