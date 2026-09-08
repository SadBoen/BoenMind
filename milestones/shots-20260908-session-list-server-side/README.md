# 会话列表收归服务端 · 三设备一致性验收截图(2026-09-08)

修复内容:Session 列表此前只存浏览器 localStorage(`bm_sessions`,每设备各记
各账,三设备列表不一致的根因);现收归服务端(SQLite sessions 表增
title/updated_at 两列 + 新端点 GET /admin/sessions),前端启动即拉服务端
权威列表。聊天正文本就在服务端(context-log.jsonl),此次补的是「目录」。

测试实例:本机 127.0.0.1:7533,独立数据目录;设备 = 两个完全隔离的浏览器
上下文(存储互不可见,等价两台电脑)。

| 文件 | 证明 |
|---|---|
| 01_device-a-two-sessions.png | 设备甲建两个会话后的列表(标题=服务端自首条用户消息回填) |
| 03_device-b-fresh-context-same-list.png | 设备乙(全新存储,首次打开)看到**完全一致**的列表——修复前此处必为空 |
| 04_device-a-sees-device-b-session.png | 乙新建会话后,甲仅刷新页面即看见(无需任何本地记忆) |
| 05_delete-synced-across-devices.png | 乙删除会话后,甲刷新同步消失 |
| 06_after-server-restart-list-persisted.png | 服务重启后,第三台全新设备打开:列表自 SQLite 完整恢复 |
| two-devices-log.json | 自动化断言记录(两设备列表一致=true、跨设备可见=true、删除同步=true) |

另经 IAB 真实浏览器(可视化)全程点击验证:发送消息→会话即入列表→
清空 localStorage 模拟新设备→列表仍完整(服务端权威)。

## 三点会话语义批(同日第二批,2026-09-08)

用户裁决:①打开网址/刷新 = 全新临时会话;②未发消息即关页/切走 = 临时会话零留痕;
③消息发出后回合归服务器管,关页/切会话不打断。

| 文件 | 证明 |
|---|---|
| 07_fresh-open-new-temp-session.png | 打开页面即「新对话 就绪」,列表无任何「当前活动」标记(不再恢复上次会话) |
| 08_reply-completed-after-page-close.png | 发消息后同一瞬间真实关页,重开页面点开该会话:往来完整在列(回合服务端照常完成) |

自动化断言:打开即新对话卡=true / 当前活动项=0 / 关页后用户+回复消息均落历史=true。
随批修复:发消息瞬间掉线曾把消息静默吞掉(会话已建、回合未诞生)——派发段
tokio::spawn 任务化后断连免疫,实测「发出即关页」回复照常落历史。
