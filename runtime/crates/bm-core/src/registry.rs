//! Capability Registry(基线 §6.2/§6.4,M4.1):「谁提供什么」的统一注册中心。
//!
//! 两层结构(基线 §6.4):
//! - 持久逻辑目录:manifest + binding 元数据(instance_id/epoch/status),
//! 重启后由持久层恢复(T3 接 SQLite capabilities 表;`restore_binding`
//! 是恢复入口,epoch 不回退);
//! - 可丢失运行时缓存:Provider 实例句柄 + 健康位,重启重建(`clear_runtime_cache`
//! 演示可丢失性:清空后行为不变,重新 attach 即恢复)。
//!
//! binding_epoch 只增不回退(ADR-0001 条件 2):register 起步 1;此后每次
//! 重新注册(热重载/重启装载)由注册方以「持久行 max+1」抬升后再落库——
//! 物理删行会让重注册归零,故注销只留墓碑(status=unavailable),行随代际
//! 存续。恢复语义见 `restore_binding`(取 max,不回退);进程内热替换原语
//! `switch_binding` 做 +1。同代内已签发在途调用的 (epoch, instance) 归属
//! 不被重注册覆盖,授权-执行-审计三方可按代际对账。
//!
//! 注册面只回答「谁提供什么」;能不能调用是 Broker 的裁决(基线 §7)——
//! 本模块不持有任何策略。

use bm_contract::capability::{CapabilityManifest, MutationClass};
use std::collections::HashMap;
use std::sync::Arc;

/// Provider 执行端口(M4 = 内置 Rust 实现;独立进程形态随 M7,调用方无感,
/// 基线 §7)。args 已由 Broker 过 manifest input_schema;返回值由 Broker
/// 过 output_schema(M4.3)。
/// 分层关系(与 [`crate::ports::AsyncCapabilityExecutor`] 的分工,
/// 审计轮注释):两者共用同一条 Broker 决策管线(身份/凭据/预扣/intent 门),
/// 仅执行步分道——`is_async()` 为真(外部慢路径,如 MCP)走异步执行器
/// (运行期 spawn + manifest.timeout_ms 钳制超时 + 取消令牌 + 进度回流),
/// 否则在本任务内联同步执行(panic 收容)。选型约束:同步实现不得长时间
/// 阻塞——会占住单写者循环,耗时能力一律注册为异步。
/// 插件行为契约(ADR-0041/0045):`invoke` 是能力执行面,`plugin_meta` 是身份面。
/// `plugin_meta` 带默认实现,故既有 provider 零改动即满足契约;需要声明身份的
/// 扩展(wasm/mcp/内置)覆写它即可,发现面(`CapabilityRegistry::discover`)是
/// 它的真实消费者。
/// **无 `shutdown` 钩子**(ADR-0045 收敛):
/// 实现(唯二出现是 trait 默认与测试替身)——因为资源归**执行器**而非 provider
/// 占位符所有:wasm 模块由 `SkillScriptManager::unregister_provider` 摘除、MCP
/// 子进程由 `McpHub::disconnect_server` 清理。留一个永远返回 `Ok(())` 的钩子是
/// 空转,故移除;真正持有资源的扩展应把释放放在其所属执行器里。
pub trait CapabilityProvider: Send + Sync {
    fn invoke(&self, args: serde_json::Value) -> Result<serde_json::Value, String>;

 /// 插件身份(kind/id/version)。默认 `None` = 未声明,按工具型
 /// ([`bm_contract::plugin::PluginKind::Tool`])对待。
    fn plugin_meta(&self) -> Option<bm_contract::plugin::PluginMeta> {
        None
    }
}

/// binding 生命周期状态(基线 §13.1/§13.2;状态持久于逻辑目录层)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BindingStatus {
    Active,
    Draining,
    Unavailable,
}

/// 持久逻辑目录中的 binding 记录(不含内存句柄)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub provider_instance_id: String,
    pub epoch: u64,
    pub status: BindingStatus,
}

/// 可丢失运行时缓存(基线 §6.4:重启后丢失并重建)。
#[derive(Default, Clone)]
struct RuntimeCache {
    handle: Option<Arc<dyn CapabilityProvider>>,
    healthy: bool,
}

impl std::fmt::Debug for RuntimeCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeCache")
            .field("handle", &self.handle.as_ref().map(|_| "<provider>"))
            .field("healthy", &self.healthy)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
 /// capability 已注册:重复注册走 `switch_binding`,不是重新 register。
    AlreadyRegistered,
    UnknownCapability,
 /// Provider 无故报告恢复(未处于 Unavailable)。
    InvalidTransition,
 /// manifest 未过冻结合同(capability/manifest.v0_1)——pattern/枚举级
 /// 约束 serde 兜不住,注册期必须拦(
 /// 冻结 schema 只在 bm-contract 测试里被消费)。
    InvalidManifest(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::AlreadyRegistered => write!(f, "capability 已注册"),
            RegistryError::UnknownCapability => write!(f, "未知 capability"),
            RegistryError::InvalidTransition => write!(f, "非法 binding 状态迁移"),
            RegistryError::InvalidManifest(e) => write!(f, "manifest 未过冻结合同: {e}"),
        }
    }
}

/// 注册期冻结合同门禁。Option 字段 None 序列化为 null,而 mutation_class
/// (纯 enum、不接受 null)等可选字段在合同中按「缺省即缺席」表述——
/// 校验前剥除 null 值可选键,得到与合同表述同形的实例。
fn validate_frozen_manifest(manifest: &CapabilityManifest) -> Result<(), RegistryError> {
    let mut value = serde_json::to_value(manifest)
        .map_err(|e| RegistryError::InvalidManifest(format!("manifest 序列化失败: {e}")))?;
    if let Some(obj) = value.as_object_mut() {
        for key in [
            "verification",
            "undo",
            "retry",
            "deprecated_by",
            "mutation_class",
            "description",
            "execution_mode",
            "authorization",
        ] {
            if obj.get(key).is_some_and(|v| v.is_null()) {
                obj.remove(key);
            }
        }
    }
    bm_contract::schemas::validate(bm_contract::registries::CAPABILITY_MANIFEST_SCHEMA, &value)
        .map_err(RegistryError::InvalidManifest)
}

/// 机器可读发现结果(基线 §6.4:CLI/Surface 的发现面由此生成,不另维护定义)。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CapabilityDiscovery {
    pub capability: String,
    pub provider: String,
    pub version: String,
    pub effect: bm_contract::capability::RiskClass,
    pub mutation_class: MutationClass,
    pub idempotent: bool,
    pub cancellable: bool,
    pub timeout_ms: u64,
    pub approval: bm_contract::capability::ApprovalRequirement,
    pub scopes: Vec<String>,
    pub binding_epoch: u64,
    pub provider_instance_id: String,
    pub status: BindingStatus,
    pub healthy: bool,
 /// 插件身份(ADR-0041/0045):提供者声明的 `PluginKind` 与 id/version。
 /// `None` = provider 未声明身份(按工具型对待)。此项使发现面成为插件
 /// 身份的**真实消费者**——管理面据此渲染徽标,不再由前端按命名猜测。
 #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_kind: Option<bm_contract::plugin::PluginKind>,
 #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
 #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_version: Option<String>,
}

#[derive(Debug, Default)]
pub struct CapabilityRegistry {
    manifests: HashMap<String, CapabilityManifest>,
    bindings: HashMap<String, Binding>,
    cache: HashMap<String, RuntimeCache>,
 /// M7:异步执行标记。注册本身不自动判定;由装载方按 manifest.provider
 /// 显式 mark_async("mcp." 前缀或内置 ".async" 后缀,见 runtime/handle.rs)。
 /// 可丢失缓存——每次启动随注册流程重建。
    async_exec: std::collections::HashSet<String>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

 /// 首次注册:manifest 过冻结合同门禁后进逻辑目录,binding 建立并分配
 /// epoch=1。重注册(热重载/重启装载)的代际抬升由注册方在调用后按
 /// 「持久行 max+1」走 `restore_binding` 完成——registry 自身不见持久层。
    pub fn register(
        &mut self,
        manifest: CapabilityManifest,
        provider_instance_id: &str,
        handle: Arc<dyn CapabilityProvider>,
    ) -> Result<u64, RegistryError> {
        validate_frozen_manifest(&manifest)?;
        let name = manifest.capability.clone();
        if self.manifests.contains_key(&name) {
            return Err(RegistryError::AlreadyRegistered);
        }
 // ADR-0036:执行分道以合同声明为真源——显式声明即落定,未声明留待
 // mark_async_for 走 provider 命名约定回退。
        match manifest.execution_mode {
            Some(bm_contract::capability::ExecutionMode::Async) => {
                self.async_exec.insert(name.clone());
            }
            Some(bm_contract::capability::ExecutionMode::Sync) => {
                self.async_exec.remove(&name);
            }
            None => {}
        }
        self.manifests.insert(name.clone(), manifest);
        self.bindings.insert(
            name.clone(),
            Binding {
                provider_instance_id: provider_instance_id.to_string(),
                epoch: 1,
                status: BindingStatus::Active,
            },
        );
        self.cache.insert(
            name,
            RuntimeCache {
                handle: Some(handle),
                healthy: true,
            },
        );
        Ok(1)
    }

 /// 注销能力(热拔/重载移除;从逻辑目录、bindings 与缓存中彻底摘除)。
    pub fn unregister(&mut self, capability: &str) -> bool {
        let removed_m = self.manifests.remove(capability).is_some();
        self.bindings.remove(capability);
        self.cache.remove(capability);
        self.async_exec.remove(capability);
        removed_m
    }

 /// 能力所属插件的身份(ADR-0041);Provider 未声明时返回 `None`。
 /// 发现面(`discover`)是它的真实消费者(ADR-0045)。
    pub fn plugin_meta_of(&self, capability: &str) -> Option<bm_contract::plugin::PluginMeta> {
        let handle = self.cache.get(capability)?.handle.as_ref()?;
        handle.plugin_meta()
    }

 /// 热替换(基线 §13.1 的注册面半边):原子切换 instance,epoch+1。
 /// 在途调用的授权-执行-审计归属由调用凭证中的旧 epoch 保全(Broker 侧)。
    pub fn switch_binding(
        &mut self,
        capability: &str,
        provider_instance_id: &str,
        handle: Arc<dyn CapabilityProvider>,
    ) -> Result<u64, RegistryError> {
        let binding = self
            .bindings
            .get_mut(capability)
            .ok_or(RegistryError::UnknownCapability)?;
        binding.provider_instance_id = provider_instance_id.to_string();
        binding.epoch += 1;
        binding.status = BindingStatus::Active;
        let cache = self.cache.entry(capability.to_string()).or_default();
        cache.handle = Some(handle);
        cache.healthy = true;
        Ok(binding.epoch)
    }

 /// Provider 崩溃/失联(基线 §13.2):标记 unavailable;epoch 不变
 /// (binding 未切换,只是当前实例不可用)。
    pub fn mark_unavailable(&mut self, capability: &str) -> Result<(), RegistryError> {
        let binding = self
            .bindings
            .get_mut(capability)
            .ok_or(RegistryError::UnknownCapability)?;
        if binding.status != BindingStatus::Active {
            return Err(RegistryError::InvalidTransition);
        }
        binding.status = BindingStatus::Unavailable;
        self.cache
            .entry(capability.to_string())
            .or_default()
            .healthy = false;
        Ok(())
    }

 /// 实例恢复(基线 §13.2:重启→重新 handshake→恢复 binding):
 /// 新实例 = 新 binding,epoch+1。
    pub fn mark_recovered(
        &mut self,
        capability: &str,
        provider_instance_id: &str,
        handle: Arc<dyn CapabilityProvider>,
    ) -> Result<u64, RegistryError> {
        let binding = self
            .bindings
            .get_mut(capability)
            .ok_or(RegistryError::UnknownCapability)?;
        if binding.status != BindingStatus::Unavailable {
            return Err(RegistryError::InvalidTransition);
        }
        binding.provider_instance_id = provider_instance_id.to_string();
        binding.epoch += 1;
        binding.status = BindingStatus::Active;
        let cache = self.cache.entry(capability.to_string()).or_default();
        cache.handle = Some(handle);
        cache.healthy = true;
        Ok(binding.epoch)
    }

 /// ADR-0037:进入排空(卸载前置)。`Active`→`Draining`;此后 dispatch 生命
 /// 周期门拒绝新调用,在途调用继续至落定,完成后再摘除——卸载不再在在途
 /// 调用中途拔路由。非 `Active` 迁移非法。
    pub fn begin_drain(&mut self, capability: &str) -> Result<(), RegistryError> {
        let binding = self
            .bindings
            .get_mut(capability)
            .ok_or(RegistryError::UnknownCapability)?;
        if binding.status != BindingStatus::Active {
            return Err(RegistryError::InvalidTransition);
        }
        binding.status = BindingStatus::Draining;
 // 排空期句柄保留至摘除,但标记不健康——dispatch 生命周期门已拒新调用。
        if let Some(c) = self.cache.get_mut(capability) {
            c.healthy = false;
        }
        Ok(())
    }

 /// ADR-0037:排空完成。`Draining`→`Unavailable`(binding 行留墓碑,
 /// epoch 不变——摘除由 `unregister` / 持久层完成,代际不回退)。
    pub fn finish_drain(&mut self, capability: &str) -> Result<(), RegistryError> {
        let binding = self
            .bindings
            .get_mut(capability)
            .ok_or(RegistryError::UnknownCapability)?;
        if binding.status != BindingStatus::Draining {
            return Err(RegistryError::InvalidTransition);
        }
        binding.status = BindingStatus::Unavailable;
        Ok(())
    }

 /// 重启恢复入口(T3 由 SQLite capabilities 表驱动):以持久值恢复逻辑
 /// 目录;epoch 取 max(现值, 持久值)——不回退(ADR-0001 条件 2)。
 /// ADR-0037:状态同样以持久值恢复(不再硬编码 `Active`),使重启后
 /// `discover()` 的 status 与实际一致(unavailable 墓碑不误回升)。
 /// 返回生效 epoch。
    pub fn restore_binding(
        &mut self,
        manifest: CapabilityManifest,
        provider_instance_id: &str,
        epoch: u64,
        status: BindingStatus,
    ) -> u64 {
        let name = manifest.capability.clone();
        let effective = match self.bindings.get(&name) {
            Some(existing) => existing.epoch.max(epoch),
            None => epoch,
        };
        self.manifests.insert(name.clone(), manifest);
        self.bindings.insert(
            name.clone(),
            Binding {
                provider_instance_id: provider_instance_id.to_string(),
                epoch: effective,
                status,
            },
        );
 // 运行时缓存不在恢复范围:句柄由注册流程重新 attach(可丢失语义)。
        self.cache.remove(&name);
        effective
    }

 /// W4b 对话工具闭环:枚举供对话 Agent 使用的全部能力(含直通与需审批的业务能力)。
 /// 排除内核私有能力(如 model.invoke)。
 /// needs_approval 与 Broker 步 5 判定同口径:effect 可审批类
 /// (reversible/external/high-risk)或 manifest 声明 required → true。
 /// 第 4 元 = manifest.description(ADR-0022 合同 Minor):面向模型的
 /// 一句功能描述;fs.*/system.exec 内置能力与 MCP 工具自描述,缺省 None
 /// 由 turn 侧兜底。
 /// 工具」套话,是工具调用别扭的直接根因之一。
 /// (P1-47,
 /// 审批语义的权威判定在 Broker,本方法只是面向模型 tools 表的投影,
 /// 改判定先改 broker/mod.rs 步 5,此处随之。)
    pub fn chat_tools(&self) -> Vec<(String, serde_json::Value, bool, Option<String>)> {
        let mut out: Vec<(String, serde_json::Value, bool, Option<String>)> = self
            .manifests
            .iter()
            .filter(|(_, m)| m.capability != "model.invoke")
            .map(|(name, m)| {
                let require_approval = m.effect.is_approval_bearing()
                    || m.approval == bm_contract::capability::ApprovalRequirement::Required;
                (
                    name.clone(),
                    m.input_schema.clone(),
                    require_approval,
                    m.description.clone(),
                )
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn manifest_of(&self, capability: &str) -> Option<&CapabilityManifest> {
        self.manifests.get(capability)
    }

    pub fn binding_of(&self, capability: &str) -> Option<&Binding> {
        self.bindings.get(capability)
    }

 /// M7:标记该能力走异步执行路径(dispatch 不再同步等 Provider)。
    pub fn mark_async(&mut self, capability: &str) {
        self.async_exec.insert(capability.to_string());
    }

 /// 异步分道判定的唯一真源(ADR-0033):按 `manifest.provider` 命名约定判定——
 /// - `mcp.*` 外部 MCP 子进程(启动装载与热装载同判);
 /// - `*.async` 内置异步执行体(如 `system.exec` 的 `builtin.async`);
 /// - `skill.*` wasm 脚本执行面(ADR-0016 第二步)。
 /// 分道,实际落到同步占位 provider 报错;本谓词收口两处调用点。
    pub fn provider_is_async(provider: &str) -> bool {
        provider.starts_with("mcp.")
            || provider.ends_with(".async")
            || provider.starts_with("skill.")
    }

 /// 依 [`Self::provider_is_async`] 自动标记异步;返回是否异步。
 /// ADR-0036:manifest 已显式声明 `execution_mode` 时以声明为准(register
 /// 已落定),命名约定只作未声明条目的兼容回退。
    pub fn mark_async_for(&mut self, capability: &str, provider: &str) -> bool {
        if let Some(mode) = self
            .manifests
            .get(capability)
            .and_then(|m| m.execution_mode)
        {
            return matches!(mode, bm_contract::capability::ExecutionMode::Async);
        }
        let is_async = Self::provider_is_async(provider);
        if is_async {
            self.mark_async(capability);
        }
        is_async
    }

    pub fn is_async(&self, capability: &str) -> bool {
        self.async_exec.contains(capability)
    }

    pub fn handle_of(&self, capability: &str) -> Option<Arc<dyn CapabilityProvider>> {
        self.cache.get(capability)?.handle.clone()
    }

 /// 重新挂接运行时句柄(缓存重建;不影响 epoch/状态)。
    pub fn attach_handle(
        &mut self,
        capability: &str,
        handle: Arc<dyn CapabilityProvider>,
    ) -> Result<(), RegistryError> {
        if !self.bindings.contains_key(capability) {
            return Err(RegistryError::UnknownCapability);
        }
        let cache = self.cache.entry(capability.to_string()).or_default();
        cache.handle = Some(handle);
        cache.healthy = true;
        Ok(())
    }

    pub fn is_available(&self, capability: &str) -> bool {
        self.bindings
            .get(capability)
            .is_some_and(|b| b.status == BindingStatus::Active)
            && self
                .cache
                .get(capability)
                .is_some_and(|c| c.healthy && c.handle.is_some())
    }

 /// 演示/测试可丢失性:清空运行时缓存,逻辑目录(manifest/binding/epoch)
 /// 不受影响——清空后行为与缓存命中时一致是架构守护断言 G3 的基础。
    pub fn clear_runtime_cache(&mut self) {
        self.cache.clear();
    }

 /// 机器可读发现面(基线 §6.4):按 capability 名稳定排序。
    pub fn discover(&self) -> Vec<CapabilityDiscovery> {
        let mut out: Vec<CapabilityDiscovery> = self
            .manifests
            .iter()
            .map(|(name, m)| {
                let binding = self.bindings.get(name).cloned().unwrap_or_else(|| Binding {
                    provider_instance_id: m.provider.clone(),
                    epoch: 0,
                    status: BindingStatus::Unavailable,
                });
                let meta = self.plugin_meta_of(name);
                CapabilityDiscovery {
                    capability: m.capability.clone(),
                    provider: m.provider.clone(),
                    version: m.version.clone(),
                    effect: m.effect,
                    mutation_class: m.mutation_class_or_derived(),
                    idempotent: m.idempotent,
                    cancellable: m.cancellable,
                    timeout_ms: m.timeout_ms,
                    approval: m.approval,
                    scopes: m.scopes.clone(),
                    binding_epoch: binding.epoch,
                    provider_instance_id: binding.provider_instance_id,
                    status: binding.status,
                    healthy: self
                        .cache
                        .get(name)
                        .is_some_and(|c| c.healthy && c.handle.is_some()),
 // ADR-0045:取 provider 声明的插件身份(发现面 = 真实消费者)
                    plugin_kind: meta.as_ref().map(|p| p.kind),
                    plugin_id: meta.as_ref().map(|p| p.id.clone()),
                    plugin_version: meta.map(|p| p.version),
                }
            })
            .collect();
        out.sort_by(|a, b| a.capability.cmp(&b.capability));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_contract::capability::{ApprovalRequirement, RiskClass};

    fn manifest(name: &str) -> CapabilityManifest {
        serde_json::from_value(serde_json::json!({
            "capability": name, "provider": name, "version": "0.1.0",
            "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
            "effect": "read-only", "idempotent": true, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required"
        }))
        .unwrap()
    }

    struct Echo;
    impl CapabilityProvider for Echo {
        fn invoke(&self, args: serde_json::Value) -> Result<serde_json::Value, String> {
            Ok(args)
        }
    }

 #[test]
    fn plugin_identity_is_consumed_by_discovery() {
 // ADR-0045:插件身份的**真实消费者 = 发现面**(。
        struct WasmLike;
        impl CapabilityProvider for WasmLike {
            fn invoke(&self, _args: serde_json::Value) -> Result<serde_json::Value, String> {
                Err("异步路径能力的同步占位不应被调用".into())
            }
            fn plugin_meta(&self) -> Option<bm_contract::plugin::PluginMeta> {
                Some(bm_contract::plugin::PluginMeta::new(
                    "demo.wasm",
                    "1.2.3",
                    bm_contract::plugin::PluginKind::Tool,
                ))
            }
        }

        let mut reg = CapabilityRegistry::new();
        reg.register(manifest("demo.tool"), "demo.wasm@1.2.3", Arc::new(WasmLike))
            .expect("注册");
 // 未声明身份的 provider:plugin_meta=None,不 panic。
        reg.register(manifest("demo.echo"), "demo.echo@0.1.0", Arc::new(Echo))
            .expect("注册");

        let discovered = reg.discover();
        let tool = discovered
            .iter()
            .find(|d| d.capability == "demo.tool")
            .expect("在发现面");
        assert_eq!(
            tool.plugin_kind,
            Some(bm_contract::plugin::PluginKind::Tool),
            "发现面必须承载插件身份(真实消费者)"
        );
        assert_eq!(tool.plugin_id.as_deref(), Some("demo.wasm"));
        assert_eq!(tool.plugin_version.as_deref(), Some("1.2.3"));

        let echo = discovered
            .iter()
            .find(|d| d.capability == "demo.echo")
            .expect("在发现面");
        assert!(echo.plugin_kind.is_none(), "未声明身份的 provider 身份为空");

 // 注销后身份随能力一起消失。
        assert!(reg.unregister("demo.tool"));
        assert!(reg.plugin_meta_of("demo.tool").is_none());
        assert!(
            !reg.discover().iter().any(|d| d.capability == "demo.tool"),
            "注销后不在发现面"
        );
    }

 #[test]
    fn manifest_must_pass_frozen_schema_at_registration() {
        let mut reg = CapabilityRegistry::new();
 // serde 形状合法但违冻结 pattern(大写能力名段)→ 注册期必须拦
        let bad: CapabilityManifest = serde_json::from_value(serde_json::json!({
            "capability": "Bad.Name", "provider": "bad.name", "version": "0.1.0",
            "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
            "effect": "read-only", "idempotent": true, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required"
        }))
        .unwrap();
        assert!(matches!(
            reg.register(bad, "bad.name@0.1.0", Arc::new(Echo)),
            Err(RegistryError::InvalidManifest(_))
        ));
        assert!(reg.manifest_of("Bad.Name").is_none(), "拒注后不留痕");

 // 冒号分层 scope(生产实况:domain:fs / domain:mcp.<server>)必须过
        let scoped: CapabilityManifest = serde_json::from_value(serde_json::json!({
            "capability": "system.scoped", "provider": "system.scoped", "version": "0.1.0",
            "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
            "effect": "read-only", "idempotent": true, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required",
            "scopes": ["domain:fs"]
        }))
        .unwrap();
        reg.register(scoped, "system.scoped@0.1.0", Arc::new(Echo))
            .expect("domain:fs 形态 scope 必须过冻结合同");
    }

 #[test]
    fn register_assigns_epoch_one_and_discovery_is_complete() {
        let mut reg = CapabilityRegistry::new();
        let epoch = reg
            .register(manifest("system.echo"), "system.echo@0.1.0", Arc::new(Echo))
            .unwrap();
        assert_eq!(epoch, 1);
        assert!(reg.is_available("system.echo"));

        let d = reg.discover();
        assert_eq!(d.len(), 1);
        let d = &d[0];
        assert_eq!(d.capability, "system.echo");
        assert_eq!(d.effect, RiskClass::ReadOnly);
        assert_eq!(d.mutation_class, MutationClass::Safe);
        assert_eq!(d.approval, ApprovalRequirement::NotRequired);
        assert_eq!(d.binding_epoch, 1);
        assert_eq!(d.provider_instance_id, "system.echo@0.1.0");
        assert!(d.healthy);
    }

 #[test]
    fn duplicate_register_is_rejected_use_switch() {
        let mut reg = CapabilityRegistry::new();
        reg.register(manifest("system.echo"), "system.echo@0.1.0", Arc::new(Echo))
            .unwrap();
        assert_eq!(
            reg.register(manifest("system.echo"), "system.echo@0.2.0", Arc::new(Echo)),
            Err(RegistryError::AlreadyRegistered)
        );
    }

 #[test]
    fn switch_binding_increments_epoch_manifest_unchanged() {
        let mut reg = CapabilityRegistry::new();
        reg.register(manifest("system.echo"), "system.echo@0.1.0", Arc::new(Echo))
            .unwrap();
        let epoch = reg
            .switch_binding("system.echo", "system.echo@0.2.0", Arc::new(Echo))
            .unwrap();
        assert_eq!(epoch, 2);
        let b = reg.binding_of("system.echo").unwrap();
        assert_eq!(b.provider_instance_id, "system.echo@0.2.0");
        assert_eq!(b.epoch, 2);
        assert_eq!(reg.manifest_of("system.echo").unwrap().version, "0.1.0");
        assert!(reg.is_available("system.echo"));
    }

 #[test]
    fn unavailable_then_recovered_increments_epoch() {
        let mut reg = CapabilityRegistry::new();
        reg.register(manifest("system.echo"), "system.echo@0.1.0", Arc::new(Echo))
            .unwrap();

 // Active 状态下不得报告恢复(仅 Unavailable → recovered 合法)
        assert_eq!(
            reg.mark_recovered("system.echo", "system.echo@0.1.0-r0", Arc::new(Echo)),
            Err(RegistryError::InvalidTransition)
        );

        reg.mark_unavailable("system.echo").unwrap();
        assert!(!reg.is_available("system.echo"));

 // unavailable → 重新 handshake → epoch+1
        let epoch = reg
            .mark_recovered("system.echo", "system.echo@0.1.0-r2", Arc::new(Echo))
            .unwrap();
        assert_eq!(epoch, 2);
        assert!(reg.is_available("system.echo"));

 // 未知 capability
        assert_eq!(
            reg.mark_unavailable("system.nope"),
            Err(RegistryError::UnknownCapability)
        );
    }

 #[test]
    fn restore_never_decreases_epoch() {
        let mut reg = CapabilityRegistry::new();
 // 持久层记录 epoch=7,运行时为空 → 恢复 7
        let e = reg.restore_binding(
            manifest("system.echo"),
            "system.echo@0.1.0",
            7,
            BindingStatus::Active,
        );
        assert_eq!(e, 7);
 // 运行时已有 epoch=2,持久值 7 → 生效 7(不回退)
        let e = reg.restore_binding(
            manifest("system.echo"),
            "system.echo@0.1.0",
            2,
            BindingStatus::Active,
        );
        assert_eq!(e, 7);
 // 之后再热替换 → 8(单调)
        let e = reg
            .switch_binding("system.echo", "system.echo@0.2.0", Arc::new(Echo))
            .unwrap();
        assert_eq!(e, 8);
    }

 /// ADR-0037:恢复携带持久状态——unavailable 墓碑不误回升为 Active。
 #[test]
    fn restore_preserves_persisted_status() {
        let mut reg = CapabilityRegistry::new();
        reg.restore_binding(
            manifest("system.echo"),
            "system.echo@0.1.0",
            3,
            BindingStatus::Unavailable,
        );
        assert_eq!(
            reg.binding_of("system.echo").unwrap().status,
            BindingStatus::Unavailable
        );
    }

 /// ADR-0037:排空生命周期 Active→Draining→finish_drain 可达;排空期不可用;
 /// 迁移非法性(非 Active 不得进 Draining,非 Draining 不得完成)。
 #[test]
    fn drain_lifecycle_is_reachable_and_guarded() {
        let mut reg = CapabilityRegistry::new();
        reg.register(manifest("system.echo"), "system.echo@0.1.0", Arc::new(Echo))
            .unwrap();
        assert!(reg.is_available("system.echo"));

        reg.begin_drain("system.echo").unwrap();
        assert_eq!(
            reg.binding_of("system.echo").unwrap().status,
            BindingStatus::Draining
        );
        assert!(!reg.is_available("system.echo"), "排空期不再可用");
 // Draining 不得再次 begin_drain(迁移非法)
        assert_eq!(
            reg.begin_drain("system.echo"),
            Err(RegistryError::InvalidTransition)
        );

        reg.finish_drain("system.echo").unwrap();
        assert_eq!(
            reg.binding_of("system.echo").unwrap().status,
            BindingStatus::Unavailable
        );
 // 非 Draining 不得 finish_drain
        assert_eq!(
            reg.finish_drain("system.echo"),
            Err(RegistryError::InvalidTransition)
        );
 // 未知能力
        assert_eq!(
            reg.begin_drain("system.nope"),
            Err(RegistryError::UnknownCapability)
        );
    }

 /// ADR-0036:执行分道以 manifest.execution_mode 声明为唯一真源;
 /// 未声明才回退 provider 命名约定。声明可覆盖约定(双向)。
 #[test]
    fn execution_mode_declaration_is_source_of_truth() {
        let mut reg = CapabilityRegistry::new();
 // 声明 async 但 provider 名不符约定 -> 仍进异步(声明优先)
        let async_declared: CapabilityManifest = serde_json::from_value(serde_json::json!({
            "capability": "custom.slow", "provider": "custom.slow", "version": "0.1.0",
            "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
            "effect": "read-only", "idempotent": true, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required", "execution_mode": "async"
        }))
        .unwrap();
        reg.register(async_declared, "custom.slow@0.1.0", Arc::new(Echo))
            .unwrap();
        assert!(
            reg.is_async("custom.slow"),
            "显式 async 声明必须进异步分道,与 provider 名无关"
        );

 // 声明 sync 但 provider 名符合 async 约定 -> 不进异步(声明优先)
        let sync_declared: CapabilityManifest = serde_json::from_value(serde_json::json!({
            "capability": "mcp.srv.fast", "provider": "mcp.srv", "version": "0.1.0",
            "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
            "effect": "read-only", "idempotent": true, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required", "execution_mode": "sync"
        }))
        .unwrap();
        reg.register(sync_declared, "mcp.srv@0.1.0", Arc::new(Echo))
            .unwrap();
        reg.mark_async_for("mcp.srv.fast", "mcp.srv");
        assert!(
            !reg.is_async("mcp.srv.fast"),
            "显式 sync 声明必须压过 mcp.* 命名约定"
        );

 // 未声明 -> 回退命名约定
        let undeclared: CapabilityManifest = serde_json::from_value(serde_json::json!({
            "capability": "mcp.legacy.tool", "provider": "mcp.legacy", "version": "0.1.0",
            "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
            "effect": "read-only", "idempotent": true, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required"
        }))
        .unwrap();
        reg.register(undeclared, "mcp.legacy@0.1.0", Arc::new(Echo))
            .unwrap();
        reg.mark_async_for("mcp.legacy.tool", "mcp.legacy");
        assert!(reg.is_async("mcp.legacy.tool"), "未声明回退 mcp.* 约定");
    }

 #[test]
    fn runtime_cache_is_lossy_but_logical_directory_survives() {
        let mut reg = CapabilityRegistry::new();
        reg.register(manifest("system.echo"), "system.echo@0.1.0", Arc::new(Echo))
            .unwrap();

        reg.clear_runtime_cache();
 // 可丢失性:缓存清空后逻辑目录与 epoch 完整,可用性降为 false
        let b = reg.binding_of("system.echo").unwrap().clone();
        assert_eq!(b.epoch, 1);
        assert_eq!(b.status, BindingStatus::Active);
        assert_eq!(
            reg.manifest_of("system.echo").unwrap().capability,
            "system.echo"
        );
        assert!(!reg.is_available("system.echo"));
        let d = &reg.discover()[0];
        assert_eq!(d.binding_epoch, 1);
        assert!(!d.healthy);

 // 缓存重建(重新 attach)后行为一致,epoch 不变
        reg.attach_handle("system.echo", Arc::new(Echo)).unwrap();
        assert!(reg.is_available("system.echo"));
        assert_eq!(reg.binding_of("system.echo").unwrap().epoch, 1);

 // attach 到未知 capability 拒绝
        assert_eq!(
            reg.attach_handle("system.nope", Arc::new(Echo)),
            Err(RegistryError::UnknownCapability)
        );
    }
}
