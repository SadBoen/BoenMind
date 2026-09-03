//! 配置:allowed_roots 白名单 + 限额。文件缺失 = 空根(工具一律报错提示配置,
//! 不做静默 cwd 兜底——权限显式化,ADR-0006)。UI 侧 allowed_roots 以分号分隔
//! 字符串下发(config_schema 只有 string/secret/range/select),此处两种形态都收。

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Config {
    pub allowed_roots: Vec<String>,
    pub max_results: usize,
    pub max_output_chars: usize,
    pub max_file_bytes: u64,
}

#[derive(Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    allowed_roots: RootField,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    max_output_chars: Option<usize>,
    #[serde(default)]
    max_file_bytes: Option<u64>,
}

/// allowed_roots 兼容数组与分号分隔字符串两种形态
#[derive(Deserialize, Default)]
#[serde(untagged)]
enum RootField {
    #[default]
    Empty,
    List(Vec<String>),
    Joined(String),
}

impl Config {
    pub fn load(path: Option<&std::path::Path>) -> Self {
        let file: ConfigFile = path
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str(raw.trim()).ok())
            .unwrap_or_default();
        let roots = match file.allowed_roots {
            RootField::Empty => Vec::new(),
            RootField::List(v) => v,
            RootField::Joined(s) => s
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
        };
        Self {
            allowed_roots: roots,
            max_results: file.max_results.unwrap_or(80).clamp(1, 500),
            max_output_chars: file.max_output_chars.unwrap_or(16_000).clamp(1_000, 65_536),
            max_file_bytes: file
                .max_file_bytes
                .unwrap_or(1_048_576)
                .clamp(256, 33_554_432),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_empty_roots_and_defaults() {
        let cfg = Config::load(None);
        assert!(cfg.allowed_roots.is_empty());
        assert_eq!(cfg.max_results, 80);
        assert_eq!(cfg.max_output_chars, 16_000);
    }

    #[test]
    fn roots_as_joined_string_and_list_both_parse() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("c.json");
        std::fs::write(
            &p,
            r#"{"allowed_roots":"D:\\a ; D:\\b","max_results":3000}"#,
        )
        .expect("write");
        let cfg = Config::load(Some(&p));
        assert_eq!(cfg.allowed_roots, vec!["D:\\a", "D:\\b"]);
        assert_eq!(cfg.max_results, 500, "超上限钳制");

        std::fs::write(&p, r#"{"allowed_roots":["D:\\x"]}"#).expect("write");
        let cfg = Config::load(Some(&p));
        assert_eq!(cfg.allowed_roots, vec!["D:\\x"]);
    }
}
