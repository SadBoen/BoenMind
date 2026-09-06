//! MCP 装配/热重载的响应形状回归(characterization):批8 前置安全网。
use bm_contract::capability::CapabilityManifest;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

/// 借助现有 testkit rig 形态启动 server 并直调 /admin/mcp/reload。
/// 用临时 mcp.json:0 个 server(全量卸载通道)+ 1 个坏 server(失败通道)。
#[tokio::test]
async fn mcp_reload_response_shape_is_stable() {
    let mut src = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    src.pop(); // crates
    src.pop(); // runtime
    // 有 --mcp-config 的现有 rig 不易构造;直接起真 server 太重——
    // 此处走 http_e2e 的 rig 形态:构造 testkit rig + admin 路由可达。
    // 简化:校验 supervisor 层的纯函数行为(批8 后 handler 是薄壳)。
    // —— 保留 HTTP 级测试为批8 后续任务,本测试锁定配置 diff 语义。
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = dir.path().join("mcp.json");
    // 0 server + 1 个 command 缺失的坏条目(名字有值,spawn 必败)
    fs::write(
        &cfg,
        r#"[{"name":"ghost","command":"Z:/definitely/absent.exe","args":[],"transport":"stdio"}]"#,
    )
    .expect("写 mcp.json");

    let secrets: Arc<dyn bm_core::ports::SecretStore> = Arc::new(MemSecretStore);
    let hub = bm_providers::mcp::McpHub::new();
    let calls: std::sync::Mutex<(usize, usize)> = std::sync::Mutex::new((0, 0));
    let registrar = TestRegistrar { calls: &calls };
    let outcome = bm_providers::mcp::supervisor::sync_from_config(
        &hub,
        &cfg,
        secrets,
        Vec::new(),
        &registrar,
    )
    .await;
    // 现有实现:loaded_names 空 → 无卸载;ghost spawn 失败 → failed 通道
    assert!(outcome.registered.is_empty());
    assert!(outcome.updated.is_empty());
    assert!(outcome.uninstalled.is_empty());
    assert_eq!(outcome.failed.len(), 1, "{:?}", outcome.failed);
    assert_eq!(outcome.failed[0]["name"], "ghost");
    assert_eq!(calls.lock().expect("锁").0, 0, "无成功接入 = 无注册调用");
    assert_eq!(calls.lock().expect("锁").1, 0, "loaded 为空 = 无注销调用");
}

struct TestRegistrar<'a> {
    calls: &'a std::sync::Mutex<(usize, usize)>,
}

impl bm_providers::mcp::supervisor::CapabilityRegistrar for TestRegistrar<'_> {
    fn register(
        &self,
        _entries: Vec<(
            CapabilityManifest,
            Arc<dyn bm_core::registry::CapabilityProvider>,
        )>,
    ) -> Result<(), String> {
        self.calls.lock().expect("锁").0 += 1;
        Ok(())
    }
    fn unregister(&self, _names: Vec<String>) -> Result<(), String> {
        self.calls.lock().expect("锁").1 += 1;
        Ok(())
    }
}

struct MemSecretStore;

impl bm_core::ports::SecretStore for MemSecretStore {
    fn get(&self, _r: &str) -> Result<String, bm_core::ports::SecretError> {
        Err(bm_core::ports::SecretError::NotFound("test".into()))
    }
    fn put(&self, _r: &str, _v: &str) -> Result<(), bm_core::ports::SecretError> {
        Ok(())
    }
    fn delete(&self, _r: &str) -> Result<(), bm_core::ports::SecretError> {
        Ok(())
    }
    fn expose_for_scan(&self) -> Vec<String> {
        Vec::new()
    }
}
