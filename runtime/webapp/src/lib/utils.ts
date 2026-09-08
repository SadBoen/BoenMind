import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** 门户 401 会话失效统一重定向 */
export function redirectToLogin(): never {
  window.location.href = "/login";
  throw new Error("需要登录");
}

