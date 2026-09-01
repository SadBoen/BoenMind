//! 配置与凭据解析 — 对应 Python 版 server.py load_config + providers/keystore.py。
//!
//! 与 Python 版的差异(有意简化,行为不变):
//! - 不再有 override 文件/env 双层写入——BoenMind 设置页直接写
//!   `config/mcp-web_multisearch.json`(webadmin /admin/mcp-config),本进程
//!   每次取 key 前按 mtime 热读该文件:设置页改 Key 下一次搜索立即生效,
//!   不需要重启(把 webadmin「改 Key 立即生效」的承诺真正落实)。
//! - env 兜底保留同名变量(Hermes 的 ~/.hermes/.env 层不迁)。

use std::path::PathBuf;
use std::time::SystemTime;

/// 配置项 key → 环境变量兜底名(Python server.py _ENV_MAP 镜像)。
const ENV_MAP: &[(&str, &str)] = &[
    ("serper_api_key", "SERPER_API_KEY"),
    ("jina_api_key", "JINA_API_KEY"),
    ("tavily_api_key", "TAVILY_API_KEY"),
    ("exa_api_key", "EXA_API_KEY"),
    ("brave_api_key", "BRAVE_SEARCH_API_KEY"),
    ("langsearch_api_key", "LANGSEARCH_API_KEY"),
    ("linkup_api_key", "LINKUP_API_KEY"),
    ("you_api_key", "YOU_API_KEY"),
    ("websearchapi_api_key", "WEBSEARCHAPI_API_KEY"),
];

const DEFAULT_LIMIT: i64 = 5;

/// 运行期配置:--config 指向的 JSON 文件,mtime 缓存热读。
pub struct Config {
    path: Option<PathBuf>,
    cache: Option<(Option<SystemTime>, serde_json::Value)>,
}

impl Config {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path, cache: None }
    }

    /// 读原始配置值(mtime 变了才重新解析;坏损文件不阻断,返回空对象)。
    fn raw(&mut self) -> serde_json::Value {
        let Some(path) = &self.path else {
            return serde_json::Value::Null;
        };
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if let Some((cached_mtime, cached)) = &self.cache {
            if *cached_mtime == mtime {
                return cached.clone();
            }
        }
        let value = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        self.cache = Some((mtime, value.clone()));
        value
    }

    /// 字符串配置项:配置文件优先,回退同名环境变量(仅 key 类映射)。
    pub fn get_str(&mut self, key: &str) -> String {
        let from_file = self
            .raw()
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        if !from_file.is_empty() {
            return from_file;
        }
        if let Some((_, env)) = ENV_MAP.iter().find(|(k, _)| *k == key) {
            if let Ok(v) = std::env::var(env) {
                let v = v.trim().to_string();
                if !v.is_empty() {
                    return v;
                }
            }
        }
        String::new()
    }

    /// SearXNG 实例地址:配置 searxng_url → 环境变量 WMS_SEARXNG_URL。
    pub fn searxng_url(&mut self) -> String {
        let from_file = self
            .raw()
            .get("searxng_url")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        if !from_file.is_empty() {
            return from_file;
        }
        std::env::var("WMS_SEARXNG_URL")
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    }

    /// 默认返回条数:args.limit → 配置 default_limit → 5;钳制 1..=100。
    pub fn resolve_limit(&mut self, args_limit: Option<i64>) -> usize {
        let raw = args_limit.unwrap_or_else(|| {
            let v = self.raw();
            match v.get("default_limit") {
                Some(serde_json::Value::Number(n)) => n.as_i64(),
                Some(serde_json::Value::String(s)) => s.trim().parse::<i64>().ok(),
                _ => None,
            }
            .unwrap_or(DEFAULT_LIMIT)
        });
        raw.clamp(1, 100) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(content: &str) -> (tempfile::NamedTempFile, Config) {
        let f = tempfile::NamedTempFile::new().expect("临时文件");
        std::fs::write(f.path(), content).expect("写临时配置");
        let cfg = Config::new(Some(f.path().to_path_buf()));
        (f, cfg)
    }

    #[test]
    fn missing_file_yields_defaults() {
        let mut cfg = Config::new(Some(PathBuf::from("Z:/definitely/not/here.json")));
        assert_eq!(cfg.get_str("serper_api_key"), "");
        assert_eq!(cfg.resolve_limit(None), 5);
    }

    #[test]
    fn reads_keys_and_limit_with_clamp() {
        let (_f, mut cfg) = temp_config(r#"{"serper_api_key":" k1,k2 ","default_limit":"9"}"#);
        assert_eq!(cfg.get_str("serper_api_key"), "k1,k2");
        assert_eq!(cfg.resolve_limit(None), 9);
        assert_eq!(cfg.resolve_limit(Some(999)), 100);
        assert_eq!(cfg.resolve_limit(Some(0)), 1);
    }

    #[test]
    fn env_fallback_when_file_empty() {
        let (_f, mut cfg) = temp_config(r#"{}"#);
        // 测试环境未必有 SERPER_API_KEY;两种结果都算通过(键在配置里缺席 → env 兜底)
        let v = cfg.get_str("serper_api_key");
        let env = std::env::var("SERPER_API_KEY").unwrap_or_default();
        assert_eq!(v, env.trim());
    }
}
