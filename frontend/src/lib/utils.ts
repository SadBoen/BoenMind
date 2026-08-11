import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"
import { intlLocale } from "@/i18n"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

/** 会话时间：当日显示时刻，跨日显示月/日（locale 跟随界面语言） */
export function formatTime(ts: number, lang?: string): string {
  const locale = intlLocale(lang ?? "zh")
  const d = new Date(ts * 1000)
  const now = new Date()
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })
  }
  return d.toLocaleDateString(locale, { month: "numeric", day: "numeric" })
}
