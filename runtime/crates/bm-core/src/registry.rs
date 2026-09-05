//! Capability Registry(基线 §6.2/§6.4,M4.1):「谁提供什么」的统一注册中心。
//!
//! 两层结构(基线 §6.4):
//! - 持久逻辑目录:manifest + binding 元数据(instance_id/epoch/status),
//!   重启后由持久层恢复(T3 接 SQLite capabilities 表;`restore_binding`
//!   是恢复入口,epoch 不回退);
//! - 可丢失运行时缓存:Provider 实例句柄 + 健康位,重启重建(`clear_runtime_cache`
//!   演示可丢失性:清空后行为不变,重新 attach 即恢复)。
//!
//! binding_epoch 在每次 binding 生命周期事件(首次注册/热替换/恢复)时单调 +1,
//! 是授权-执行-审计三方一致性的根基(ADR-0001 条件 2);重启恢复不得使 epoch
//! 回退(Runtime generation 变更不改变已签发在途调用的归属)。
//!
//! 注册面只回答「谁提供什么」;能不能调用是 Broker 的裁决(基线 §7)——
//! 本模块不持有任何策略。

use bm_contract::capability::{CapabilityManifest, MutationClass};
use std::collections::HashMap;
use std::sync::Arc;

/// Provider 执行端口(M4 = 内置 Rust 实现;独立进程形态随 M7,调用方无感,
/// 基线 §7)。args 已由 Broker 过 manifest input_schema;返回值由 Broker
/// 过 output_schema(M4.3)。
///
/// 分层关系(与 [`crate::ports::AsyncCapabilityExecutor`] 的分工,2026-09-02
/// 审计轮注释):两者共用同一条 Broker 决策管线(身份/凭据/预扣/intent 门),
/// 仅执行步分道——`is_async()` 为真(外部慢路径,如 MCP)走异步执行器
/// (运行期 spawn + manifest.timeout_ms 钳制超时 + 取消令牌 + 进度回流),
/// 否则在本任务内联同步执行(panic 收容)。选型约束:同步实现不得长时间
/// 阻塞——会占住单写者循环,耗时能力一律注册为异步。
pub trait CapabilityProvider: Send + Sync {
    fn invoke(&self, args: serde_json::Value) -> Result<serde_json::Value, String>;
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
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::AlreadyRegistered => write!(f, "capability 已注册"),
            RegistryError::UnknownCapability => write!(f, "未知 capability"),
            RegistryError::InvalidTransition => write!(f, "非法 binding 状态迁移"),
        }
    }
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

    /// 首次注册:manifest 进逻辑目录,binding 建立并分配 epoch=1。
    pub fn register(
        &mut self,
        manifest: CapabilityManifest,
        provider_instance_id: &str,
        handle: Arc<dyn CapabilityProvider>,
    ) -> Result<u64, RegistryError> {
        let name = manifest.capability.clone();
        if self.manifests.contains_key(&name) {
            return Err(RegistryError::AlreadyRegistered);
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

    /// 重启恢复入口(T3 由 SQLite capabilities 表驱动):以持久值恢复逻辑
    /// 目录;epoch 取 max(现值, 持久值)——不回退(ADR-0001 条件 2)。
    /// 返回生效 epoch。
    pub fn restore_binding(
        &mut self,
        manifest: CapabilityManifest,
        provider_instance_id: &str,
        epoch: u64,
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
                status: BindingStatus::Active,
            },
        );
        // 运行时缓存不在恢复范围:句柄由注册流程重新 attach(可丢失语义)。
        self.cache.remove(&name);
        effective
    }

    /// W4 对话工具闭环:枚举全部「免审批直通」能力(只读类)。
    /// 返回 (capability 名, input_schema);调用方据此构造模型侧 tools。
    pub fn direct_tools(&self) -> Vec<(String, serde_json::Value)> {
        let mut out: Vec<(String, serde_json::Value)> = self
            .manifests
            .iter()
            .filter(|(_, m)| {
                m.approval == bm_contract::capability::ApprovalRequirement::NotRequired
                    && m.capability != "model.invoke"
            })
            .map(|(name, m)| (name.clone(), m.input_schema.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// W4b 对话工具闭环:枚举供对话 Agent 使用的全部能力(含直通与需审批的业务能力)。
    /// 排除内核私有能力(如 model.invoke)。
    /// needs_approval 与 Broker 步 5 判定同口径:effect 可审批类
    /// (reversible/external/high-risk)或 manifest 声明 required → true。
    /// 第 4 元 = manifest.description(ADR-0022 合同 Minor):面向模型的
    /// 一句功能描述;fs.*/system.exec 内置能力与 MCP 工具自描述,缺省 None
    /// 由 turn 侧兜底。此前 MCP 工具描述被整层丢弃,模型只见「只读直通
    /// 工具」套话,是工具调用别扭的直接根因之一。
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
        let e = reg.restore_binding(manifest("system.echo"), "system.echo@0.1.0", 7);
        assert_eq!(e, 7);
        // 运行时已有 epoch=2,持久值 7 → 生效 7(不回退)
        let e = reg.restore_binding(manifest("system.echo"), "system.echo@0.1.0", 2);
        assert_eq!(e, 7);
        // 之后再热替换 → 8(单调)
        let e = reg
            .switch_binding("system.echo", "system.echo@0.2.0", Arc::new(Echo))
            .unwrap();
        assert_eq!(e, 8);
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
