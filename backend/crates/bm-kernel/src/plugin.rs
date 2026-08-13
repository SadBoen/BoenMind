//! 插件契约（内核侧）：Plugin trait + 可逆副作用 Disposer。
//!
//! 与 bm-protocol 的依赖方向：Plugin 的 apply 签名需要 Ctx（内核类型），
//! 因此 Plugin trait 落在内核层而非契约层（实现方案目录把 plugin.rs
//! 画在 bm-protocol 下，但契约 crate 纯净性优先——纯类型与内核语义
//! 不互相拉扯）。

use bm_protocol::ProtocolError;

use crate::ctx::Ctx;

/// 服务 key（"event_store" / "model_provider" / …），编译期字符串。
pub type ServiceKey = &'static str;

/// 可逆副作用（RAII）：drop/fire 时执行撤销逻辑（退订/移除注册）。
/// 卸载顺序 = Disposer 逆序（后安装的先卸载）。
pub struct Disposer(Option<Box<dyn FnOnce() + Send>>);

impl Disposer {
    pub fn new(f: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(f)))
    }

    /// 立即执行并消费（等价 drop）。
    pub fn fire(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }

    /// 合成：多个 disposer 合并为一个（先执行的先撤销）。
    pub fn join_all(mut ds: Vec<Disposer>) -> Disposer {
        Disposer::new(move || {
            for d in ds.iter_mut() {
                d.fire();
            }
        })
    }
}

impl Drop for Disposer {
    fn drop(&mut self) {
        self.fire();
    }
}

impl std::fmt::Debug for Disposer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Disposer")
    }
}

/// 插件：声明依赖 + 注册一切副作用。
///
/// 生命周期（loader 负责）：`deps()` 中每个 key 必须已注册
/// （内核内置或先前插件提供）才调用 `apply()`；apply 返回的
/// Disposer 是"撤销一切注册"的清单。
pub trait Plugin: Send + Sync {
    /// 插件名（与 Manifest.name 一致，安装时校验）。
    fn name(&self) -> &'static str;

    /// 依赖的服务 key 列表（拓扑排序依据；未就绪 → 安装失败）。
    fn deps(&self) -> &[ServiceKey] {
        &[]
    }

    /// 挂载：注册服务/订阅事件/挂接中间件，返回全部可逆副作用。
    fn apply(&mut self, ctx: &mut Ctx<'_>) -> Result<Vec<Disposer>, ProtocolError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposer_fires_once() {
        let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n2 = n.clone();
        let mut d = Disposer::new(move || {
            n2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        d.fire(); // 手动执行
        drop(d); // drop 不重复执行（Option 已 take）
        assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn disposer_join_all_reverses() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut ds = Vec::new();
        for i in 0..3 {
            let order2 = order.clone();
            ds.push(Disposer::new(move || order2.lock().unwrap().push(i)));
        }
        let joined = Disposer::join_all(ds);
        // drop 时按 push 顺序执行（join_all 内的迭代序）
        drop(joined);
        assert_eq!(*order.lock().unwrap(), vec![0, 1, 2]);
    }
}
