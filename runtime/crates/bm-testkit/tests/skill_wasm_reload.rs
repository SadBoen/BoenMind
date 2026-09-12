//! Skill wasm 执行面端到端守护(ADR-0033)。
//!
//! 修复前:`skill.*` 能力从未进异步分道(`handle.rs` 只认 `mcp.`/`.async`,
//! `handlers.rs` 只认 `mcp.`)——ADR-0016 第二步的 wasm 脚本执行面在生产不可
//! 达,调用落到同步占位 provider 直接报「skill 能力仅限异步路径」;且无卸载/
//! 热重载路径。本测试锁死:①skill 能力走异步分道真执行;②热重载(摘旧+重注册)
//! 即时生效;③代际沿 ADR-0032 墓碑机制不回退。

use bm_contract::capability::CapabilityManifest;
use bm_contract::ids::{BmId, IdGen};
use bm_contract::skill::SkillDefinition;
use bm_contract::states::OperationState;
use bm_contract::wire::{CapabilityCallParams, GetOperationParams};
use bm_core::limits::LimitsCell;
use bm_core::ports::{AsyncCallError, AsyncCapabilityExecutor};
use bm_providers::fs_tools::FsExecutor;
use bm_providers::jobs::JobTable;
use bm_providers::skill_wasm::SkillScriptManager;
use bm_providers::system_exec::{ExecExecutor, SplitExecutor};
use bm_testkit::rig;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// 最小 WASI 命令式模块(WAT):stdout 写固定 JSON,退出 0——与 skill_wasm
/// 单测同款;此处经 `wat` crate 转 wasm 落盘,走生产 `register_skill` 的
/// `Module::from_binary` 路径(非测试内的 `Module::new`)。
const ECHO_WAT: &str = r#"(module
    (import "wasi_snapshot_preview1" "fd_write"
        (func $fd_write (param i32 i32 i32 i32) (result i32)))
    (import "wasi_snapshot_preview1" "proc_exit"
        (func $proc_exit (param i32)))
    (memory (export "memory") 1)
    (data (i32.const 100) "{\"ok\":true,\"src\":\"skill-wasm\"}")
    (func $_start (local $iovec i32)
        (i32.store (i32.const 0) (i32.const 100))
        (i32.store (i32.const 4) (i32.const 30))
        (drop (call $fd_write
            (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))
        (call $proc_exit (i32.const 0))
    )
    (export "_start" (func $_start))
)"#;

const SKILL_ID: &str = "skill_demo";
const CAP: &str = "skill.skill_demo.echo";

/// 测试无回落执行体:非 skill/fs/exec 能力一律报错(本测试只走 skill 分道)。
struct NoFallback;

#[async_trait::async_trait]
impl AsyncCapabilityExecutor for NoFallback {
    async fn call(
        &self,
        _operation_id: &str,
        capability: &str,
        _args: Value,
        _deadline: Duration,
    ) -> Result<Value, AsyncCallError> {
        Err(AsyncCallError::Transport(format!(
            "测试无回落执行体: {capability}"
        )))
    }
}

fn skill_def() -> SkillDefinition {
    serde_json::from_value(json!({
        "skill_id": SKILL_ID,
        "name": "回声",
        "instruction": "x",
        "scripts": [{
            "name": "echo",
            "path": "echo.wasm",
            "effect": "read-only",
            "input_schema": {"type": "object"},
            "output_schema": {"type": "object"}
        }]
    }))
    .expect("技能定义合法")
}

/// 一条待注册能力对(manifest + 同步占位 provider)。
type SkillEntries = Vec<(
    CapabilityManifest,
    Arc<dyn bm_core::registry::CapabilityProvider>,
)>;

/// 落盘 echo.wasm 并建管理器;返回 (manager, 待注册能力对)。
fn load_skill(data_dir: &std::path::Path) -> (Arc<SkillScriptManager>, SkillEntries) {
    let root = data_dir.join("skills").join(SKILL_ID);
    std::fs::create_dir_all(&root).expect("建技能目录");
    let wasm = wat::parse_str(ECHO_WAT).expect("WAT → wasm");
    std::fs::write(root.join("echo.wasm"), wasm).expect("写 wasm");
    let manager = Arc::new(SkillScriptManager::new().expect("wasmtime 引擎"));
    let manifests = manager
        .register_skill(SKILL_ID, &skill_def(), &root)
        .expect("注册技能脚本");
    let entries = SkillScriptManager::capability_entries(manifests);
    (manager, entries)
}

/// 组装 SplitExecutor(skill 分道走本管理器,其余走 NoFallback)。
fn split_executor(
    data_dir: &std::path::Path,
    manager: Arc<SkillScriptManager>,
) -> Arc<dyn AsyncCapabilityExecutor> {
    let limits = LimitsCell::with_default();
    let jobs = JobTable::new(data_dir, limits.clone());
    let exec = Arc::new(ExecExecutor::new(
        limits.clone(),
        jobs,
        data_dir.to_path_buf(),
        data_dir.to_path_buf(),
    ));
    let fs = FsExecutor::with_limits(data_dir.to_path_buf(), data_dir.to_path_buf(), limits);
    Arc::new(SplitExecutor {
        exec,
        fs,
        skills: Some(manager),
        fallback: Arc::new(NoFallback),
    })
}

async fn await_terminal(handle: &bm_core::runtime::RuntimeHandle, op: BmId) -> OperationState {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let r = handle
            .operations_get(GetOperationParams {
                operation_id: op.clone(),
            })
            .await
            .expect("收据可查");
        if r.state.is_terminal() {
            return r.state;
        }
        assert!(std::time::Instant::now() < deadline, "operation 未落终态");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn capability_names(handle: &bm_core::runtime::RuntimeHandle) -> Vec<String> {
    handle
        .capability_list(bm_contract::wire::CapabilityListParams { provider: None })
        .await
        .expect("capability.list")
        .capabilities
        .iter()
        .filter_map(|c| c["capability"].as_str().map(str::to_string))
        .collect()
}

async fn epoch_of(handle: &bm_core::runtime::RuntimeHandle, cap: &str) -> Option<u64> {
    handle
        .capability_list(bm_contract::wire::CapabilityListParams { provider: None })
        .await
        .expect("capability.list")
        .capabilities
        .iter()
        .find(|c| c["capability"].as_str() == Some(cap))
        .and_then(|c| c["binding_epoch"].as_u64())
}

#[tokio::test]
async fn skill_script_executes_async_and_hot_reloads() {
    let dir = tempfile::tempdir().expect("临时目录");
    let data = dir.path();
    let (manager, entries) = load_skill(data);
    let exec = split_executor(data, manager.clone());

    let rig = rig(vec![], true, entries, Some(exec)).await;
    let handle = &rig.handle;

    // ① 端到端:skill 能力在发现面且走异步分道真执行(修复前落同步占位报错)
    assert!(
        capability_names(handle).await.contains(&CAP.to_string()),
        "skill 能力应在发现面"
    );
    let out = handle
        .capability_call(
            rig.ids.next_id("req"),
            CapabilityCallParams {
                capability: CAP.into(),
                args: json!({"x": 1}),
                idempotency_key: None,
                deadline_ms: Some(2000),
            },
        )
        .await
        .expect("skill 调用受理");
    let op = BmId::parse(out["operation_id"].as_str().unwrap()).unwrap();
    let state = await_terminal(handle, op.clone()).await;
    assert_eq!(state, OperationState::Succeeded, "skill 脚本应执行成功");
    let result = handle
        .operation_result(op)
        .await
        .expect("结果可查")
        .expect("成功操作必有结果");
    assert_eq!(
        result,
        json!({"ok": true, "src": "skill-wasm"}),
        "wasm stdout JSON 应作为结果回传"
    );

    // ② 热重载:摘旧(编译缓存 + 核心注册表墓碑)→ 重编译注册
    let removed_local = manager.unregister_skill(SKILL_ID);
    assert_eq!(removed_local, vec![CAP.to_string()], "管理器摘除脚本条目");
    let removed_core = handle
        .capabilities_unregister(removed_local)
        .await
        .expect("核心注销");
    assert_eq!(removed_core, vec![CAP.to_string()]);
    assert!(
        !capability_names(handle).await.contains(&CAP.to_string()),
        "注销后应从发现面摘除"
    );

    let manifests = manager
        .register_skill(SKILL_ID, &skill_def(), &data.join("skills").join(SKILL_ID))
        .expect("重编译");
    handle
        .capabilities_register(SkillScriptManager::capability_entries(manifests))
        .await
        .expect("重注册");
    assert!(
        capability_names(handle).await.contains(&CAP.to_string()),
        "重注册后应回发现面"
    );

    // ③ 代际沿 ADR-0032 墓碑机制续命:重注册 = 旧 epoch + 1
    assert_eq!(
        epoch_of(handle, CAP).await,
        Some(2),
        "热重载代际不回退(修复前 skill 面无常驻能力,无从谈起)"
    );

    // 重载后仍可执行(执行体共用同一管理器实例)
    let out = handle
        .capability_call(
            rig.ids.next_id("req"),
            CapabilityCallParams {
                capability: CAP.into(),
                args: json!({}),
                idempotency_key: None,
                deadline_ms: Some(2000),
            },
        )
        .await
        .expect("重载后调用受理");
    let op = BmId::parse(out["operation_id"].as_str().unwrap()).unwrap();
    let state = await_terminal(handle, op).await;
    assert_eq!(state, OperationState::Succeeded, "重载后应仍可执行");

    rig.handle.stop("done").await;
}

/// 未装载脚本时管理器摘除是幂等空操作(删除纯知识包技能不报错)。
#[tokio::test]
async fn unregister_missing_skill_is_noop() {
    let manager = SkillScriptManager::new().expect("引擎");
    assert!(manager.unregister_skill("never_loaded").is_empty());
}

// ---- ADR-0041/0042:通用 wasm 插件(注入面 = 声明,**不是** `skill.*` 命名)--------

const PLUGIN_CAP: &str = "demo.echo";

/// 通用插件声明(非 `skill.*` 命名)——证明 wasm 宿主与分道不靠名字前缀。
/// 显式 `approval: not-required`(read-only 直通);声明缺省是 required
/// (安全默认),此处为走通调用面按只读语义声明,与技能脚本口径一致。
fn plugin_decl() -> Value {
    json!([{
        "capability": PLUGIN_CAP,
        "provider": "demo.wasm",
        "version": "0.2.0",
        "wasm": "echo.wasm",
        "effect": "read-only",
        "approval": "not-required",
        "timeout_ms": 5000
    }])
}

/// 从声明文件装载通用插件:落 echo.wasm + plugins.json,返回 (manager, 待注册能力对)。
fn load_plugin(data_dir: &std::path::Path) -> (Arc<SkillScriptManager>, SkillEntries) {
    let cfg = data_dir.join("config");
    std::fs::create_dir_all(&cfg).expect("建 config");
    std::fs::write(
        cfg.join("echo.wasm"),
        wat::parse_str(ECHO_WAT).expect("WAT → wasm"),
    )
    .expect("写 wasm");
    std::fs::write(cfg.join("plugins.json"), plugin_decl().to_string()).expect("写声明");
    let manager = Arc::new(SkillScriptManager::new().expect("wasmtime 引擎"));
    let manifests = manager.load_plugins_file(&cfg.join("plugins.json"));
    let entries = SkillScriptManager::capability_entries(manifests);
    (manager, entries)
}

/// 通用插件全链:非 `skill.*` 命名 → 声明装载 → 走异步分道真执行 → 按来源摘除。
/// 锁死 ADR-0041 的"第二个真实调用方"与 ADR-0042 的"按 origin 摘除"。
#[tokio::test]
async fn generic_wasm_plugin_executes_and_unregisters_by_origin() {
    let dir = tempfile::tempdir().expect("临时目录");
    let data = dir.path();
    let (manager, entries) = load_plugin(data);
    let exec = split_executor(data, manager.clone());

    let rig = rig(vec![], true, entries, Some(exec)).await;
    let handle = &rig.handle;

    // ① 非 skill.* 命名的能力也在发现面,且走异步分道真执行(分道按归属,不按前缀)
    assert!(
        capability_names(handle)
            .await
            .contains(&PLUGIN_CAP.to_string()),
        "通用插件能力应在发现面(证明宿主对命名空间不可知)"
    );
    let out = handle
        .capability_call(
            rig.ids.next_id("req"),
            CapabilityCallParams {
                capability: PLUGIN_CAP.into(),
                args: json!({}),
                idempotency_key: None,
                deadline_ms: Some(2000),
            },
        )
        .await
        .expect("通用插件调用受理");
    let op = BmId::parse(out["operation_id"].as_str().unwrap()).unwrap();
    assert_eq!(
        await_terminal(handle, op.clone()).await,
        OperationState::Succeeded,
        "通用 wasm 插件应执行成功"
    );
    assert_eq!(
        handle.operation_result(op).await.unwrap().unwrap(),
        json!({"ok": true, "src": "skill-wasm"}),
        "wasm stdout JSON 应作为结果回传"
    );

    // ② 按**来源**摘除(origin=Generic):不依赖 provider 名字前缀
    let removed = manager.unregister_all_generic();
    assert_eq!(removed, vec![PLUGIN_CAP.to_string()], "按来源摘除通用插件");
    handle
        .capabilities_unregister(removed)
        .await
        .expect("核心注销");
    assert!(
        !capability_names(handle)
            .await
            .contains(&PLUGIN_CAP.to_string()),
        "摘除后应从发现面消失"
    );
    // 摘除技能(origin=Skill)不受影响:通用摘除不越界
    assert!(!manager.has_capability(PLUGIN_CAP));

    rig.handle.stop("done").await;
}
