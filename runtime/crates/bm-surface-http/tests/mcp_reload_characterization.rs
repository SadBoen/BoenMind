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
        &bm_core::limits::LimitsCell::with_default(),
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

/// issue #6 卸载下线通道端到端(此前 characterization 仅覆盖 0-server 与
/// spawn 失败通道):已装载 server 从 mcp.json 摘除后,sync 必须
/// ① hub 断连(路由摘除 + 子进程 shutdown/exit 通知)② 能力注销回传
/// registrar ③ uninstalled 通道上报。任何一环缺失 = 「卸载未立即下线」。
#[tokio::test]
async fn sync_uninstalls_removed_server_offline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = dir.path().join("mcp.json");
    // 目标态:alpha 已被用户卸载(配置里不再存在)
    fs::write(&cfg, "[]").expect("写空 mcp.json");

    let secrets: Arc<dyn bm_core::ports::SecretStore> = Arc::new(MemSecretStore);
    let hub = bm_providers::mcp::McpHub::new();
    // 预置「已装载」形态:alpha 经进程内 MCP 伪实现真实握手入 hub,
    // 产生合规工具路由 mcp.alpha.echo(与 stdio 装载同一条 connect 路径)
    let tools = vec![bm_providers::mcp::McpToolDef {
        name: "echo".into(),
        description: None,
        input_schema: serde_json::json!({"type": "object", "properties": {}}),
        annotations: serde_json::json!({}),
    }];
    hub.connect("alpha", bm_testkit::InProcMcpServer::new(tools), 5_000)
        .await
        .expect("预置装载 alpha");

    let unregistered: std::sync::Mutex<Vec<Vec<String>>> = std::sync::Mutex::new(Vec::new());
    let registrar = RecordingRegistrar {
        unregistered: &unregistered,
    };
    let outcome = bm_providers::mcp::supervisor::sync_from_config(
        &hub,
        &cfg,
        secrets,
        vec!["alpha".into()],
        &registrar,
        &bm_core::limits::LimitsCell::with_default(),
    )
    .await;

    // ① uninstalled 通道上报;其余通道必须全空
    assert_eq!(
        outcome.uninstalled,
        vec!["alpha".to_string()],
        "{:?}",
        outcome
    );
    assert!(outcome.registered.is_empty());
    assert!(outcome.updated.is_empty());
    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    // ② 能力注销以 FQ 名回传 registrar(核对面 Registry 据此摘除)
    assert_eq!(
        unregistered.lock().expect("锁").as_slice(),
        [vec!["mcp.alpha.echo".to_string()]]
    );
    // ③ hub 路由已摘:二次断连零残留(子进程通道同样关闭)
    assert!(hub.disconnect_server("alpha").await.is_empty());
}

/// 捕获注销调用名的 registrar(区别于 TestRegistrar 的纯计数)。
struct RecordingRegistrar<'a> {
    unregistered: &'a std::sync::Mutex<Vec<Vec<String>>>,
}

#[async_trait::async_trait]
impl bm_providers::mcp::supervisor::CapabilityRegistrar for RecordingRegistrar<'_> {
    async fn register(
        &self,
        _entries: Vec<(
            CapabilityManifest,
            Arc<dyn bm_core::registry::CapabilityProvider>,
        )>,
    ) -> Result<(), String> {
        Ok(())
    }
    async fn unregister(&self, names: Vec<String>) -> Result<(), String> {
        self.unregistered.lock().expect("锁").push(names);
        Ok(())
    }
}

struct TestRegistrar<'a> {
    calls: &'a std::sync::Mutex<(usize, usize)>,
}

#[async_trait::async_trait]
impl bm_providers::mcp::supervisor::CapabilityRegistrar for TestRegistrar<'_> {
    async fn register(
        &self,
        _entries: Vec<(
            CapabilityManifest,
            Arc<dyn bm_core::registry::CapabilityProvider>,
        )>,
    ) -> Result<(), String> {
        self.calls.lock().expect("锁").0 += 1;
        Ok(())
    }
    async fn unregister(&self, _names: Vec<String>) -> Result<(), String> {
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
