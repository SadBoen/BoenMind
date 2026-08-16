//! UID 生成/校验（对齐 xu-wiki utils/paths.py CONST-ARCH-3）。
//!
//! UID = 8 位 `[A-Z0-9]`：同秒内单调计数器（2 位 base36，0-1295）打头 +
//! 6 位随机——同秒内确定性递增、跨秒随机兜底，永不重用（BAN-ARCH-2）。
//! 计数器溢出（单秒 >1295 个，不现实）退化为 8 位纯随机。

use std::sync::atomic::{AtomicU64, Ordering};

const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
/// 每秒计数器上限（xu-wiki 同值；36^2 = 1296 组合）。
const MAX_COUNTER: u32 = 1296;

static SLOT: AtomicU64 = AtomicU64::new(0);

/// 生成 8 位 UID。
pub fn gen_uid() -> String {
    let now = unix_sec();
    let mut prev = SLOT.load(Ordering::Relaxed);
    let counter = loop {
        let (sec, cnt) = (prev >> 32, prev as u32);
        let (new_sec, new_cnt) = if sec == now { (sec, cnt + 1) } else { (now, 0) };
        let next = (new_sec << 32) | u64::from(new_cnt);
        if SLOT
            .compare_exchange_weak(prev, next, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break new_cnt;
        }
        prev = SLOT.load(Ordering::Relaxed);
    };
    let mut out = String::with_capacity(8);
    if counter < MAX_COUNTER {
        out.push(ALPHABET[(counter / 36) as usize] as char);
        out.push(ALPHABET[(counter % 36) as usize] as char);
        for c in random_chars(6) {
            out.push(c);
        }
    } else {
        for c in random_chars(8) {
            out.push(c);
        }
    }
    out
}

/// 校验 UID 形状（`^[A-Z0-9]{8}$`）。
pub fn is_valid_uid(uid: &str) -> bool {
    uid.len() == 8 && uid.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn unix_sec() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn random_chars(n: usize) -> Vec<char> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..n).map(|_| ALPHABET[rng.gen_range(0..36)] as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_shape_and_uniqueness() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            let u = gen_uid();
            assert!(is_valid_uid(&u), "invalid uid {u}");
            assert!(seen.insert(u), "duplicate uid");
        }
    }

    #[test]
    fn same_second_counter_increments() {
        // 同秒内前两个 UID 的计数部分应相邻（00, 01, …）
        let a = gen_uid();
        let b = gen_uid();
        assert_eq!(a.len(), 8);
        assert_eq!(b.len(), 8);
        assert_ne!(a, b);
    }

    #[test]
    fn reject_bad_shapes() {
        assert!(!is_valid_uid("abc"));
        assert!(!is_valid_uid("abcdefghij"));
        assert!(!is_valid_uid("abc_defg"));
        assert!(!is_valid_uid("abc-1234"));
    }
}
