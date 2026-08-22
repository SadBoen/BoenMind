import { DEFAULT_SETTINGS, type Settings } from "../types";

const KEY = "boenmind.settings.v1";

export function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    const parsed = JSON.parse(raw) as Partial<Settings>;
    return {
      ...DEFAULT_SETTINGS,
      ...parsed,
      providers: Array.isArray(parsed.providers) ? parsed.providers : DEFAULT_SETTINGS.providers,
      bgUrl: typeof parsed.bgUrl === "string" ? parsed.bgUrl : DEFAULT_SETTINGS.bgUrl,
    };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

export function saveSettings(s: Settings): void {
  // api_key 不落 localStorage（经后端 credentials.set 保存；此处防御性剥离，
  // 任何 XSS 都读不到密钥）。
  const safe: Settings = {
    ...s,
    providers: s.providers.map(({ api_key: _omit, ...rest }) => rest),
  };
  localStorage.setItem(KEY, JSON.stringify(safe));
}

export function applyDomSettings(s: Settings): void {
  const root = document.documentElement;
  root.dataset.theme = s.material === "glass" ? "glass" : "paper";
  root.dataset.style = s.style;
  root.dataset.material = s.material;
  root.dataset.font = s.fontSize;
  root.dataset.bg = s.background;
  root.style.setProperty("--g", String(s.glassG));
  // 背景图：优先用户设置，空时回落到内置春天绿叶图
  root.style.setProperty("--bg-url", s.bgUrl ? `url("${s.bgUrl}")` : "url(\"/bg-spring-leaves.svg\")");
  const hue = s.glassHue;
  root.style.setProperty("--glass-rgb", hue === 0 ? "0, 0, 0" : `${hue}, ${Math.max(0, hue - 8)}, ${Math.max(0, hue - 4)}`);
}
