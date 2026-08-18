//! 双栅栏 trust fence（契约台账 §1 栅栏 A/B）：
//! A. Host/Origin 信任栅栏（DNS-rebinding 防御）：loopback + 显式 trustedHosts。
//! B. 17 个特权方法 loopback-pin：即使 LAN 部署也强制 loopback。

/// 特权方法表（台账逐字 15 个 + 目录 2 个；源 packages/client/connection/src/index.ts
/// PRIVILEGED_METHODS；host.listDirectory/host.createDirectory 触碰文件系统，
/// 注释即"特权"，2026-08-18 交叉审查 #33 补录）。
pub const PRIVILEGED_METHODS: &[&str] = &[
    "agentPreset.read",
    "agentPreset.copy",
    "agentPreset.openDocument",
    "agentPreset.remove",
    "host.pickDirectory",
    "host.openPath",
    "host.listDirectory",
    "host.createDirectory",
    "settings.describe",
    "settings.openDocument",
    "settings.update",
    "settings.replace",
    "settings.mutate",
    "credentials.describe",
    "credentials.set",
    "credentials.unset",
    "llm.discoverModels",
];

/// loopback 判定：`localhost` / `[::1]` / 127/8 点分（台账：恰好 4 段、首段 127、
/// 每段 1-3 位数字且 ≤255）。
pub fn is_loopback_hostname(hostname: &str) -> bool {
    if hostname == "localhost" || hostname == "[::1]" {
        return true;
    }
    let parts: Vec<&str> = hostname.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    if parts[0] != "127" {
        return false;
    }
    parts.iter().all(|p| {
        !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()) && {
            let n: u32 = p.parse().unwrap_or(256);
            n <= 255
        }
    })
}

/// 规范化 host[:port]：小写主机名；去掉显式 `:80`（台账：WHATWG 规范化后比较）。
/// 返回 (hostname, port)。解析失败 → None。
pub fn parse_authority(authority: &str) -> Option<(String, Option<String>)> {
    let authority = authority.trim();
    if authority.is_empty() {
        return None;
    }
    if let Some(rest) = authority.strip_prefix('[') {
        // IPv6 字面量 [::1]:port（默认端口 80 归一到 None，与主机名分支一致）。
        let idx = rest.find(']')?;
        let ip = &rest[..idx];
        let after = &rest[idx + 1..];
        let port = if after.is_empty() {
            None
        } else {
            let p = after.strip_prefix(':')?.to_string();
            if p == "80" { None } else { Some(p) }
        };
        return Some((format!("[{ip}]"), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            let host = host.to_lowercase();
            let port = if port == "80" { None } else { Some(port.to_string()) };
            Some((host, port))
        }
        _ => Some((authority.to_lowercase(), None)),
    }
}

/// trustedHosts 条目匹配：带显式端口 = 精确 host:port；无端口 = hostname 匹配任意端口。
pub fn is_trusted_authority(hostname: &str, port: Option<&str>, trusted_hosts: &[String]) -> bool {
    trusted_hosts.iter().any(|entry| {
        let Some((entry_host, entry_port)) = parse_authority(entry) else {
            return false;
        };
        if entry_host != hostname {
            return false;
        }
        match entry_port {
            Some(ep) => Some(ep.as_str()) == port,
            None => true,
        }
    })
}

/// Host/Origin 信任栅栏判定（台账栅栏 A 完整逻辑）。
/// `request_host` = Host 头原始值；`origin` = Origin 头（Option）；`sec_fetch_site` = sec-fetch-site 头。
pub fn is_trusted_api_request(
    request_host: Option<&str>,
    origin: Option<&str>,
    sec_fetch_site: Option<&str>,
    trusted_hosts: &[String],
) -> bool {
    // 1. 无 host 头 → 拒绝。
    let Some(host) = request_host else {
        return false;
    };
    let Some((hostname, port)) = parse_authority(host) else {
        return false;
    };
    // hostname 必须 loopback 或 trusted。
    if !is_loopback_hostname(&hostname)
        && !is_trusted_authority(&hostname, port.as_deref(), trusted_hosts)
    {
        return false;
    }
    // 2. Cross-site 栅栏：sec-fetch-site === 'cross-site' → 拒绝。
    if sec_fetch_site == Some("cross-site") {
        return false;
    }
    // 3. Origin 栅栏：无 Origin 放行；有 Origin 必须 host(+port) 匹配；'null' 拒绝。
    match origin {
        None => true,
        Some(o) => {
            if o == "null" {
                return false;
            }
            // Origin 是完整 URL（带 scheme）——按 WHATWG new URL(origin).host 取
            // host:port（含端口比对：http://127.0.0.1:evil 不得冒充 :3080）。
            match extract_url_host_port(o) {
                Some((o_host, o_port)) => o_host == hostname && o_port == port,
                None => false,
            }
        }
    }
}

/// 从完整 URL 提取 host:port（WHATWG new URL(x).host 语义：剥 scheme/路径/查询，
/// 端口与 Host 侧 parse_authority 同一归一规则：显式 :80 → None）。
fn extract_url_host_port(url: &str) -> Option<(String, Option<String>)> {
    let rest = match url.find("://") {
        Some(idx) => &url[idx + 3..],
        None => url,
    };
    // 取到第一个 / ? # 为止
    let end = rest
        .find(['/', '?', '#'])
        .unwrap_or(rest.len());
    let authority = &rest[..end];
    parse_authority(authority)
}

/// 特权方法判定：路径 `/api/<method>` 的 method 段命中特权表。
pub fn is_privileged_method(path: &str) -> Option<&'static str> {
    let rest = path.strip_prefix("/api/")?;
    PRIVILEGED_METHODS.iter().find(|m| **m == rest).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_rules() {
        assert!(is_loopback_hostname("localhost"));
        assert!(is_loopback_hostname("[::1]"));
        assert!(is_loopback_hostname("127.0.0.1"));
        assert!(is_loopback_hostname("127.255.1.9"));
        assert!(!is_loopback_hostname("128.0.0.1"));
        assert!(!is_loopback_hostname("192.168.1.1"));
        assert!(!is_loopback_hostname("127.0.0"));
        assert!(!is_loopback_hostname("127.0.0.256"));
    }

    #[test]
    fn authority_parsing() {
        assert_eq!(
            parse_authority("127.0.0.1:3080"),
            Some(("127.0.0.1".to_string(), Some("3080".to_string())))
        );
        assert_eq!(parse_authority("LocalHost"), Some(("localhost".to_string(), None)));
        assert_eq!(parse_authority("[::1]:80"), Some(("[::1]".to_string(), None)));
        assert_eq!(parse_authority("host:abc"), Some(("host:abc".to_string(), None)));
    }

    #[test]
    fn fence_logic() {
        // loopback 放行
        assert!(is_trusted_api_request(Some("127.0.0.1:3080"), None, None, &[]));
        // 无 host 拒绝
        assert!(!is_trusted_api_request(None, None, None, &[]));
        // 外部 host + 空 trustedHosts 拒绝
        assert!(!is_trusted_api_request(Some("evil.example.com"), None, None, &[]));
        // trustedHosts 放行
        assert!(is_trusted_api_request(
            Some("192.168.1.5:3080"),
            None,
            None,
            &["192.168.1.5".to_string()]
        ));
        // 端口不匹配拒绝（trusted 无端口=任意端口；显式端口必须匹配）
        assert!(!is_trusted_api_request(
            Some("192.168.1.5:9999"),
            None,
            None,
            &["192.168.1.5:3080".to_string()]
        ));
        // cross-site 拒绝
        assert!(!is_trusted_api_request(
            Some("127.0.0.1:3080"),
            None,
            Some("cross-site"),
            &[]
        ));
        // Origin 不匹配拒绝
        assert!(!is_trusted_api_request(
            Some("127.0.0.1:3080"),
            Some("http://evil.example.com"),
            None,
            &[]
        ));
        // Origin null 拒绝
        assert!(!is_trusted_api_request(Some("127.0.0.1:3080"), Some("null"), None, &[]));
        // Origin 匹配放行
        assert!(is_trusted_api_request(
            Some("127.0.0.1:3080"),
            Some("http://127.0.0.1:3080"),
            None,
            &[]
        ));
    }

    #[test]
    fn privileged_methods_verbatim() {
        assert_eq!(PRIVILEGED_METHODS.len(), 17);
        assert_eq!(is_privileged_method("/api/settings.describe"), Some("settings.describe"));
        assert_eq!(is_privileged_method("/api/session.list"), None);
        assert!(PRIVILEGED_METHODS.contains(&"llm.discoverModels"));
        assert!(PRIVILEGED_METHODS.contains(&"host.pickDirectory"));
        assert!(PRIVILEGED_METHODS.contains(&"credentials.set"));
        // #33：目录 RPC 注释即特权，必须 loopback-pin。
        assert_eq!(is_privileged_method("/api/host.listDirectory"), Some("host.listDirectory"));
        assert_eq!(is_privileged_method("/api/host.createDirectory"), Some("host.createDirectory"));
    }

    #[test]
    fn origin_port_must_match() {
        // 回归 SEC-002：Origin 端口与 Host 端口必须一致（跨端口 localhost 不得过闸）。
        assert!(is_trusted_api_request(
            Some("127.0.0.1:3080"),
            Some("http://127.0.0.1:3080"),
            None,
            &[]
        ));
        assert!(!is_trusted_api_request(
            Some("127.0.0.1:3080"),
            Some("http://127.0.0.1:9999"),
            None,
            &[]
        ));
        // 显式 :80 归一为 None，与无端口 Host 匹配（对齐 WHATWG）。
        assert!(is_trusted_api_request(
            Some("127.0.0.1"),
            Some("http://127.0.0.1:80"),
            None,
            &[]
        ));
    }
}
