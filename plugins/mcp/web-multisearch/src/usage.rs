//! 月度用量记账 — 记录每家供应商本月已调用次数,并可选按配额上限显示进度。
//!
//! 存储:`<config 同级目录>/mcp-web_multisearch-usage.json`(与 mcp-*.json
//! 同目录),扁平结构:
//! ```json
//! { "month": "2026-09", "by_provider": { "serper": 3, "jina": 12 } }
//! ```
//! 跨自然月自动重置:读取时若 month 与当前月不符,则视为 0 并写回新月份。
//! 只记成功调用(真正让该家处理了一次搜索,失败/授权错误不计),避免消费
//! 误报。文件损坏时按空计数恢复,不阻断搜索。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// 当前自然月 "YYYY-MM"(本地时区)。
fn current_month() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 转本地时间做月份(2026-09 语义)。这里用固定 UTC 参考近似;应用场景为
    // 个人本机,吃容忍时区差异(最多差几小时跨月边界,可接受)。
    let days = secs / 86400;
    let (y, m) = {
        // 简略自算:从 1970-01-01 起
        let mut y = 1970i64;
        let mut rem = days as i64;
        loop {
            let dy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                366
            } else {
                365
            };
            if rem < dy {
                break;
            }
            rem -= dy;
            y += 1;
        }
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let mdays = [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let mut m = 0i64;
        for (i, d) in mdays.iter().enumerate() {
            if rem < *d {
                m = (i as i64) + 1;
                break;
            }
            rem -= *d;
        }
        (y, m)
    };
    format!("{y:04}-{m:02}")
}

/// 月度用量账本。读时自动处理跨月重置,写时原子写。
pub struct UsageLedger {
    path: PathBuf,
    /// month → (by_provider map)
    month: String,
    by_provider: serde_json::Map<String, Value>,
}

impl UsageLedger {
    /// 从 config 文件路径推导 usage 文件路径:<config>.usage.json
    pub fn from_config_path(config_path: Option<&Path>) -> Self {
        let path = match config_path {
            Some(p) => p.with_extension("usage.json"),
            None => PathBuf::from("usage.json"),
        };
        let mut ledger = UsageLedger {
            path,
            month: current_month(),
            by_provider: serde_json::Map::new(),
        };
        ledger.load();
        ledger
    }

    fn load(&mut self) {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        let root: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let stored_month = root
            .get("month")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if stored_month == self.month.as_str() {
            if let Some(m) = root.get("by_provider").and_then(Value::as_object) {
                self.by_provider = m.clone();
            }
        } else {
            // 跨月:归零(旧月份在文件里被覆盖)
            self.by_provider = serde_json::Map::new();
        }
    }

    fn save(&self) {
        let root = json!({
            "month": self.month,
            "by_provider": self.by_provider,
        });
        if let Some(text) = serde_json::to_string(&root).ok() {
            let _ = atomic_write(&self.path, text.as_bytes());
        }
    }

    /// 记录一次成功调用(某家 provider 真正处理了请求)。
    pub fn record(&mut self, provider_id: &str) {
        let cur = self
            .by_provider
            .get(provider_id)
            .and_then(Value::as_u64)
            .unwrap_or(0);
        self.by_provider
            .insert(provider_id.to_string(), json!(cur.saturating_add(1)));
        self.save();
    }

    /// 读某家本月已用次数。
    pub fn used(&self, provider_id: &str) -> u64 {
        self.by_provider
            .get(provider_id)
            .and_then(Value::as_u64)
            .unwrap_or(0)
    }

    /// 当前记账月份。
    pub fn month(&self) -> &str {
        &self.month
    }
}

/// 简易原子写(同名文件旁写 .tmp 后 rename)。
fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_format_is_yyyy_mm() {
        let m = current_month();
        assert_eq!(m.len(), 7);
        assert_eq!(&m[4..5], "-");
        assert!(m.as_bytes()[0].is_ascii_digit());
    }

    #[test]
    fn record_and_read_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = dir.path().join("mcp-web_multisearch.json");
        let mut led = UsageLedger::from_config_path(Some(&cfg));
        led.record("serper");
        led.record("serper");
        led.record("jina");
        assert_eq!(led.used("serper"), 2);
        assert_eq!(led.used("jina"), 1);
        assert_eq!(led.used("exa"), 0);
        // 重新加载(模拟重启)仍能读到
        let led2 = UsageLedger::from_config_path(Some(&cfg));
        assert_eq!(led2.used("serper"), 2);
        assert_eq!(led2.used("jina"), 1);
    }

    #[test]
    fn corrupt_file_falls_back_to_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = dir.path().join("mcp.json");
        std::fs::write(dir.path().join("mcp.usage.json"), "not json").unwrap();
        let led = UsageLedger::from_config_path(Some(&cfg));
        assert_eq!(led.used("serper"), 0);
    }
}
