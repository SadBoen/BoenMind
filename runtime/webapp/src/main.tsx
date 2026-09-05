import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";
// W3 主题令牌(四主题预设+专属效果);须在 styles.css 之后,覆盖生效
import "./w3/theme.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

// 门户正向引导(2026-09-06):配置了访问密码而本地会话失效(服务重启即
// 失效)时,启动即跳登录页——不等用户撞上一串 401 红条再猜去 /login
void fetch("/api/portal/state")
  .then((r) => r.json())
  .then((s: { configured?: boolean; authed?: boolean }) => {
    if (s?.configured && !s?.authed) window.location.href = "/login";
  })
  .catch(() => {
    // 门户态不可达(开发环境等)不拦截
  });
