# 工作交接：UI 微调 + 答复复制/分叉（2026-08-16）

> 2026-08-16 用户开题三条 UI 意见，本轮已全部落地（commit 见后）；
> 本交接记录改动点 + 遗留项，供后续轮次续接。

## 已落地改动

### 1. 宽度调整（黄金比例之前的小步）
- 会话列表（ChatPane 两处 `w-64`）：**+1/5 → `w-[19.2rem]`**（307px）
- 设置页左侧菜单（app-registry SettingsAppView `w-52`）：**+1/4 → `w-[16.25rem]`**（260px）
- 改动文件：`frontend/src/components/chat/ChatPane.tsx`、`frontend/src/lib/app-registry.tsx`

### 2. 专家设置弹窗宽度 = 页宽 × 0.618（黄金比例）
- `ui/dialog.tsx` DialogContent 新增 `size` prop：`sm`（默认 384px）/ `md`（29.6rem ≈ 768×0.618 = 474px）
- `ExpertsSettings` 弹窗改 `size="md"`；其他弹窗不变
- 用户定调"多用黄金比例"——后续大表单弹窗优先 `md`

### 3. 答复末尾复制 + 分叉
- **后端**：`POST /api/sessions/{id}/fork`（body `{at_message}`）→ db.rs `fork_session`：
  新会话（标题"「源」分叉"）+ 复制历史（messages 到 at_message 含，tool_calls 一并复制，
  历史渲染完整）；不动内核分支模型
- **前端**：MessageItem 助手答复末尾悬停显示操作栏（复制/分叉）；
  app-store `forkFromMessage(messageId)` → fork → loadSessions → selectSession 切到新会话；
  i18n `chat.message.*`（zh/en/ja/ko）
- 复制：clipboard API + execCommand 兜底（IAB/非安全上下文）

## 遗留项（待后续轮）

1. **角色提示词撰写**（最优先，architect 正文为空）——见
   `docs/archive/HANDOFF_EXPERT_ROLE_PROMPTS.md`（上一轮交接，未动）
2. **记忆管理插件**：桶的浏览/迁移/清理（usage.json 已有删除记录）
3. **内核级 fork 事件**：本轮的"分叉"是**会话级**（复制历史到新会话）；
   架构三缺口之一的内核 fork 事件/分支 UI 仍未做——会话级分叉已验证交互价值，
   内核分支模型落地时可平滑升级（分叉即"新会话从消息 X 继承"语义）
4. **专家模型默认接线**：已接（请求/会话级未指定时用专家 provider::model），
   但前端聊天页模型选择器总有值，实际触发少——per-app 绑定专家后是否要
   同步聊天页默认模型显示，待用户定夺
5. **设置中心其他弹窗**：如需黄金比例可统一 `size="md"`（专家表单已用）

## 验证方式

- 宽度/弹窗：浏览器截图目检（会话列表 307px、侧栏 260px、专家弹窗 474px）
- 分叉：会话内发消息 → 悬停答复 → 点分叉 → 新会话出现且历史完整（含工具块）
- 质量门：cargo test + clippy + tsc 全绿后推送
