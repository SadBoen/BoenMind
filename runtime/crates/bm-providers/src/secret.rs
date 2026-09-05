//! Secret Store 三个实现:
//! - `MemSecretStore`:测试/回放专用,进程内存。
//! - `KeyringSecretStore`:OS keychain(Windows Credential Manager / macOS
//!   Keychain / libsecret),生产默认(基线 4.6)。
//! - `FileSecretStore`:AES-256-GCM 加密文件兜底(keychain 不可用时)。
//!
//! 三者都实现 `expose_for_scan`(INV-5 扫描面):返回本进程经手的凭据明文。

use bm_core::ports::{SecretError, SecretStore};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// 经手登记:所有实现共享的扫描面簿记。
#[derive(Default)]
struct ScanLedger {
    values: std::collections::BTreeSet<String>,
}

impl ScanLedger {
    fn note(&mut self, value: &str) {
        if value.len() >= 6 {
            self.values.insert(value.to_string());
        }
    }
    fn expose(&self) -> Vec<String> {
        self.values.iter().cloned().collect()
    }
}

// ---- MemSecretStore -------------------------------------------------------

/// A-13(审计台账):合同字符集校验(`connector::validate_secret_ref`)。
/// 只在写路径(put)fail-fast——坏引用当场拒绝,而不是静默入库、取用时
/// 才 NotFound。读/删路径保持宽松(查无即 NotFound)。
fn ensure_valid_ref(secret_ref: &str) -> Result<(), SecretError> {
    bm_contract::connector::validate_secret_ref(secret_ref).map_err(SecretError::InvalidRef)
}

/// 密钥库原子写(同 bm_persist::atomic_write;本 crate 不依赖 persist,
/// 故本地同款):临时文件 + flush + fsync + rename,断电不留半截密钥库。
// 2026-09-05 回看收归:与 bm_persist::util 同款实现,单点维护
fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), SecretError> {
    bm_persist::atomic_write(path, bytes).map_err(|e| SecretError::Backend(e.to_string()))
}

#[derive(Default)]
pub struct MemSecretStore {
    map: Mutex<BTreeMap<String, String>>,
    ledger: Mutex<ScanLedger>,
}

impl MemSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(ref_: &str, value: &str) -> Self {
        let s = Self::new();
        s.put(ref_, value).expect("内存存储写入不会失败");
        s
    }
}

impl SecretStore for MemSecretStore {
    fn get(&self, secret_ref: &str) -> Result<String, SecretError> {
        let map = self.map.lock().expect("锁未中毒");
        let v = map
            .get(secret_ref)
            .cloned()
            .ok_or_else(|| SecretError::NotFound(secret_ref.to_string()))?;
        drop(map);
        self.ledger.lock().expect("锁未中毒").note(&v);
        Ok(v)
    }

    fn put(&self, secret_ref: &str, value: &str) -> Result<(), SecretError> {
        ensure_valid_ref(secret_ref)?;
        self.map
            .lock()
            .expect("锁未中毒")
            .insert(secret_ref.to_string(), value.to_string());
        self.ledger.lock().expect("锁未中毒").note(value);
        Ok(())
    }

    fn delete(&self, secret_ref: &str) -> Result<(), SecretError> {
        self.map.lock().expect("锁未中毒").remove(secret_ref);
        Ok(())
    }

    fn expose_for_scan(&self) -> Vec<String> {
        // 测试存储:全量(即便未被 get 过)
        let map = self.map.lock().expect("锁未中毒");
        let mut all: Vec<String> = map.values().cloned().collect();
        all.extend(self.ledger.lock().expect("锁未中毒").expose());
        all
    }
}

// ---- KeyringSecretStore ---------------------------------------------------

/// OS keychain。service 固定,用户名为 secret_ref。
pub struct KeyringSecretStore {
    service: String,
    ledger: Mutex<ScanLedger>,
}

impl KeyringSecretStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            ledger: Mutex::new(ScanLedger::default()),
        }
    }

    fn entry(&self, secret_ref: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&self.service, secret_ref)
            .map_err(|e| SecretError::Backend(e.to_string()))
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, secret_ref: &str) -> Result<String, SecretError> {
        let v = self
            .entry(secret_ref)?
            .get_password()
            .map_err(|e| match e {
                keyring::Error::NoEntry => SecretError::NotFound(secret_ref.to_string()),
                other => SecretError::Backend(other.to_string()),
            })?;
        self.ledger.lock().expect("锁未中毒").note(&v);
        Ok(v)
    }

    fn put(&self, secret_ref: &str, value: &str) -> Result<(), SecretError> {
        ensure_valid_ref(secret_ref)?;
        // keyring v3:set_password 即 upsert(存在则更新)。
        self.entry(secret_ref)?
            .set_password(value)
            .map_err(|e| SecretError::Backend(e.to_string()))?;
        self.ledger.lock().expect("锁未中毒").note(value);
        Ok(())
    }

    fn delete(&self, secret_ref: &str) -> Result<(), SecretError> {
        match self.entry(secret_ref)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(other) => Err(SecretError::Backend(other.to_string())),
        }
    }

    fn expose_for_scan(&self) -> Vec<String> {
        self.ledger.lock().expect("锁未中毒").expose()
    }
}

// ---- FileSecretStore ------------------------------------------------------

/// AES-256-GCM 加密文件兜底。主密钥由组装方显式注入(生产中来自
/// 环境变量/系统配置,至少 32 字节)——绝不落明文(基线 4.6:加密文件兜底,
/// 不是明文兜底)。
pub struct FileSecretStore {
    path: PathBuf,
    key: [u8; 32],
    ledger: Mutex<ScanLedger>,
}

impl FileSecretStore {
    /// 打开(不存在则创建空库)。密钥材料不足 32 字节即报错。
    pub fn open(path: PathBuf, key_material: &str) -> Result<Self, SecretError> {
        if key_material.len() < 32 {
            return Err(SecretError::Backend("文件库主密钥不足 32 字节".into()));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_material.as_bytes()[..32]);
        if !path.exists() {
            // 建库即写加密的空映射,保证库内无明文
            let empty =
                serde_json::to_vec(&BTreeMap::<String, String>::new()).expect("空映射可序列化");
            let enc = crypto::encrypt(&key, &empty)?;
            atomic_write(&path, &enc)?;
        }
        Ok(Self {
            path,
            key,
            ledger: Mutex::new(ScanLedger::default()),
        })
    }

    fn read_all(&self) -> Result<BTreeMap<String, String>, SecretError> {
        let data = std::fs::read(&self.path).map_err(|e| SecretError::Backend(e.to_string()))?;
        if data.is_empty() {
            return Ok(BTreeMap::new());
        }
        let plain = crate::secret::crypto::decrypt(&self.key, &data)?;
        serde_json::from_slice(&plain).map_err(|e| SecretError::Backend(e.to_string()))
    }

    fn write_all(&self, map: &BTreeMap<String, String>) -> Result<(), SecretError> {
        let plain = serde_json::to_vec(map).map_err(|e| SecretError::Backend(e.to_string()))?;
        let enc = crate::secret::crypto::encrypt(&self.key, &plain)?;
        atomic_write(&self.path, &enc)
    }
}

impl SecretStore for FileSecretStore {
    fn get(&self, secret_ref: &str) -> Result<String, SecretError> {
        let v = self
            .read_all()?
            .remove(secret_ref)
            .ok_or_else(|| SecretError::NotFound(secret_ref.to_string()))?;
        self.ledger.lock().expect("锁未中毒").note(&v);
        Ok(v)
    }

    fn put(&self, secret_ref: &str, value: &str) -> Result<(), SecretError> {
        ensure_valid_ref(secret_ref)?;
        let mut map = self.read_all()?;
        map.insert(secret_ref.to_string(), value.to_string());
        self.write_all(&map)?;
        self.ledger.lock().expect("锁未中毒").note(value);
        Ok(())
    }

    fn delete(&self, secret_ref: &str) -> Result<(), SecretError> {
        let mut map = self.read_all()?;
        map.remove(secret_ref);
        self.write_all(&map)
    }

    fn expose_for_scan(&self) -> Vec<String> {
        self.ledger.lock().expect("锁未中毒").expose()
    }
}

pub(crate) mod crypto {
    use bm_core::ports::SecretError;

    /// AES-256-GCM:12 字节随机 nonce 前置,密文 = nonce || ciphertext||tag。
    pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, SecretError> {
        use aes_gcm::Aes256Gcm;
        use aes_gcm::aead::{Aead, KeyInit};
        let cipher = Aes256Gcm::new(key.into());
        let mut nonce_bytes = [0u8; 12];
        getrandom::fill(&mut nonce_bytes)
            .map_err(|e| SecretError::Backend(format!("随机数不可用: {e}")))?;
        #[allow(deprecated)]
        let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| SecretError::Backend(format!("加密失败: {e}")))?;
        let mut out = nonce_bytes.to_vec();
        out.extend_from_slice(&ct);
        Ok(out)
    }

    pub fn decrypt(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, SecretError> {
        use aes_gcm::Aes256Gcm;
        use aes_gcm::aead::{Aead, KeyInit};
        if blob.len() < 12 {
            return Err(SecretError::Backend("密文库损坏(过短)".into()));
        }
        let cipher = Aes256Gcm::new(key.into());
        let (nonce, ct) = blob.split_at(12);
        #[allow(deprecated)]
        let nonce = aes_gcm::Nonce::from_slice(nonce);
        cipher
            .decrypt(nonce, ct)
            .map_err(|_| SecretError::Backend("密文库解密失败".into()))
    }
}

// A-13(审计台账 2026-08-31)验收:写路径 fail-fast——合同字符集外的
// secret_ref 拒绝入库(含超长 body、非法字符、缺前缀);合法引用不受影响。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_rejects_invalid_secret_ref_charset() {
        let s = MemSecretStore::new();
        for bad in [
            "model.x",
            "secret:",
            "secret:model/zhipu",
            "secret:has space",
            "secret:大写外字符",
        ] {
            let err = SecretStore::put(&s, bad, "v").expect_err(&format!("坏引用应被拒绝: {bad}"));
            assert!(
                matches!(err, SecretError::InvalidRef(_)),
                "应为 InvalidRef: {bad}"
            );
            assert!(SecretStore::get(&s, bad).is_err());
        }
        // 合法引用(真实模型映射形状)不受影响
        let good = "secret:model.gpt-5.6-luna";
        SecretStore::put(&s, good, "sk-demo").expect("合法引用应入库");
        assert_eq!(SecretStore::get(&s, good).unwrap(), "sk-demo");
    }
}
