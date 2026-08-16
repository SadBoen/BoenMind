//! 环境变量展开（吸收 Claude Code 语法）：`${VAR}` / `${VAR:-default}`。
//!
//! 适用范围：MCP server 配置的 command/args/env/url/headers 字符串值，
//! 让密钥等敏感项不进配置文件明文（如 `${GITHUB_TOKEN:-}`）。

use std::collections::HashMap;

/// 展开字符串中的 `${VAR}` 与 `${VAR:-default}`。
/// 未定义且无默认 → 空字符串（与 Claude Code 行为一致）。
pub fn expand_env(input: &str, env: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'{'
            && let Some(end) = input[i + 2..].find('}')
        {
            let expr = &input[i + 2..i + 2 + end];
            let (name, default) = match expr.split_once(":-") {
                Some((n, d)) => (n, Some(d)),
                None => (expr, None),
            };
            let value = env.get(name).cloned().or_else(|| default.map(str::to_string));
            out.push_str(&value.unwrap_or_default());
            i += 2 + end + 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// 展开配置中的全部字符串值（command/args/env 值/url/headers）。
/// `env` 字段自身参与展开（允许互相引用，如 `PATH=${PATH:-...}`）。
pub fn expand_config_strings(
    command: &mut Option<String>,
    args: &mut [String],
    env: &mut std::collections::BTreeMap<String, String>,
    url: &mut Option<String>,
    headers: &mut std::collections::BTreeMap<String, String>,
) {
    // 宿主环境变量 + 已声明 env 的合并视图（后者优先）
    let mut view: HashMap<String, String> = std::env::vars().collect();
    view.extend(env.iter().map(|(k, v)| (k.clone(), v.clone())));

    if let Some(c) = command {
        *c = expand_env(c, &view);
    }
    for a in args.iter_mut() {
        *a = expand_env(a, &view);
    }
    for v in env.values_mut() {
        *v = expand_env(v, &view);
    }
    if let Some(u) = url {
        *u = expand_env(u, &view);
    }
    for v in headers.values_mut() {
        *v = expand_env(v, &view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn plain_passthrough() {
        let e = env(&[]);
        assert_eq!(expand_env("node server.js", &e), "node server.js");
        assert_eq!(expand_env("no braces $X", &e), "no braces $X");
    }

    #[test]
    fn simple_substitution() {
        let e = env(&[("GITHUB_TOKEN", "abc123")]);
        assert_eq!(expand_env("Bearer ${GITHUB_TOKEN}", &e), "Bearer abc123");
    }

    #[test]
    fn default_syntax() {
        let e = env(&[]);
        assert_eq!(expand_env("${MISSING:-fallback}", &e), "fallback");
        assert_eq!(expand_env("${MISSING}", &e), "", "无默认且未定义 → 空串");
    }

    #[test]
    fn default_ignored_when_defined() {
        let e = env(&[("PORT", "8080")]);
        assert_eq!(expand_env("${PORT:-9999}", &e), "8080");
    }

    #[test]
    fn multiple_and_adjacent() {
        let e = env(&[("A", "1"), ("B", "2")]);
        assert_eq!(expand_env("${A}${B}-${A}", &e), "12-1");
    }
}
