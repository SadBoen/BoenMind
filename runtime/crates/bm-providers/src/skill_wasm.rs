//! Skill v0.2 第二步(ADR-0016):wasmtime 脚本执行面。
//!
//! 脚本形态 = WASI 命令式 wasm 模块:stdin 进 JSON 入参、stdout 出 JSON
//! 结果、退出码 0 = 成功。与语言无关(Rust/C/TinyGo 皆可编译),执行走
//! wasmtime 沙箱:fuel + 超时双限,WASI 仅挂技能自身目录(零网络)。
//!
//! 权限零新通道:脚本经 [`SkillScriptManager::register_skill`] 合成
//! CapabilityManifest(`skill.<skill_id>.<name>`)注册进 Registry 后,与
//! 内置/MCP 能力完全平权地走 Broker 七步管线(查表/审批/凭证/审计)。
//! 执行体实现 [`AsyncCapabilityExecutor`](bm_core::ports),与 MCP 同分道,
//! wasmtime 编译/执行为 CPU 密集,一律 spawn_blocking 不占核心单写者循环。

use bm_contract::capability::CapabilityManifest;
use bm_contract::skill::SkillDefinition;
use bm_core::ports::AsyncCallError;
use bm_core::ports::AsyncCapabilityExecutor;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use wasmtime::{Engine, Module};

/// 默认脚本超时(ADR-0016:10s 看门狗)。
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
/// fuel 上限(防死循环烧 CPU:约对应数秒纯计算,超时硬顶兜底)。
const FUEL_LIMIT: u64 = 2_000_000_000;

/// 一条已注册脚本:capability 名 → 编译缓存 + 执行参数。
pub struct ScriptEntry {
    pub capability: String,
    pub wasm_path: PathBuf,
    pub module: Module,
    pub timeout_ms: u64,
}

/// 技能脚本管理器:编译缓存 + 注册 + 执行(实现 AsyncCapabilityExecutor)。
pub struct SkillScriptManager {
    engine: Engine,
    entries: Mutex<HashMap<String, Arc<ScriptEntry>>>,
}

impl SkillScriptManager {
    pub fn new() -> Result<Self, String> {
        // fuel 计量必须引擎级开启:set_fuel 才可用(死循环 wasm 的硬保险)
        let mut cfg = wasmtime::Config::new();
        cfg.consume_fuel(true);
        let engine = Engine::new(&cfg).map_err(|e| format!("wasmtime Engine 失败: {e}"))?;
        Ok(Self {
            engine,
            entries: Mutex::new(HashMap::new()),
        })
    }

    /// 注册一个技能的全部脚本:编译 wasm → 缓存 Module → 合成 manifests。
    /// 返回 (manifest, placeholder) 对齐内置能力注册形态(执行体走本管理器)。
    pub fn register_skill(
        &self,
        skill_id: &str,
        def: &SkillDefinition,
        skill_root: &Path,
    ) -> Result<Vec<CapabilityManifest>, String> {
        let scripts = def.scripts.as_ref().ok_or("技能未声明 scripts")?;
        for sc in scripts {
            let wasm_path = skill_root.join(&sc.path);
            let bytes = std::fs::read(&wasm_path)
                .map_err(|e| format!("脚本 {} 读取失败({}): {}", sc.name, sc.path, e))?;
            let module = Module::from_binary(&self.engine, &bytes)
                .map_err(|e| format!("脚本 {} wasm 编译失败: {}", sc.name, e))?;
            let timeout_ms = sc.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
            let capability = format!("skill.{}.{}", skill_id, sc.name);
            self.entries.lock().expect("锁未中毒").insert(
                capability.clone(),
                Arc::new(ScriptEntry {
                    capability,
                    wasm_path,
                    module,
                    timeout_ms,
                }),
            );
        }
        self.manifests_for(skill_id, scripts)
    }

    /// scripts[] → CapabilityManifest 列表(effect 直映 RiskClass;
    /// approval 语义交由 manifest.effect + Broker 统一裁决)。
    fn manifests_for(
        &self,
        skill_id: &str,
        scripts: &[bm_contract::skill::SkillScript],
    ) -> Result<Vec<CapabilityManifest>, String> {
        scripts
            .iter()
            .map(|sc| {
                serde_json::from_value(json!({
                    "capability": format!("skill.{}.{}", skill_id, sc.name),
                    "provider": format!("skill.{}", skill_id),
                    "version": "0.1.0",
                    "input_schema": sc.input_schema,
                    "output_schema": sc.output_schema,
                    "effect": sc.effect,
                    "idempotent": false,
                    "cancellable": true,
                    "timeout_ms": sc.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
                    "approval": if sc.effect == "read-only" { "not-required" } else { "required" },
                    "scopes": [format!("domain:skill.{}", skill_id)],
                }))
                .map_err(|e| format!("脚本 {} manifest 非法: {}", sc.name, e))
            })
            .collect()
    }

    /// 执行:stdin 进 JSON,stdout 收 JSON(退出码 0 = 成功)。
    async fn run(&self, capability: &str, args: &Value) -> Result<Value, AsyncCallError> {
        let entry: Arc<ScriptEntry> = self
            .entries
            .lock()
            .expect("锁未中毒")
            .get(capability)
            .cloned()
            .ok_or_else(|| AsyncCallError::Transport(format!("skill 脚本未注册: {capability}")))?;
        let input = serde_json::to_vec(args)
            .map_err(|e| AsyncCallError::Transport(format!("入参序列化失败: {e}")))?;
        let engine = self.engine.clone();

        // wasmtime 同步执行(CPU 密集)挪出单写者循环;超时由 tokio 硬顶。
        let engine_for_task = engine.clone();
        let entry_for_task = entry.clone();
        let task = tokio::task::spawn_blocking(move || {
            let entry = entry_for_task;
            run_wasi(&engine_for_task, &entry.module, &input, entry.timeout_ms)
        });
        let timeout = std::time::Duration::from_millis(entry.timeout_ms.max(100));
        match tokio::time::timeout(timeout, task).await {
            Err(_) => Err(AsyncCallError::Timeout),
            Ok(Err(join)) => Err(AsyncCallError::Transport(format!("skill 任务失败: {join}"))),
            Ok(Ok(r)) => r,
        }
    }
}

/// WASI 执行单发:内存管道喂 stdin/收 stdout;trap/非零退出收容为错误。
fn run_wasi(
    engine: &Engine,
    module: &Module,
    input: &[u8],
    timeout_ms: u64,
) -> Result<Value, AsyncCallError> {
    use wasmtime_wasi::WasiCtxBuilder;
    use wasmtime_wasi::p1::WasiP1Ctx;
    use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};

    let stdin = MemoryInputPipe::new(input.to_vec());
    let stdout = MemoryOutputPipe::new(1_048_576);
    let stderr = MemoryOutputPipe::new(4_096);

    let wasi: WasiP1Ctx = WasiCtxBuilder::new()
        .stdin(stdin)
        .stdout(stdout.clone())
        .stderr(stderr.clone())
        .build_p1();

    let mut store = wasmtime::Store::new(engine, wasi);
    store
        .set_fuel(FUEL_LIMIT)
        .map_err(|e| AsyncCallError::Transport(format!("fuel 初始化失败: {e}")))?;

    let mut linker: wasmtime::Linker<WasiP1Ctx> = wasmtime::Linker::new(engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |cx| cx)
        .map_err(|e| AsyncCallError::Transport(format!("wasi linker 失败: {e}")))?;

    let instantiation = linker.instantiate(&mut store, module);
    // 编译期已经拿到 module;此处仍可能因实例化失败(imports 不满足)报错
    let instance = match instantiation {
        Ok(i) => i,
        Err(e) => return Err(AsyncCallError::Transport(format!("wasm 实例化失败: {e}"))),
    };
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .map_err(|e| {
            AsyncCallError::Transport(format!("wasm 缺少 _start 导出(WASI 命令式形态): {e}"))
        });

    let start = start?;
    let run_result = start.call(&mut store, ());
    let _ = timeout_ms; // 超时由外层 tokio timeout + fuel 双限;此处保留语义占位

    if let Err(trap) = run_result {
        // WASI proc_exit 以 I32Exit 形式"trap"收场:0 = 正常退出,非 0 = 失败;
        // 其余 trap(含 fuel 耗尽)收容为可读错误,不击穿核心循环。
        if let Some(exit) = trap.downcast_ref::<wasmtime_wasi::I32Exit>() {
            if exit.0 != 0 {
                return Err(AsyncCallError::Transport(format!(
                    "wasm 非零退出: {}",
                    exit.0
                )));
            }
        } else {
            let msg = format!("wasm trap: {trap}");
            return Err(AsyncCallError::Transport(msg));
        }
    }

    let out_bytes = stdout.contents();
    if !out_bytes.is_empty()
        && let Ok(v) = serde_json::from_slice::<Value>(&out_bytes)
    {
        return Ok(v);
    }
    let err_bytes = stderr.contents();
    let err_text = String::from_utf8_lossy(err_bytes.as_ref());
    Err(AsyncCallError::Transport(format!(
        "wasm 未输出合法 JSON(stderr: {})",
        err_text.chars().take(200).collect::<String>()
    )))
}

impl SkillScriptManager {
    /// manifests → 注册对(占位 Provider;真正执行走本管理器异步分道)。
    pub fn capability_entries(
        manifests: Vec<CapabilityManifest>,
    ) -> Vec<(
        CapabilityManifest,
        Arc<dyn bm_core::registry::CapabilityProvider>,
    )> {
        manifests
            .into_iter()
            .map(|m| {
                (
                    m,
                    bm_core::broker::provider_fn(|_| Err("skill 能力仅限异步路径".into())),
                )
            })
            .collect()
    }
}

/// 异步执行器包装:capability = `skill.<skill_id>.<script_name>`。
#[async_trait::async_trait]
impl AsyncCapabilityExecutor for SkillScriptManager {
    async fn call(
        &self,
        _operation_id: &str,
        capability: &str,
        args: Value,
        _deadline: std::time::Duration,
    ) -> Result<Value, AsyncCallError> {
        if !capability.starts_with("skill.") {
            return Err(AsyncCallError::Transport(format!(
                "skill 执行器不认识能力 {capability}"
            )));
        }
        self.run(capability, &args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小 WASI 命令式模块(WAT):stdin 读 JSON,stdout 写固定 JSON,退出 0。
    /// 依赖 fd_write/fd_read/proc_exit 三个 wasi 调用,验证管道契约与沙箱收容。
    const ECHO_WAT: &str = r#"(module
        (import "wasi_snapshot_preview1" "fd_write"
            (func $fd_write (param i32 i32 i32 i32) (result i32)))
        (import "wasi_snapshot_preview1" "proc_exit"
            (func $proc_exit (param i32)))
        (memory (export "memory") 1)
        (data (i32.const 100) "{\"ok\":true,\"src\":\"skill-wasm\"}")
        (func $_start (local $iovec i32)
            ;; iovec@0 = {ptr=100, len=31}
            (i32.store (i32.const 0) (i32.const 100))
            (i32.store (i32.const 4) (i32.const 30))
            (drop (call $fd_write
                (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))
            (call $proc_exit (i32.const 0))
        )
        (export "_start" (func $_start))
    )"#;

    fn manager_with_wat(wat: &str) -> (SkillScriptManager, String) {
        let mgr = SkillScriptManager::new().expect("engine");
        let engine = mgr.engine.clone();
        let module = Module::new(&engine, wat).expect("wat 编译");
        let cap = "skill.demo.convert".to_string();
        mgr.entries.lock().expect("锁").insert(
            cap.clone(),
            Arc::new(ScriptEntry {
                capability: cap.clone(),
                wasm_path: PathBuf::from("demo.wat"),
                module,
                timeout_ms: 5_000,
            }),
        );
        (mgr, cap)
    }

    #[tokio::test]
    async fn skill_script_runs_and_returns_json() {
        let (mgr, cap) = manager_with_wat(ECHO_WAT);
        let out = mgr
            .run(&cap, &serde_json::json!({"x": 1}))
            .await
            .expect("执行成功");
        assert_eq!(out["ok"], serde_json::json!(true));
        assert_eq!(out["src"], serde_json::json!("skill-wasm"));
    }

    #[tokio::test]
    async fn unknown_capability_is_transport_error() {
        let (mgr, _cap) = manager_with_wat(ECHO_WAT);
        let err = mgr
            .run("skill.demo.absent", &serde_json::json!({}))
            .await
            .expect_err("应失败");
        assert!(matches!(err, AsyncCallError::Transport(m) if m.contains("未注册")));
    }

    #[tokio::test]
    async fn fuel_exhaustion_is_contained_not_fatal() {
        // 死循环 wasm:fuel 耗尽 → trap 收容为 Transport 错误(不击穿调用方)
        const SPIN_WAT: &str = r#"(module
            (func $_start (loop br 0))
            (export "_start" (func $_start))
        )"#;
        let (mgr, cap) = manager_with_wat(SPIN_WAT);
        let err = mgr
            .run(&cap, &serde_json::json!({}))
            .await
            .expect_err("应超时/fuel 耗尽");
        assert!(matches!(
            err,
            AsyncCallError::Transport(_) | AsyncCallError::Timeout
        ));
    }

    #[test]
    fn manifests_for_maps_effect_and_capability_name() {
        let mgr = SkillScriptManager::new().expect("engine");
        let def: SkillDefinition = serde_json::from_value(serde_json::json!({
            "skill_id": "skill_units",
            "name": "换算",
            "instruction": "x",
            "scripts": [{
                "name": "convert",
                "path": "scripts/convert.wasm",
                "effect": "external-side-effect",
                "input_schema": {"type": "object"},
                "output_schema": {"type": "object"}
            }]
        }))
        .expect("合法");
        let manifests = mgr
            .manifests_for("skill_units", def.scripts.as_ref().unwrap())
            .expect("manifests");
        assert_eq!(manifests[0].capability, "skill.skill_units.convert");
        assert_eq!(
            manifests[0].approval,
            bm_contract::capability::ApprovalRequirement::Required,
            "副作用脚本必须审批"
        );
    }
}
