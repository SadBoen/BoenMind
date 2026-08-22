import { IconWifi, IconWifiOff } from "../lib/icons";
import { useStore } from "../store";

export function StatusBar() {
  const { state } = useStore();
  const pct = Math.round((state.usage.used / state.usage.limit) * 100);
  const r = 7;
  const c = 2 * Math.PI * r;
  const dash = (pct / 100) * c;
  return (
    <footer className="statusbar">
      <span
        className={`statusbar-link${state.connected ? "" : " is-off"}`}
        title={state.connected ? "已连接" : "已断开"}
        aria-label={state.connected ? "已连接" : "已断开"}
      >
        {state.connected ? <IconWifi width={26} height={26} /> : <IconWifiOff width={26} height={26} />}
      </span>
      <div className="usage" title={`用量 ${state.usage.used}/${state.usage.limit}`}>
        <svg className="usage-ring" viewBox="0 0 20 20" aria-hidden="true">
          <circle cx="10" cy="10" r={r} fill="none" stroke="var(--stroke)" strokeWidth="2" />
          <circle
            cx="10"
            cy="10"
            r={r}
            fill="none"
            stroke="var(--accent)"
            strokeWidth="2"
            strokeDasharray={`${dash} ${c}`}
            strokeLinecap="round"
            transform="rotate(-90 10 10)"
          />
        </svg>
        <span>
          {state.usage.used}/{state.usage.limit}
        </span>
      </div>
      <span className="brand">BoenMind</span>
    </footer>
  );
}
