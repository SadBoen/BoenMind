//! 服务注册表：按 key 注册/查找，重复注册拒绝，类型安全获取。

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use bm_protocol::{ErrorCode, ProtocolError};

use crate::ServiceKey;

#[derive(Default)]
pub struct Registry {
    services: RwLock<HashMap<ServiceKey, Arc<dyn Any + Send + Sync>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册服务。key 已存在 → AlreadyRegistered。
    pub fn register(
        &self,
        key: ServiceKey,
        svc: Arc<dyn Any + Send + Sync>,
    ) -> Result<(), ProtocolError> {
        let mut m = self.services.write().expect("registry poisoned");
        if m.contains_key(key) {
            return Err(ProtocolError::new(
                ErrorCode::AlreadyRegistered,
                format!("service key `{key}` already registered"),
            ));
        }
        m.insert(key, svc);
        Ok(())
    }

    /// 按 key + 类型取服务（类型不符 → InvalidArgument）。
    pub fn get<T: Send + Sync + 'static>(&self, key: ServiceKey) -> Result<Arc<T>, ProtocolError> {
        let m = self.services.read().expect("registry poisoned");
        let svc = m.get(key).ok_or_else(|| {
            ProtocolError::new(ErrorCode::NotFound, format!("service key `{key}` not found"))
        })?;
        svc.clone().downcast::<T>().map_err(|_| {
            ProtocolError::new(
                ErrorCode::InvalidArgument,
                format!("service `{key}` type mismatch"),
            )
        })
    }

    /// 服务是否已注册（loader 检查 deps 用）。
    pub fn contains(&self, key: &str) -> bool {
        self.services.read().expect("registry poisoned").contains_key(key)
    }

    /// 移除服务（Disposer 撤销注册用）。
    pub fn remove(&self, key: ServiceKey) -> bool {
        self.services.write().expect("registry poisoned").remove(key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_get_concrete_type() {
        let r = Registry::new();
        // 注：trait object（如 Arc<dyn EventStorePort>）无法进 Any 注册表，
        // 这是设计取舍——trait 服务经 Kernel::event_store 特例取用
        let store = Arc::new(crate::InMemoryEventStore::new());
        r.register("event_store", store.clone()).unwrap();
        let got = r.get::<crate::InMemoryEventStore>("event_store").unwrap();
        assert!(Arc::ptr_eq(&got, &store));
    }

    #[test]
    fn duplicate_register_rejected() {
        let r = Registry::new();
        let store = Arc::new(crate::InMemoryEventStore::new());
        r.register("event_store", store.clone()).unwrap();
        let err = r.register("event_store", store.clone()).unwrap_err();
        assert_eq!(err.code(), ErrorCode::AlreadyRegistered);
    }

    #[test]
    fn missing_key_not_found() {
        let r = Registry::new();
        let err = r.get::<String>("nope").unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
    }

    #[test]
    fn type_mismatch_invalid_argument() {
        let r = Registry::new();
        r.register("num", Arc::new(42i32)).unwrap();
        let err = r.get::<String>("num").unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidArgument);
    }

    #[test]
    fn remove_undoes_registration() {
        let r = Registry::new();
        r.register("num", Arc::new(42i32)).unwrap();
        assert!(r.remove("num"));
        assert!(!r.contains("num"));
    }
}
