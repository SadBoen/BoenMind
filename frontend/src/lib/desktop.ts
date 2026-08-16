/** 桌面环境判定（Tauri 内嵌 webview）：本地使用，不启用 UI 登录门。
 *
 * 与网页版共用：App.tsx（登录门直放行）、设置中心「安全」页（桌面隐藏）。
 */
export function isDesktopShell(): boolean {
  return (
    typeof window !== "undefined" &&
    Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
  );
}
