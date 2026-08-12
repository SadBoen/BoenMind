//! 插件权限询问桥：实现上游 `ExtensionUiHandler`，把扩展的能力询问
//! （capability prompt：exec / env 等）转发给前端聊天界面（SSE 事件），
//! 并等待用户决策——允许 / 拒绝。
//!
//! 无响应或超时 → fail-closed 拒绝（与上游 SDK 默认行为一致）。
//!
//! 决策记忆：上游会把任何决策（含"允许"）持久化到
//! `~/.boenmind/pi/extension-permissions.json`，跨会话生效，插件版本变化后
//! 重新询问。这是上游既有语义（问一次记一次，少打扰用户），本桥不再维护
//! 自有的白名单表——上游缓存即权威存储。

use std::time::Duration;

use async_trait::async_trait;
use pi::extension_dispatcher::ExtensionUiHandler;
use pi::extensions::{ExtensionUiRequest, ExtensionUiResponse};

use crate::PermissionDecision;
use crate::chat::send_permission_request;

/// 询问等待上限：用户无响应时按拒绝处理（fail-closed）。
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(60);

/// 每个会话一个桥（闭包捕获 session_id 与共享状态），随 agent 会话创建注入。
pub struct PermissionBridge {
    pub state: crate::AppState,
    pub session_id: String,
}

#[async_trait]
impl ExtensionUiHandler for PermissionBridge {
    async fn request_ui(
        &self,
        request: ExtensionUiRequest,
    ) -> Result<Option<ExtensionUiResponse>, pi::error::Error> {
        // 仅处理能力询问（confirm）；其它 UI 方法（select/prompt 等）暂不询问，返回取消
        if request.method != "confirm" {
            return Ok(None);
        }
        let capability = request
            .payload
            .get("capability")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let extension_id = request
            .extension_id
            .clone()
            .or_else(|| {
                request
                    .payload
                    .get("extension_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "unknown".to_string());

        // 1. 注册 pending（request id → 决策通道）
        let (tx, rx) = tokio::sync::oneshot::channel::<PermissionDecision>();
        self.state
            .permission_pending
            .lock()
            .await
            .insert(request.id.clone(), tx);

        // 2. 推给前端弹窗（事件经会话活跃 prompt 的 SSE 通道发出）
        let message = request
            .payload
            .get("message")
            .and_then(|v| v.as_str())
            .or_else(|| request.payload.get("title").and_then(|v| v.as_str()))
            .unwrap_or(&capability)
            .to_string();
        send_permission_request(
            &self.state,
            &self.session_id,
            &request.id,
            &extension_id,
            &capability,
            &message,
        )
        .await;

        // 3. 等待用户决策（超时 → None → 上层 fail-closed 拒绝）
        let decision = tokio::time::timeout(PERMISSION_TIMEOUT, rx)
            .await
            .ok()
            .and_then(|r| r.ok());

        // 无论决策如何，先移除挂起条目
        self.state
            .permission_pending
            .lock()
            .await
            .remove(&request.id);

        match decision {
            // 决策记忆交给上游（写 extension-permissions.json），本桥只管转发
            Some(decision) => Ok(Some(ExtensionUiResponse {
                id: request.id,
                value: Some(serde_json::json!(decision.allow)),
                cancelled: !decision.allow,
            })),
            // 超时/通道关闭 → 取消（fail-closed）
            None => Ok(Some(ExtensionUiResponse {
                id: request.id,
                value: None,
                cancelled: true,
            })),
        }
    }
}
