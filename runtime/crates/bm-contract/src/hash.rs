//! 通用哈希工具(2026-09-05 回看收归):sha256→hex 转换此前在
//! bm-core/bm-persist/bm-providers/bm-surface-http 各自复制十余处,
//! 统一单点提供,消费方一律走本模块。

use sha2::{Digest, Sha256};

/// 字节序列小写十六进制编码。
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 摘要的十六进制文本。
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex(&h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hex(&[]), "");
    }
}
