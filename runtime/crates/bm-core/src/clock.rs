//! 时钟端口:墙钟一律经 Clock 注入(基线 8.3:排序以 seq 为准,时间戳仅参考)。
//! 测试用 MockClock 保证回放确定性。

use chrono::{DateTime, Utc};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        SystemTime::now().into()
    }
}

/// 可手动推进的确定性时钟(毫秒精度)。
#[derive(Default)]
pub struct MockClock {
    current_ms: Mutex<u128>,
}

impl MockClock {
    pub fn at_ms(ms: u128) -> Self {
        Self {
            current_ms: Mutex::new(ms),
        }
    }

    pub fn advance_ms(&self, ms: u128) {
        *self.current_ms.lock().expect("锁未中毒") += ms;
    }
}

impl Clock for MockClock {
    fn now(&self) -> DateTime<Utc> {
        let ms = *self.current_ms.lock().expect("锁未中毒");
        let st = UNIX_EPOCH + std::time::Duration::from_millis(ms as u64);
        DateTime::<Utc>::from(st)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_clock_advances() {
        let c = MockClock::at_ms(1_787_952_928_359);
        let t0 = c.now();
        c.advance_ms(1873);
        assert_eq!((c.now() - t0).num_milliseconds(), 1873);
    }
}
