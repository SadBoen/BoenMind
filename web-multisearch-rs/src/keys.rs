//! 逗号分隔多 Key 轮换 — 与 Python 版 providers/keys.py 等价移植。
//!
//! Key 可写多个(`key-a,key-b`);搜索遇到 401/403/429 自动轮换下一个,
//! 其余错误立即中止;全部耗尽返回最后一个错误摘要。生产端错误摘要只含
//! 状态码与源名,绝不回显 key 本体(见 sources.rs 各源消息)。

/// 触发换下一个 key 的 HTTP 状态码。
pub const ROTATABLE_STATUSES: [u16; 3] = [401, 403, 429];

/// 带状态码的请求错误;轮换逻辑只看 `Status`。
#[derive(Debug)]
pub enum HttpErr {
    Status(u16, String),
    Other(String),
}

/// 把逗号分隔的 key 串拆成去空白、去空的候选列表(保序)。
pub fn split_keys(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(String::from)
        .collect()
}

/// 按候选顺序执行 `attempt(key)`,401/403/429 轮换下一个,其余错误立即中止。
/// 全部耗尽时返回最后一个错误摘要(由调用方包装,不回显 key)。
pub async fn with_key_rotation<T, F, Fut>(
    candidates: &[String],
    mut attempt: F,
) -> Result<T, String>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<T, HttpErr>>,
{
    let mut last_err: Option<String> = None;
    for key in candidates {
        match attempt(key.clone()).await {
            Ok(v) => return Ok(v),
            Err(HttpErr::Status(status, msg)) => {
                if ROTATABLE_STATUSES.contains(&status) {
                    last_err = Some(msg);
                    continue;
                }
                return Err(msg);
            }
            Err(HttpErr::Other(msg)) => return Err(msg),
        }
    }
    Err(last_err.unwrap_or_else(|| "no key candidates".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn split_keys_trims_and_skips_empty() {
        assert_eq!(
            split_keys(" k1 , ,k2,, "),
            vec!["k1".to_string(), "k2".to_string()]
        );
        assert!(split_keys("  ").is_empty());
    }

    #[tokio::test]
    async fn rotation_advances_on_rotatable_status() {
        let tried = Arc::new(Mutex::new(Vec::new()));
        let t = tried.clone();
        let result = with_key_rotation(&["a".to_string(), "b".to_string()], move |k| {
            let t = t.clone();
            async move {
                t.lock().unwrap().push(k.clone());
                if k == "a" {
                    Err(HttpErr::Status(429, "first 429".into()))
                } else {
                    Ok("ok".to_string())
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), "ok");
        assert_eq!(*tried.lock().unwrap(), ["a", "b"]);
    }

    #[tokio::test]
    async fn rotation_aborts_immediately_on_non_rotatable() {
        let tried = Arc::new(Mutex::new(Vec::new()));
        let t = tried.clone();
        let result: Result<(), String> =
            with_key_rotation(&["a".to_string(), "b".to_string()], move |k| {
                let t = t.clone();
                async move {
                    t.lock().unwrap().push(k);
                    Err::<(), _>(HttpErr::Status(500, "boom".into()))
                }
            })
            .await;
        assert_eq!(result.unwrap_err(), "boom");
        // 500 非轮换码:第一个 key 失败即中止,第二个不被尝试
        assert_eq!(*tried.lock().unwrap(), ["a"]);
    }

    #[tokio::test]
    async fn rotation_exhausted_returns_last_summary() {
        // 两个候选都 401 → 返回最后一个错误摘要(源名+状态码,不含 key)
        let result: Result<(), String> =
            with_key_rotation(&["a".to_string(), "b".to_string()], |k| async move {
                Err::<(), _>(HttpErr::Status(
                    401,
                    format!("source returned HTTP 401 (候选 {k} 已跳过)"),
                ))
            })
            .await;
        let err = result.unwrap_err();
        assert!(err.contains("401"));
        // 全部耗尽 → 错误来自最后一个候选
        assert!(err.contains('b'));
    }

    #[tokio::test]
    async fn empty_candidates_yields_placeholder_error() {
        let result: Result<(), String> =
            with_key_rotation(&[], |_: String| async { Ok::<(), HttpErr>(()) }).await;
        assert_eq!(result.unwrap_err(), "no key candidates");
    }
}
