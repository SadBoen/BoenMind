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

/// fuel 上限(防死循环烧 CPU:约对应数秒纯计算,超时硬顶兜底)。
const FUEL_LIMIT: u64 = 2_000_000_000;

/// 一条已注册脚本:capability 名 → 编译缓存 + 执行参数。
pub struct ScriptEntry {
    pub capability: String,
 /// 提供者标识(ADR-0041 热重载):`skill.<id>` 或插件声明的 provider。
 /// 按它摘除一组能力,取代原先写死的 `skill.` 前缀拼接。
    pub provider: String,
 /// 装载来源(ADR-0042):技能(skills.json)还是通用插件(plugins.json)。
 /// 显式记录来源,避免用 provider 名字前缀(`skill.`)反推——那正是被
 /// 反复批评的"前缀猜代替声明"。
    pub origin: PluginOrigin,
    pub wasm_path: PathBuf,
    pub module: Module,
    pub timeout_ms: u64,
}

/// wasm 能力的装载来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginOrigin {
 /// skills.json 声明的技能脚本。
    Skill,
 /// plugins.json(或 `register_wasm` 直接调用)装载的通用 wasm 插件。
    Generic,
}

/// 归一化的 wasm 能力声明(ADR-0049)。
/// 技能(`skills.json` 的 `scripts[]`)与插件(`plugins.json` 条目)是**两种磁盘
/// 形状**,但**能力语义相同**——
/// (默认值/字段易漂移)。本结构把两者归一,`synthesize` 单一合成 manifest:
/// **装载与清单只剩一条代码路径**,两格式各作薄适配器喂入。
struct WasmDecl {
    capability: String,
    provider: String,
    version: String,
    input_schema: Value,
    output_schema: Value,
    effect: bm_contract::capability::RiskClass,
    idempotent: bool,
    timeout_ms: u64,
    approval: bm_contract::capability::ApprovalRequirement,
    scopes: Vec<String>,
    description: Option<String>,
}

impl WasmDecl {
 /// 合成 `CapabilityManifest`。ADR-0051:与内置/MCP/share 等族共用
 /// [`bm_contract::capability::ManifestSpec`] 单一合成路径(execution_mode
 /// 恒 async;cancellable 恒 true);本结构只承载两格式(技能/插件)归一后的差异。
    fn synthesize(&self) -> Result<CapabilityManifest, String> {
        let mut spec = bm_contract::capability::ManifestSpec::new(
            &self.capability,
            &self.provider,
            self.effect,
        )
        .version(&self.version)
        .input_schema(self.input_schema.clone())
        .output_schema(self.output_schema.clone())
        .idempotent(self.idempotent)
        .cancellable(true)
        .timeout_ms(self.timeout_ms)
        .approval(self.approval)
        .scopes(self.scopes.clone())
        .execution_mode(bm_contract::capability::ExecutionMode::Async);
        if let Some(d) = &self.description {
            spec = spec.description(d);
        }
        spec.build()
    }
}

/// 技能脚本管理器:编译缓存 + 注册 + 执行(实现 AsyncCapabilityExecutor)。
pub struct SkillScriptManager {
    engine: Engine,
    entries: Mutex<HashMap<String, Arc<ScriptEntry>>>,
 /// 脚本默认超时(声明未指定 `timeout_ms` 时的回退)。来源 =
 /// `limits.skill_default_timeout_ms`(ADR-0024 限制集中配置面)——
    default_timeout_ms: u64,
}

impl SkillScriptManager {
    pub fn new(limits: bm_core::limits::LimitsCell) -> Result<Self, String> {
 // fuel 计量必须引擎级开启:set_fuel 才可用(死循环 wasm 的硬保险)
        let mut cfg = wasmtime::Config::new();
        cfg.consume_fuel(true);
        let engine = Engine::new(&cfg).map_err(|e| format!("wasmtime Engine 失败: {e}"))?;
        Ok(Self {
            engine,
            entries: Mutex::new(HashMap::new()),
            default_timeout_ms: limits.get().skill_default_timeout_ms,
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
        let provider = format!("skill.{skill_id}");
        for sc in scripts {
            let capability = format!("skill.{}.{}", skill_id, sc.name);
            let wasm_path = skill_root.join(&sc.path);
            let timeout_ms = sc.timeout_ms.unwrap_or(self.default_timeout_ms);
            self.register_wasm_with_origin(
                &provider,
                &capability,
                &wasm_path,
                skill_root,
                timeout_ms,
                PluginOrigin::Skill,
            )?;
        }
        self.manifests_for(skill_id, scripts)
    }

 /// 从**声明文件**装载通用 wasm 插件(ADR-0041 去特化;这是宿主在 `skills.json`
 /// 之外的第二个真实调用方)。
 /// 声明形状 = `boenmind-contracts/plugin/wasm-plugin.v0_1.schema.json`(ADR-0042
 /// 冻结):数组,每项一个 wasm 工具能力。
 /// ```json
 /// [{"capability":"demo.echo","provider":"demo.wasm","version":"0.1.0",
 /// "wasm":"echo.wasm","effect":"read-only","timeout_ms":10000,
 /// "input_schema":{"type":"object"},"output_schema":{"type":"object"},
 /// "description":"...","scopes":[]}]
 /// ```
 /// wasm 路径相对 `decl_path` 所在目录解析,并钉死在该目录内(防越界)。
 /// 返回合成 manifests;缺 `capability`/`wasm` 或**违反冻结 schema** 者跳过并告警。
    pub fn load_plugins_file(&self, decl_path: &Path) -> Vec<CapabilityManifest> {
        let Ok(text) = std::fs::read_to_string(decl_path) else {
            return Vec::new();
        };
        let Ok(items) = serde_json::from_str::<Value>(&text) else {
            eprintln!("[Plugin] {} 解析失败(已跳过)", decl_path.display());
            return Vec::new();
        };
 // ADR-0042:装载前过冻结 schema 门(与
 // 申报形状须机器可校验,不靠约定)。整文件级校验:形状错即全部不装载。
        if let Err(e) =
            bm_contract::schemas::validate(bm_contract::registries::WASM_PLUGIN_SCHEMA, &items)
        {
            eprintln!(
                "[Plugin] {} 违反 wasm 插件合同(拒绝装载): {e}",
                decl_path.display()
            );
            return Vec::new();
        }
        let Some(list) = items.as_array() else {
            return Vec::new();
        };
        let Some(root) = decl_path.parent() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for it in list {
            let Some(capability) = it["capability"].as_str() else {
                continue;
            };
            let provider = it["provider"].as_str().unwrap_or(capability);
            let Some(wasm_rel) = it["wasm"].as_str() else {
                eprintln!("[Plugin] {capability} 未声明 wasm(已跳过)");
                continue;
            };
            let timeout_ms = it["timeout_ms"].as_u64().unwrap_or(self.default_timeout_ms);
            if let Err(e) =
                self.register_wasm(provider, capability, &root.join(wasm_rel), root, timeout_ms)
            {
                eprintln!("[Plugin] {capability} 装载失败(已跳过): {e}");
                continue;
            }
            let decl = WasmDecl {
                capability: capability.to_string(),
                provider: provider.to_string(),
                version: it["version"].as_str().unwrap_or("0.1.0").to_string(),
                input_schema: it
                    .get("input_schema")
                    .cloned()
                    .unwrap_or(json!({"type": "object"})),
                output_schema: it
                    .get("output_schema")
                    .cloned()
                    .unwrap_or(json!({"type": "object"})),
 // 契约 schema 已保证 effect/approval 为合法枚举;缺省分别取
 // read-only / required(未知风险从严)。
                effect: it["effect"]
                    .as_str()
                    .and_then(bm_contract::capability::RiskClass::from_wire)
                    .unwrap_or(bm_contract::capability::RiskClass::ReadOnly),
                idempotent: it["idempotent"].as_bool().unwrap_or(false),
                timeout_ms,
                approval: it["approval"]
                    .as_str()
                    .and_then(bm_contract::capability::ApprovalRequirement::from_wire)
                    .unwrap_or(bm_contract::capability::ApprovalRequirement::Required),
                scopes: it["scopes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                description: it["description"].as_str().map(str::to_string),
            };
            match decl.synthesize() {
                Ok(m) => {
                    eprintln!("[Plugin] wasm 插件 {capability} 已装载(provider {provider})");
                    out.push(m);
                }
                Err(e) => {
                    eprintln!("[Plugin] {capability} manifest 非法(已跳过): {e}");
                }
            }
        }
        out
    }

 /// 通用 wasm 能力注册(ADR-0041 去特化):任意 capability 名 + wasm 路径。
 /// 校验 wasm 落在 `root` 内(防越界读取,同 P1-10),编译进缓存后由
 /// [`Self::run`] 按精确 capability 执行——宿主对命名空间不可知。技能装载
 /// (`register_skill`)是它的上层:命名与清单由技能声明驱动。
 /// 来源记为 [`PluginOrigin::Generic`]。
    pub fn register_wasm(
        &self,
        provider: &str,
        capability: &str,
        wasm_path: &Path,
        root: &Path,
        timeout_ms: u64,
    ) -> Result<(), String> {
        self.register_wasm_with_origin(
            provider,
            capability,
            wasm_path,
            root,
            timeout_ms,
            PluginOrigin::Generic,
        )
    }

    fn register_wasm_with_origin(
        &self,
        provider: &str,
        capability: &str,
        wasm_path: &Path,
        root: &Path,
        timeout_ms: u64,
        origin: PluginOrigin,
    ) -> Result<(), String> {
        let root_canon = root
            .canonicalize()
            .map_err(|e| format!("root 解析失败({}): {e}", root.display()))?;
        let wasm_canon = wasm_path
            .canonicalize()
            .map_err(|e| format!("wasm 路径解析失败({}): {e}", wasm_path.display()))?;
        if !wasm_canon.starts_with(&root_canon) {
            return Err(format!("wasm 路径越出 root: {}", wasm_path.display()));
        }
        let bytes = std::fs::read(&wasm_canon).map_err(|e| format!("wasm 读取失败: {e}"))?;
        let module =
            Module::from_binary(&self.engine, &bytes).map_err(|e| format!("wasm 编译失败: {e}"))?;
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                capability.to_string(),
                Arc::new(ScriptEntry {
                    capability: capability.to_string(),
                    provider: provider.to_string(),
                    origin,
                    wasm_path: wasm_canon,
                    module,
                    timeout_ms,
                }),
            );
        Ok(())
    }

 /// 注销一个技能的全部脚本(ADR-0033;幂等,可安全重复调用)。
 /// `skill_id` 映射到 provider `skill.<id>`。
    pub fn unregister_skill(&self, skill_id: &str) -> Vec<String> {
        self.unregister_provider(&format!("skill.{skill_id}"))
    }

 /// 按**提供者**摘除其全部能力(ADR-0041 热重载):返回被摘除的 capability 名
 /// (供热重载侧 `capabilities_unregister` 墓碑化)。按 `ScriptEntry.provider`
 /// 精确匹配,取代原先写死 `skill.` 前缀的拼接——通用 wasm 插件同样适用。
 /// 未装载的 provider 返回空表(幂等)。
    pub fn unregister_provider(&self, provider: &str) -> Vec<String> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let removed: Vec<String> = entries
            .iter()
            .filter(|(_, e)| e.provider == provider)
            .map(|(k, _)| k.clone())
            .collect();
        for k in &removed {
            entries.remove(k);
        }
        removed
    }

 /// 摘除全部**通用插件**来源的能力(ADR-0042:按 `origin` 判断,不按名字前缀),
 /// 返回被摘除的 capability 名。技能(origin=Skill)不受影响——由 skills.json
 /// 自己的热重载管理。供 `/admin/plugins` 整表重载使用。
    pub fn unregister_all_generic(&self) -> Vec<String> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let removed: Vec<String> = entries
            .iter()
            .filter(|(_, e)| e.origin == PluginOrigin::Generic)
            .map(|(k, _)| k.clone())
            .collect();
        for k in &removed {
            entries.remove(k);
        }
        removed
    }

 /// 本宿主是否编译了某 capability(ADR-0041 去前缀分道)。
 /// 路由用它按**归属**分道,而不是按名字前缀猜:凡进过本宿主编译表的
 /// 能力(技能脚本或通用 wasm 插件)都归 wasm 执行面。
    pub fn has_capability(&self, capability: &str) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(capability)
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
 // ADR-0049:技能脚本 → 归一化声明(与 plugins.json 共用同一合成函数)。
                let decl = WasmDecl {
                    capability: format!("skill.{}.{}", skill_id, sc.name),
                    provider: format!("skill.{}", skill_id),
                    version: "0.1.0".to_string(),
                    input_schema: sc.input_schema.clone(),
                    output_schema: sc.output_schema.clone(),
                    effect: bm_contract::capability::RiskClass::from_wire(&sc.effect)
                        .unwrap_or(bm_contract::capability::RiskClass::ReadOnly),
                    idempotent: false,
                    timeout_ms: sc.timeout_ms.unwrap_or(self.default_timeout_ms),
 // 副作用脚本必须审批;只读直通。
                    approval: if sc.effect == "read-only" {
                        bm_contract::capability::ApprovalRequirement::NotRequired
                    } else {
                        bm_contract::capability::ApprovalRequirement::Required
                    },
                    scopes: vec![format!("domain:skill.{}", skill_id)],
                    description: None,
                };
                decl.synthesize()
                    .map_err(|e| format!("脚本 {} manifest 非法: {}", sc.name, e))
            })
            .collect()
    }

 /// 执行:stdin 进 JSON,stdout 收 JSON(退出码 0 = 成功)。
    async fn run(&self, capability: &str, args: &Value) -> Result<Value, AsyncCallError> {
        let entry: Arc<ScriptEntry> = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
 /// ADR-0046:实现上移为 `bm_core::ports::skill_host::placeholder_entries`
 /// (surface 构造注册对时无需依赖 bm-providers),此处保留为该函数的中继。
    pub fn capability_entries(
        manifests: Vec<CapabilityManifest>,
    ) -> Vec<(
        CapabilityManifest,
        Arc<dyn bm_core::registry::CapabilityProvider>,
    )> {
        bm_core::ports::skill_host::placeholder_entries(manifests)
    }
}

/// ADR-0046:宿主管理面端口——surface 经此驱动装载/摘除,不再持具体类型。
impl bm_core::ports::skill_host::SkillHost for SkillScriptManager {
    fn load_plugins_file(&self, decl_path: &Path) -> Vec<CapabilityManifest> {
        SkillScriptManager::load_plugins_file(self, decl_path)
    }

    fn register_skill(
        &self,
        skill_id: &str,
        def: &SkillDefinition,
        skill_root: &Path,
    ) -> Result<Vec<CapabilityManifest>, String> {
        SkillScriptManager::register_skill(self, skill_id, def, skill_root)
    }

    fn unregister_skill(&self, skill_id: &str) -> Vec<String> {
        SkillScriptManager::unregister_skill(self, skill_id)
    }

    fn unregister_all_generic(&self) -> Vec<String> {
        SkillScriptManager::unregister_all_generic(self)
    }
}

/// 异步执行器包装:capability = `skill.<skill_id>.<script_name>`。
/// ADR-0041:不再按 `skill.` 前缀守卫——宿主本就按**精确 capability 查编译表**
/// (`run` 内的 `entries.get`),前缀判断是冗余的字符串派发。去掉后本执行器对
/// 命名空间不可知:凡注册进其编译表的 wasm 能力皆可执行,是「通用 wasm 插件
/// 宿主」的第一步;生产路由仍由 `SplitExecutor` 按 capability 分道。
#[async_trait::async_trait]
impl AsyncCapabilityExecutor for SkillScriptManager {
    async fn call(
        &self,
        _operation_id: &str,
        capability: &str,
        args: Value,
        _deadline: std::time::Duration,
    ) -> Result<Value, AsyncCallError> {
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
        let mgr =
            SkillScriptManager::new(bm_core::limits::LimitsCell::with_default()).expect("engine");
        let engine = mgr.engine.clone();
        let module = Module::new(&engine, wat).expect("wat 编译");
        let cap = "skill.demo.convert".to_string();
        mgr.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                cap.clone(),
                Arc::new(ScriptEntry {
                    capability: cap.clone(),
                    provider: "skill.demo".to_string(),
                    origin: PluginOrigin::Skill,
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

 // ADR-0041:宿主对命名空间不可知——能力名不必带 `skill.` 前缀,按精确
 // capability 查编译表即可执行(通用 wasm 插件宿主的第一步)。
 #[tokio::test]
    async fn host_is_namespace_agnostic() {
        let (mgr, _skill_cap) = manager_with_wat(ECHO_WAT);
        let engine = mgr.engine.clone();
        let module = Module::new(&engine, ECHO_WAT).expect("wat 编译");
        let cap = "plugin.demo.echo".to_string();
        mgr.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                cap.clone(),
                Arc::new(ScriptEntry {
                    capability: cap.clone(),
                    provider: "plugin.demo".to_string(),
                    origin: PluginOrigin::Generic,
                    wasm_path: PathBuf::from("demo.wat"),
                    module,
                    timeout_ms: 5_000,
                }),
            );
        let out = AsyncCapabilityExecutor::call(
            &mgr,
            "op-1",
            &cap,
            serde_json::json!({}),
            std::time::Duration::from_secs(5),
        )
        .await
        .expect("非 skill 前缀的已编译能力应可执行");
        assert_eq!(out["ok"], serde_json::json!(true));
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
        let mgr =
            SkillScriptManager::new(bm_core::limits::LimitsCell::with_default()).expect("engine");
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

 // ADR-0041 第二个真实调用方:声明文件装载通用 wasm 插件——非 skill.* 能力
 // 经声明文件进入宿主,合成 manifest 且执行体可触达。
 #[tokio::test]
    async fn load_plugins_file_registers_generic_wasm_capability() {
        let dir = tempfile::tempdir().expect("临时目录");
        std::fs::write(
            dir.path().join("echo.wasm"),
            wat::parse_str(ECHO_WAT).expect("wat→wasm"),
        )
        .expect("写 wasm");
        let decl = dir.path().join("plugins.json");
        std::fs::write(
            &decl,
            serde_json::json!([{
                "capability": "demo.echo",
                "provider": "demo.wasm",
                "version": "0.2.0",
                "wasm": "echo.wasm",
                "effect": "read-only",
                "timeout_ms": 5000
            }])
            .to_string(),
        )
        .expect("写声明");

        let mgr =
            SkillScriptManager::new(bm_core::limits::LimitsCell::with_default()).expect("engine");
        let manifests = mgr.load_plugins_file(&decl);
        assert_eq!(manifests.len(), 1, "声明应装载出一个能力");
        assert_eq!(manifests[0].capability, "demo.echo");
        assert_eq!(manifests[0].provider, "demo.wasm");
 // 声明装载的能力真的进了宿主编译表,且可按精确名执行。
        assert!(mgr.has_capability("demo.echo"));
        let out = AsyncCapabilityExecutor::call(
            &mgr,
            "op-1",
            "demo.echo",
            serde_json::json!({}),
            std::time::Duration::from_secs(5),
        )
        .await
        .expect("声明装载的能力应可执行");
        assert_eq!(out["ok"], serde_json::json!(true));

 // wasm 路径越出声明目录 → 该条跳过(不装载、不报错击穿)。
        let bad = dir.path().join("bad.json");
        std::fs::write(
            &bad,
            serde_json::json!([{
                "capability": "demo.bad", "provider": "demo.bad",
                "wasm": "../outside.wasm", "effect": "read-only"
            }])
            .to_string(),
        )
        .expect("写声明");
        assert!(
            mgr.load_plugins_file(&bad).is_empty(),
            "越界 wasm 必须被跳过"
        );
    }

 // ADR-0042:声明必须过**冻结 schema 门**——非法形状(未知字段/坏能力名)拒绝装载,
 // 证明这道门不是装饰。。
 #[test]
    fn load_plugins_file_rejects_schema_violations() {
        let dir = tempfile::tempdir().expect("临时目录");
        std::fs::write(
            dir.path().join("echo.wasm"),
            wat::parse_str(ECHO_WAT).expect("wat→wasm"),
        )
        .expect("写 wasm");
        let mgr =
            SkillScriptManager::new(bm_core::limits::LimitsCell::with_default()).expect("engine");

 // 未知字段(additionalProperties:false)→ 拒绝
        let unknown = dir.path().join("unknown.json");
        std::fs::write(
            &unknown,
            serde_json::json!([{
                "capability": "demo.echo", "wasm": "echo.wasm",
                "unexpected_field": true
            }])
            .to_string(),
        )
        .expect("写");
        assert!(
            mgr.load_plugins_file(&unknown).is_empty(),
            "未知字段必须被合同拒绝"
        );

 // 非法能力名(缺命名空间段)→ 拒绝
        let badname = dir.path().join("badname.json");
        std::fs::write(
            &badname,
            serde_json::json!([{ "capability": "NoNamespace", "wasm": "echo.wasm" }]).to_string(),
        )
        .expect("写");
        assert!(
            mgr.load_plugins_file(&badname).is_empty(),
            "非法能力名必须被合同拒绝"
        );

 // 合法声明仍通过(门的正例,防"全拒")→ 装载成功
        let ok = dir.path().join("ok.json");
        std::fs::write(
            &ok,
            serde_json::json!([{ "capability": "demo.ok", "wasm": "echo.wasm" }]).to_string(),
        )
        .expect("写");
        assert_eq!(
            mgr.load_plugins_file(&ok).len(),
            1,
            "合法声明必须通过合同门"
        );
    }

 // ADR-0041:wasm 插件以 Tool 身份注册,内核可读到「谁提供」。
 #[test]
    fn capability_entries_declare_plugin_identity() {
        let mgr =
            SkillScriptManager::new(bm_core::limits::LimitsCell::with_default()).expect("engine");
        let def: SkillDefinition = serde_json::from_value(serde_json::json!({
            "skill_id": "units",
            "name": "换算",
            "instruction": "x",
            "scripts": [{
                "name": "convert",
                "path": "scripts/convert.wasm",
                "effect": "read-only",
                "input_schema": {"type": "object"},
                "output_schema": {"type": "object"}
            }]
        }))
        .expect("合法");
        let manifests = mgr
            .manifests_for("units", def.scripts.as_ref().unwrap())
            .expect("manifests");
        let entries = SkillScriptManager::capability_entries(manifests);
        let meta = entries[0].1.plugin_meta().expect("wasm 插件必须有身份");
        assert_eq!(meta.id, "skill.units");
        assert_eq!(meta.kind, bm_contract::plugin::PluginKind::Tool);
    }

 // P1-10():`..` 越出技能根目录的脚本路径必须拒绝。
 #[test]
    fn script_path_escaping_skill_root_is_rejected() {
        let dir = tempfile::tempdir().expect("临时目录");
        let secret = dir.path().join("secret.bin");
        std::fs::write(&secret, b"MZ-not-wasm").expect("写外部文件");
        let skill_root = dir.path().join("skill");
        std::fs::create_dir_all(&skill_root).expect("建技能目录");
        let mgr =
            SkillScriptManager::new(bm_core::limits::LimitsCell::with_default()).expect("engine");
        let def: SkillDefinition = serde_json::from_value(serde_json::json!({
            "skill_id": "escape",
            "name": "越界",
            "instruction": "x",
            "scripts": [{
                "name": "steal",
                "path": "../secret.bin",
                "effect": "read-only",
                "input_schema": {"type": "object"},
                "output_schema": {"type": "object"}
            }]
        }))
        .expect("合法");
        let err = mgr
            .register_skill("escape", &def, &skill_root)
            .expect_err("越界路径必须被拒绝");
        assert!(err.contains("越出 root"), "{err}");
    }

 // ADR-0041 去特化:通用 register_wasm 支持任意 capability 名(非 skill.*),
 // 注册后按精确名可执行;越界路径同样被拒。
 #[test]
    fn generic_register_wasm_accepts_any_capability_name() {
        let dir = tempfile::tempdir().expect("临时目录");
        let wasm = dir.path().join("demo.wasm");
 // 真 wasm 字节:from_binary 只吃二进制,不经文本嗅探。
        let bytes = wat::parse_str(ECHO_WAT).expect("wat→wasm");
        std::fs::write(&wasm, &bytes).expect("写 wasm");

        let mgr =
            SkillScriptManager::new(bm_core::limits::LimitsCell::with_default()).expect("engine");
        mgr.register_wasm("plugin.demo", "plugin.demo.echo", &wasm, dir.path(), 5_000)
            .expect("通用注册应接受任意 capability 名");
        assert!(mgr.entries.lock().unwrap().contains_key("plugin.demo.echo"));
 // ADR-0041 热重载支点:按 provider 精确摘除其全部能力。
        let removed = mgr.unregister_provider("plugin.demo");
        assert_eq!(removed, vec!["plugin.demo.echo".to_string()]);
        assert!(!mgr.has_capability("plugin.demo.echo"), "摘除后不可达");

 // 越界路径仍拒(canonicalize 把 root 外的目标判否)
        let outside = tempfile::tempdir().expect("另一目录");
        let other = outside.path().join("x.wasm");
        std::fs::write(&other, wat::parse_str(ECHO_WAT).expect("wat→wasm")).expect("写");
        let err = mgr
            .register_wasm("plugin.bad", "plugin.bad", &other, dir.path(), 1_000)
            .expect_err("越界必须拒");
        assert!(err.contains("越出 root"), "{err}");
    }

 // (ADR-0024 限制集中配置面)。
 // 本测试锁死:非默认 limits 值必须落到合成 manifest。
 #[test]
    fn default_timeout_follows_limits_cell() {
        let limits = bm_core::limits::Limits {
            skill_default_timeout_ms: 12_345,
            ..Default::default()
        };
        let mgr =
            SkillScriptManager::new(bm_core::limits::LimitsCell::new(limits)).expect("engine");
        let def: SkillDefinition = serde_json::from_value(serde_json::json!({
            "skill_id": "limits_demo",
            "name": "超时",
            "instruction": "x",
            "scripts": [{
                "name": "s",
                "path": "s.wasm",
                "effect": "read-only",
                "input_schema": {"type": "object"},
                "output_schema": {"type": "object"}
            }]
        }))
        .expect("合法");
        let manifests = mgr
            .manifests_for("limits_demo", def.scripts.as_ref().unwrap())
            .expect("manifests");
        assert_eq!(
            manifests[0].timeout_ms, 12_345,
            "默认超时须随 limits 生效(声明未给 timeout_ms 时)"
        );
    }
}
