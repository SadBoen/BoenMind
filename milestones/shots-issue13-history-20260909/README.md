# issue #13 providers 软删除+历史版本 · 手测证据(2026-09-09)

环境:隔离端口 127.0.0.1:7597 + 临时 data-dir;创建 restore-gw(provider)
→ 页面删除(confirm 确认)→ 历史抽屉 → 恢复。

证据:
1. 01_history_drawer.png——「历史」按钮 + 抽屉显示 restore-gw
   (baseUrl · 删除于 2026/9/9 09:49:17 · 含密钥 + 恢复按钮)。
2. 02_restored.png——点恢复后:提示「已恢复到活跃列表」,卡片回到活跃列表
   (密钥已存),抽屉转「暂无删除记录」。
3. 集成测试 t_w2b_provider_soft_delete_history_and_restore:删→历史(打码+墓碑
   +原文留档)→恢复→再恢复 404。
4. 测毕:停进程、删临时目录、关标签页。
