# 桌面壳对标调研（2026-08-15）

> 触发：用户看到现前端后拍板方向——BoenMind 前端应是"模拟操作系统桌面"形态（Windows 式布局逻辑：
> 左下开始菜单 + 任务栏；macOS 式风格）。用户指示先找网上现成的模拟 OS 前端项目与 NAS 管理界面参考。
> 口径：本轮为**搜索核实**（星数/许可证/栈/机制为搜索归纳，未逐仓库深读源码；引用前对关键仓库复核）。
> 原则沿用：**抄机制不抄代码**；许可证红线——CC0 可抄、MIT 可抄需注明、AGPL 只学机制。

## 一、通用 Web 桌面（React 系，与我们同栈）

| 项目 | 星数 | 许可证 | 栈 | 可吸收机制 |
|---|---|---|---|---|
| **daedalOS**（DustinBrett）| 13.0K | **MIT（已复核）** | React + TS | **最完整参照**：react-rnd 窗口管理 + Framer Motion 开关动画、窗口状态（尺寸/位置/最大化）跨会话持久化、BrowserFS(IndexedDB) 文件系统、Monaco 编辑器、任务栏+开始菜单+预览、Web Worker 时钟 |
| **win11React**（blueedgetechno）| ~9.7K | CC0（可抄） | React 17 + Redux（2024-08 已归档） | 开始菜单/搜索/Widgets、窗口吸附、启动屏+锁屏、内置应用集（文件资源管理器/设置/VSCode 等）、多语言 |
| **Puter**（HeyPuter）| ~40K | AGPL-3.0（**只学机制**） | vanilla JS + jQuery（性能取向） | 任务栏/文件管理器/启动器/应用市场/第三方应用生态——**平台化形态参照**（商店路线 §四·C 对齐） |

> 在线看：daedalOS → https://dustinbrett.com；win11React → https://win11.blueedge.me；Puter → https://puter.com

## 二、macOS 风格（用户风格诉求）

| 项目 | 星数 | 许可证 | 栈 | 可吸收机制 |
|---|---|---|---|---|
| **Macos-Simulaing-System-GUI**（dawidolko）| 中小 | MIT | React + TS + Vite + UnoCSS + Zustand——**与我们栈几乎同构** | Big Sur→Sonoma 外观：可拖拽窗口、Launchpad、动态 Dock、明暗主题跟随系统 |
| macOS Web（PuruVJ，现迁 Svelte）| 中 | 开源 | Preact/Svelte + Vite | Monterey 桌面、Dock、菜单栏 |
| ryOS（thesimsguy）| 小 | 开源 | React + TS + Vite + Tailwind + shadcn + Framer Motion | 经典 macOS：Finder/TextEdit/Terminal 多内置应用、本地持久化、移动端响应式 |

> 在线看：Macos-Simulaing-System-GUI → https://macos.dawidolko.pl；macOS Web → https://macos.now.sh

## 三、NAS 管理面（用户补充——"网页里的桌面"成熟商用形态；用户 8-15 复评：DSM 出局"最丑"、飞牛/绿联风格入选）

| 项目 | 形态 | 可吸收机制 |
|---|---|---|
| **Synology DSM** | ~~私有；DSM 3.0(2010) 起窗口化多任务 Web UI；原 Ext JS → DSM 7 现代化~~ | ~~"窗口宿主 + 应用包注册(SPCI) + 包中心"三件套~~ **用户判定风格出局**；机制层面包注册/包中心仍有参照价值 |
| **飞牛 fnOS** | 免费但**未开源**（核实：官方对"是否开源"仅模板化回应"遵循开源协议要求"，无源码无时间表；GitHub 上全是第三方工具/适配包，如 ophub/fnnas 非官方源码；且有 GPL 合规争议帖） | **UI 视觉参照**（用户认可）：现代简洁卡片化；**第三方应用网关机制有真参照价值**——官方开放平台 developer.fnnas.com，Web 应用经统一网关 `/app/<id>` 接入（ui.gateway.json/ui.cgi.json 规范）≈ 我们"应用插件前端包注册进 DE"的同构先例。**红线：无开源代码可抄，只作视觉与机制参照** |
| **绿联 UGOS Pro** | 私有（自家 NAS 硬件），基于 Debian 12 | **UI 视觉参照**（用户认可）：评测画像="精简高级/生活化设计"——卡片式首页（相册墙/影视组件可拖动排序）、全面重绘的精细应用图标、独立任务中心（上下行进度可视化+断点续传）≈ 我们的活任务清单投影；海报墙级视觉仪式感 |
| **CasaOS**（IceWhaleTech）| 开源（Go + Vue）| **manifest 驱动商店**：Git 仓库 apps.json → docker-compose 解析装机，支持第三方商店、社区 PR 贡献——我们插件商店路线（§四·C）的直接参照 |

## 三·五、风格结论（用户拍板路径）

用户逛完 A/B/C 组候选后的原话："都有点丑，群晖最丑；飞牛、绿联我记得不错。"——**风格基调 = 现代国产消费级 NAS 管理界面**：
卡片化 + 扁平 + 生活化配色 + 精细图标 + 适度玻璃拟态，拒绝复古 Windows/macOS 克隆感。
机制参照仍从 daedalOS（react-rnd 窗口）/ fnOS（应用网关注册）/ CasaOS（manifest 商店）取。

## 四、吸收清单（→ 桌面壳设计）

1. **窗口管理**：react-rnd（MIT）现成依赖可直用——拖拽/缩放/层叠/最小化成本远低于自研；daedalOS 实证多年
2. **应用注册器（AppRegistry）**：id/名称/图标/组件/窗口默认尺寸——正是架构 §四·B DE 契约"应用注册器"的落地件，也是"前端插件化"的装配点
3. **开始菜单 = 注册器投影、任务栏 = 运行状态投影**——与"UI 全是投影、事件日志唯一事实源"纪律同构
4. **Dock/Launchpad**（macOS 风）：Dock 放常用应用 + 运行指示点；Launchpad 可选（二阶段）
5. **主题**：UnoCSS 设计令牌/CSS 变量明暗主题——我们已有 theme 配置（light/dark），接现有 i18n/主题体系
6. **商店模式**（远期）：CasaOS manifest 驱动 Git 商店 ↔ 架构 §四·C 商店路线，无需新设计
7. **锁屏/启动屏**：win11React 有先例；BoenMind 可做成"开机进桌面"仪式感（低成本高感知），是否要留拍板

## 五、对桌面壳形态的建议（调研后修订）

- 起步：**桌面 + 左下开始菜单 + 底部任务栏（时钟右置）**；应用**居中窗口**打开（react-rnd 成本可控，比全屏更有"桌面感"，比多窗口层叠省掉 z-order/最小化状态机）→ 窗口层叠/最小化二阶段
- 聊天应用 = 现有界面整体保留（用户拍板）；编程应用 = 占位"建设中"（桌面先上线，用户拍板）；设置/插件/管家状态页 = 注册为应用
- 落地件 = 前端现有 React 项目加一层 DesktopShell（不重写）：路由由 AppRegistry 驱动，现有页面组件包装为应用视图

## 六、模拟 macOS 系对比（2026-08-15 第二轮，GitHub API 实拉数据）

用户结论：模拟 macOS 系最成熟；Umbrel 也算 macOS 风。API 实测（stars/pushed/license）：

| 项目 | 星 | 许可证 | 语言 | 最近推送 | 活跃度 | 轻量化 | 扩展性 | 结构 |
|---|---|---|---|---|---|---|---|---|
| **PuruVJ/macos-web** | 2.6K | MIT | Svelte 5 | 2026-07-05 | ★★★★★（2021 起 5 年） | ★★★★★ 运行时依赖仅 5 个（neodrag/date-fns/popmotion/icons/fontsource） | ★★★★☆ **configs/apps + configs/menu 注册制**（应用=组件+菜单配置，编译期） | ★★★★☆ Desktop/TopBar/Dock/SystemUI（DE 宿主）+ apps + state(stores) + actions + configs 分层清晰 |
| dawidolko/Macos-Simulaing-System-GUI | 8 | MIT | JS(React18) | 2026-02-15 | ★★☆ 个人 playground | ★★★ 依赖重（framer/milkdown/katex/webcam 演示应用多） | ★★★ 应用硬编码 | ★★★ 个人项目；**栈与我们同款（React+TS+Vite+Zustand+react-rnd）** |
| getumbrel/umbrel | 11.7K | PolyForm 非商业 | TS | 2026-07-10 | ★★★★★ | ★★☆ 完整 OS 栈 monorepo（os+ui+umbreld+containers） | ★★★★★ manifest 应用商店（与 §四·C 商店路线同构，远期参照） | ★★★★☆ 工程化但为硬件 OS 服务，非纯前端样板 |
| wolfgunblood/macos | 0 | 无 | TS | 2024-07 | 弃 | — | — | 出局 |
| giant-sur | 123 | 无 | TS | 2021-08 | 弃 | — | — | 出局 |
| macOS-react(gianlucajahn) | 162 | MIT | TS | 2024-02 | 弃 | — | — | 出局 |
| ryOS(thesimsguy) | 仓库名对不上（API 搜索无果） | — | — | — | — | — | — | 出局 |

**结论**：结构样板 = PuruVJ/macos-web（MIT 可学，注册制结构与我们 AppRegistry 设计同构——注意它是 Svelte，照结构不照代码）；视觉 = Cosmos 渐变 + macOS 窗口质感；窗口实现 = react-rnd（与 dawidolko 同款选型）；远期商店机制 = Umbrel/CasaOS/1Panel manifest 路线（§四·C）。
