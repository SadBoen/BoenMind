//! 评审修复回归(2026-09-10):异步执行器任务恐慌必须回账为 operation 失败。
//! 此前 spawn 后只靠任务自身回发 ProviderCall,任务 panic 即静默死亡——
//! operation 永停 running,回合等待环在限制全零(不限时)下永久挂死。

use bm_contract::capability::CapabilityManifest;
use bm_contract::ids::{BmId, IdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::{CapabilityCallParams, GetOperationParams};
use bm_core::broker::provider_fn;
use bm_core::ports::{AsyncCallError, AsyncCapabilityExecutor};
use bm_testkit::rig;
use serde_json::json;
use std::sync::Arc;

/// call 一进来就 panic 的执行器(等价 mcp.rs 边界锁中毒/不变量 assert 炸裂)。
struct PanickyExecutor;

#[async_trait::async_trait]
impl AsyncCapabilityExecutor for PanickyExecutor {
    async fn call(
        &self,
        _operation_id: &str,
        _capability: &str,
        _args: serde_json::Value,
        _deadline: std::time::Duration,
    ) -> Result<serde_json::Value, AsyncCallError> {
        panic!("故意恐慌:边界执行炸了");
    }
}

#[tokio::test]
async fn executor_panic_settles_operation_failed() {
    // provider 以 .async 结尾 → RuntimeHandle::start 标记为异步能力
    let manifest: CapabilityManifest = serde_json::from_value(json!({
        "capability": "panic.boom", "provider": "test.async", "version": "0.1.0",
        "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": "read-only", "idempotent": true, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    }))
    .unwrap();

    let rig = rig(
        vec![],
        true,
        vec![(manifest, provider_fn(|_| Err("异步能力不走同步桩".into())))],
        Some(Arc::new(PanickyExecutor)),
    )
    .await;

    let out = rig
        .handle
        .capability_call(
            rig.ids.next_id("req"),
            CapabilityCallParams {
                capability: "panic.boom".into(),
                args: json!({}),
                idempotency_key: None,
                deadline_ms: Some(1000),
            },
        )
        .await
        .expect("read-only 直通受理应成功(异步派发回 running 收据)");
    let op_id = BmId::parse(out["operation_id"].as_str().unwrap()).unwrap();

    // 恐慌后必须在有限时间落终态(修复前:永停 running,此处 5s 必超时)
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let receipt = loop {
        let r = rig
            .handle
            .operations_get(GetOperationParams {
                operation_id: op_id.clone(),
            })
            .await
            .expect("收据可查");
        if r.state.is_terminal() {
            break r;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "恐慌后 operation 未落终态——回账缺失,等待环将永挂"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };

    assert_eq!(receipt.state, OperationState::Failed);
    let err = receipt.error.as_ref().expect("失败收据必有错误");
    assert!(
        err.message.contains("恐慌") && err.message.contains("故意恐慌"),
        "收据应承载恐慌死因: {}",
        err.message
    );
}
