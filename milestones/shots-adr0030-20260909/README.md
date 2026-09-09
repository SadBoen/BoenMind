# ADR-0030 审批裁决后台化 · 手测证据(2026-09-09)

环境:隔离端口 127.0.0.1:7598 + 临时 data-dir + 脚本化 mock 网关(127.0.0.1:8771,
奇数次调用返回 system.exec 工具调用,偶数次返回终稿;BOEN_MODEL_STREAM=0)。

证据:
1. **01_ask_approval_card.png**(默认 ask 模式):发"请运行探测命令"→ 审批卡出现
   (执行命令 echo bm-yolo-probe-001 + ✓批准/驳回),回合滞留等待人工(计时 45.7s→70.7s)。
2. **02_yolo_auto_approve_no_card.png**(切"完全访问"后):同一能力调用 **900ms**
   完成,**零审批卡**——服务端在裁决点自动放行;同屏可见首条 70778ms 对照。
   切换动作 = 前端 POST /admin/sessions/{sid}/mode(服务端状态实测回读 yolo)。
3. **审计区分**(events.jsonl 实录):第一次批准 `"source":"user"`(人工),
   第二次 `"source":"mode_auto"`(yolo 自动放行)——人工与机器裁决可区分,
   此前浏览器代批时审计不可区分的缺陷就此修复。
4. 关闭网页语义由集成测试护航(session_mode.rs:服务端自动批准不依赖前端;
   模式持久化经 session.mode.changed 事件物化,重启装载)。

测毕:停进程、删临时目录。
