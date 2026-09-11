//! binding_epoch 代际连续性守护(2026-09-11 架构评审风险 5 修复的回锁)。
//!
//! 修复前:热注销物理删持久行 + 注册恒 epoch=1 → 每次 MCP 重载(管理面
//! 常规操作)后 (epoch, provider_instance_id) 与任何已签发凭证无法区分,
//! 授权-执行-审计归属链断;registry.rs 头注声称的单调性只在测试里成立。
//! 本测试锁死:注销墓碑化 + 重注册按持久 max+1 续代,跨热重载与重启不回退。

use bm_contract::capability::CapabilityManifest;
use bm_contract::ids::SeqIdGen;
use bm_contract::wire;
use bm_core::broker::provider_fn;
use bm_core::runtime::{RuntimeConfig, RuntimeHandle};
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;

fn manifest(name: &str, provider: &str) -> CapabilityManifest {
    serde_json::from_value(json!({
        "capability": name, "provider": provider, "version": "0.1.0",
        "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
        "effect": "read-only", "idempotent": true, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    }))
    .unwrap()
}

async fn rig(dir: &std::path::Path) -> RuntimeHandle {
    let store: Arc<dyn bm_persist::EventStore> =
        Arc::new(bm_persist::PersistStore::open(dir).expect("打开持久层"));
    RuntimeHandle::start(RuntimeConfig {
        capabilities: vec![(manifest("system.echo", "system.echo"), provider_fn(Ok))],
        version: "0.1.0-epoch".into(),
        data_dir: Some(dir.to_path_buf()),
        store: Some(store),
        connector: Arc::new(MockConnector::new(vec![])),
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(bm_core::clock::SystemClock),
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::with_default(),
        job_board: None,
    })
    .await
}

async fn epoch_of(handle: &RuntimeHandle, capability: &str) -> Option<u64> {
    let list = handle
        .capability_list(wire::CapabilityListParams { provider: None })
        .await
        .expect("capability.list");
    list.capabilities
        .iter()
        .find(|c| c["capability"].as_str() == Some(capability))
        .and_then(|c| c["binding_epoch"].as_u64())
}

#[tokio::test]
async fn binding_epoch_never_regresses_across_reload_and_restart() {
    let dir = tempfile::tempdir().expect("临时目录");
    let handle = rig(dir.path()).await;
    assert_eq!(epoch_of(&handle, "system.echo").await, Some(1), "首装 = 1");

    // 热注册 → 注销(墓碑) → 重注册:第二代必须 > 第一代
    let fake = manifest("mcp.fake.echo", "mcp.fake");
    let reg1 = handle
        .capabilities_register(vec![(fake.clone(), provider_fn(Ok))])
        .await
        .expect("热注册");
    assert_eq!(reg1, vec!["mcp.fake.echo".to_string()]);
    assert_eq!(epoch_of(&handle, "mcp.fake.echo").await, Some(1));

    let removed = handle
        .capabilities_unregister(vec!["mcp.fake.echo".into()])
        .await
        .expect("热注销");
    assert_eq!(removed, vec!["mcp.fake.echo".to_string()]);
    assert_eq!(
        epoch_of(&handle, "mcp.fake.echo").await,
        None,
        "注销即从发现面摘除"
    );

    handle
        .capabilities_register(vec![(fake.clone(), provider_fn(Ok))])
        .await
        .expect("重注册");
    assert_eq!(
        epoch_of(&handle, "mcp.fake.echo").await,
        Some(2),
        "墓碑行驱动代际续命:重注册 = 旧 epoch + 1(修复前归零)"
    );
    handle.stop("restart").await;
    drop(handle);

    // 重启(同 data_dir 新开持久层):启动注册的内置能力代际也要前进,
    // 热能力再注册续接墓碑代际。
    let handle2 = rig(dir.path()).await;
    assert_eq!(
        epoch_of(&handle2, "system.echo").await,
        Some(2),
        "重启装载 = 持久 max + 1(修复前启动落库先覆盖、恢复读回恒 1)"
    );
    handle2
        .capabilities_register(vec![(fake, provider_fn(Ok))])
        .await
        .expect("重启后重注册");
    assert_eq!(
        epoch_of(&handle2, "mcp.fake.echo").await,
        Some(3),
        "跨重启代际不回退"
    );
    handle2.stop("done").await;
}

/// 注册门禁端到端:违冻结合同的 manifest 被批量注册拦下(逐条记错),
/// 合法同批不受影响;全坏才整体报错。
#[tokio::test]
async fn frozen_schema_gate_rejects_bad_manifest_in_hot_register() {
    let dir = tempfile::tempdir().expect("临时目录");
    let handle = rig(dir.path()).await;

    let bad: CapabilityManifest = serde_json::from_value(json!({
        "capability": "mcp.fake.OkButWait", "provider": "mcp.fake", "version": "0.1.0",
        "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
        "effect": "read-only", "idempotent": true, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    }))
    .expect("serde 形状合法(冻结合同 pattern 才是门禁)");
    let good = manifest("mcp.other.ping", "mcp.other");

    let registered = handle
        .capabilities_register(vec![(bad, provider_fn(Ok)), (good, provider_fn(Ok))])
        .await
        .expect("部分失败不拖垮批量");
    assert_eq!(registered, vec!["mcp.other.ping".to_string()]);
    assert_eq!(epoch_of(&handle, "mcp.other.ping").await, Some(1));
    assert_eq!(epoch_of(&handle, "mcp.fake.OkButWait").await, None);

    // 全坏 = 整体报错
    let another_bad: CapabilityManifest = serde_json::from_value(json!({
        "capability": "mcp.fake.空间", "provider": "mcp.fake", "version": "0.1.0",
        "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
        "effect": "read-only", "idempotent": true, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    }))
    .unwrap();
    assert!(
        handle
            .capabilities_register(vec![(another_bad, provider_fn(Ok))])
            .await
            .is_err()
    );
    handle.stop("done").await;
}
