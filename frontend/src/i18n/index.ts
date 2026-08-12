/**
 * i18n 初始化：4 语言（zh / en / ja / ko）直接打包，桌面端（Tauri）无网络依赖。
 *
 * 语言存储沿 theme 模式：
 * - localStorage `boenmind.lang` 即时生效（启动读）
 * - 后端 config.toml `lang` 字段持久化（桌面/网页一致，启动 loadConfig 后以后端为准）
 */
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { zh } from "./locales/zh";
import { en } from "./locales/en";
import { ja } from "./locales/ja";
import { ko } from "./locales/ko";

export const LANGS = ["zh", "en", "ja", "ko"] as const;
export type Lang = (typeof LANGS)[number];

/** 语言自身名称（不自译） */
export const LANG_NAMES: Record<Lang, string> = {
  zh: "中文",
  en: "English",
  ja: "日本語",
  ko: "한국어",
};

/** 语言 → Intl locale（日期时间格式化用） */
export function intlLocale(lang: string): string {
  const map: Record<string, string> = { zh: "zh-CN", en: "en-US", ja: "ja-JP", ko: "ko-KR" };
  return map[lang] ?? "zh-CN";
}

const LANG_STORAGE_KEY = "boenmind.lang";

export function isLang(v: unknown): v is Lang {
  return typeof v === "string" && (LANGS as readonly string[]).includes(v);
}

export function getStoredLang(): Lang {
  try {
    const v = localStorage.getItem(LANG_STORAGE_KEY);
    if (isLang(v)) return v;
  } catch {
    /* SSR/禁用存储时忽略 */
  }
  return "zh";
}

// v26 起 init 恒为异步（initImmediate 选项已删除），在 Promise 完成后同步 <html lang>
void i18n
  .use(initReactI18next)
  .init({
    resources: {
      zh: { translation: zh },
      en: { translation: en },
      ja: { translation: ja },
      ko: { translation: ko },
    },
    lng: getStoredLang(),
    fallbackLng: "zh",
    interpolation: { escapeValue: false },
  })
  .then(() => {
    document.documentElement.lang = intlLocale(i18n.language);
  });

// 语言变化时同步 <html lang>（index.html 硬编码 zh-CN 被覆盖）
i18n.on("languageChanged", (lng) => {
  document.documentElement.lang = intlLocale(lng);
});

/** 切换语言：写 localStorage + i18next 生效（await 完成后 i18n.language 即为新值） */
export async function applyLang(lang: Lang): Promise<void> {
  try {
    localStorage.setItem(LANG_STORAGE_KEY, lang);
  } catch {
    /* ignore */
  }
  await i18n.changeLanguage(lang);
}

export default i18n;
