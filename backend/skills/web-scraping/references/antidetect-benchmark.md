# 反检测基准结论（2026-08 迁移，知识层）

> 来源一：2026 外部基准（ianlpaterson/anti-detect-browser-bench，7 工具 × 31 Cloudflare 目标 × 3 轮，
> 住宅 IP，headed 模式，零漂移）。
> 来源二：原 Hermes 本机基准（VPS RainYun 数据中心 IP，6×7 判定，2026-08-09）。
> 本文件只保留**结论层**；引擎安装/调用命令属 Hermes VPS 环境，已剔除（BoenMind 当前无浏览器引擎）。

## 三条硬结论

1. **IP 权重 > 引擎权重**：同一工具在住宅/机房 IP 结果不同（外部基准 nodriver 过 medium；
   VPS 上 5 个 Chromium 系全被 CF 403）。目标站 CF 强防护时，**换网络比换引擎更有效**。
   → 桌面端（住宅/办公 IP）值得重测 VPS 上的"无解"结论（京东、大陆工商源）。
2. **反检测的关键不是"伪装成什么"，而是"怎么被驱动"**（automation-protocol fingerprinting）：
   Playwright 的 CDP 启动序列（`Runtime.enable`、`Target.setAutoAttach`）本身可被检测，指纹修补类
   工具补不掉这一层。nodriver 绕开 Playwright 直连 Chrome DevTools WebSocket 而胜出（31 目标 28 OK）。
3. **多数站点根本不查 JS**：31 个目标里 26 个纯 HTTP 可达（curl_cffi 与 49 处 C++ 修改的
   CloakBrowser 打平）→ **先试最轻的工具，别一上来就上浏览器**。

## 判定词（防误判，实测踩过）

- nowsecure.nl CF 挑战通过后输出极短（72B）是**正常成功**——数真实内容关键词，不信 exit code/字节数
- 检测站页面自带 "robot/captcha" 说明文字（pixelscan/browserscan 的标题与说明）——判定词只保留
  真实拦截特征：`just a moment` / `attention required` / `verify you are human` / `验证`，否则全部误判 BLOCKED

## 引擎事实卡（选型参考，一句话 + 许可 + 内存）

| 工具 | 结论 | 备注 |
|---|---|---|
| **nodriver** | 外部基准最强（28/31），绕开 Playwright 直连 CDP | 需系统已装 Chrome/Chromium；AGPL-3.0（服务用需开源修改，个人自用无碍）；~500MB |
| **Camoufox** | VPS 本机 7/0 **唯一全过**（含真 CF medium） | Firefox fork + C 级指纹随机化，Firefox 形状 TLS 在 CF 是白名单（不同攻击面）；MPL-2.0；服务方式 ~40MB 空闲 |
| **Obscura** | 轻量兜底（6/1 挂 medium），85MB | 纯 Rust 无头浏览器 + V8 真渲染，单二进制，可作 Puppeteer/Playwright drop-in 替代 |
| **curl_cffi** | 纯 HTTP 26/31，21 行代码顶一个浏览器 fork | MIT；`impersonate="chrome"` 拿 TLS 形状；先确认目标不查 JS 再用 |
| **Patchright** | Playwright fork 修 CDP 泄漏（6/1） | Apache-2.0；`channel=chrome` 驱动系统 Chrome 拿真 TLS 指纹；活跃维护 |
| **CloakBrowser** | 与 star 数不符：本机 5/2 垫底（nowsecure/medium 均导航超时） | 不推荐；macOS arm64 停在 Chromium 145（2026-03 后无更新） |
| **DrissionPage** | 社区推荐用于京东 | 许可证禁商业用途，注意 |

## 弃用/清理记录（避免重踩）

- **curl_cffi**：与内置 web_extract（Firecrawl+Jina）功能重叠，已弃用（Hermes 侧决策）
- **nodriver/Patchright/CloakBrowser**：数据中心 IP 下无优势，已清理
- **BrowserOxide**：无 LICENSE 文件，法律风险排除
- **vanilla Playwright / rebrowser-playwright**：rebrowser 2024-09 后未维护；Playwright 有协议级痕迹
- **Selenium + undetected-chromedriver / puppeteer-stealth**：JD 场景已弃用
