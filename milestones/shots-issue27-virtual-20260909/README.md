# issue #27 长会话渲染性能 · 手测证据(2026-09-09)

环境:隔离端口 127.0.0.1:7595 + 临时 data-dir;脚本灌 81 组问答(162 行
context-log)成超长会话。

证据:
1. 01_long_session_windowed.png——切长会话正常渲染:首屏仅载最近 50 条,
   顶部「加载更早消息」;连点两次后 150 条入窗,底部输入框与流式区不串位。
2. DOM 计数:切换时 .msg=50(初始页<窗口 120 全渲染);分页加载即时入窗
   (loadOlder 成功窗口同步扩,读旧内容不藏);content-visibility 计算值 = auto。
3. 行为语义:状态累计上限 1000(此前无界=缺陷本体);DOM 窗口 120(活跃
   长会话超窗后「展开更早」即时入窗不取数);content-visibility:auto 让
   离屏消息跳过布局/绘制(掉帧主源),contain-intrinsic-size 估高 120px。
4. 方案评估:@tanstack/react-virtual 在 assistant-ui Viewport 内需自管
   动态测高/滚动锚定/流式贴底,回归风险高;改用官方钦定路径
   unstable_useThreadMessageIds + ThreadPrimitive.Unstable_MessageById
   (文档明确支持「virtualized or custom message list」)窗口化 +
   CSS content-visibility,零上下文链破坏。
5. 测毕:停进程、删临时目录、关标签页。
