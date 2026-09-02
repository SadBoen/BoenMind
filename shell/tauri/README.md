# BoenMind Windows 桌面壳(Tauri v2)

ADR-0009:桌面只出 Windows 壳;**前端与 Web UI 共用同一代码库**
(`runtime/webapp`,经 `vite build` 产出 `dist/`,禁止分叉)。壳本身不带业务逻辑——加载同一
静态页,页面内的 server 地址/令牌字段指向本机或 VPS 上的 boenmind-server。

## 复现构建

```bash
# 0) 先构建前端(webapp 目录)
cd runtime/webapp
npm run build

# 1) 安装 tauri-cli(一次)
cargo install tauri-cli --locked

# 2) 构建(在本目录执行;需 Rust MSVC 工具链)
cd shell/tauri
cargo tauri build
# 产物:src-tauri/target/release/boenmind-shell.exe(及 bundle 安装包)
```

## 结构

- `tauri.conf.json`:frontendDist 指向 `../../runtime/webapp/dist`(webapp 构建产物);
  窗口加载 `index.html`,所有业务请求由页面 JS 直连 boenmind-server HTTP API。
- `src-tauri/src/main.rs`:最小 Tauri 入口(窗口 + 资源)。

## 安全边界

壳不持有令牌/密钥;令牌由用户在页面内输入(auth.v0_1),存于 WebView
localStorage(与浏览器形态一致)。壳与 server 之间的信任边界 = 网络边界
(ADR-0009)。
