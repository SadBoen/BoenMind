//! 轻量共享工具：HTTP agent 与目录复制（供 providers / skills / plugins 复用）。
//!
//! 统一 agent 配置的意图：
//! - 4xx/5xx 作为正常响应返回（`http_status_as_error(false)`），由调用方检查
//!   状态码——这样错误 body（如 401 详情）能透传给用户，且 404 可被当作
//!   "资源不存在"的分支处理（如 skills 安装时 main 分支 404 需回退 master，
//!   若 4xx 直接报错该回退永远走不到）。

use std::path::Path;
use std::time::Duration;

/// 标准 HTTP agent：连接 10s / 单次调用 20s，4xx/5xx 不作错误返回。
pub(crate) fn http_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_per_call(Some(Duration::from_secs(20)))
                .http_status_as_error(false)
                .build(),
        )
    })
}

/// 全局超时版 HTTP agent（整次请求受总时长约束，含连接/读体），
/// 供连接测试等需要严格收口的场景；调用频率低，不做静态缓存。
pub(crate) fn http_agent_global(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build(),
    )
}

/// 递归复制目录树，可排除指定文件名（如 .bm-meta.json）。
pub(crate) fn copy_dir_excluding(src: &Path, dest: &Path, exclude: &[&str]) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if exclude.contains(&name.to_string_lossy().as_ref()) {
            continue;
        }
        let from = entry.path();
        let to = dest.join(name);
        if from.is_dir() {
            copy_dir_excluding(&from, &to, exclude)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_dir_excludes_files() {
        let tmp = std::env::temp_dir().join(format!("bm-httputil-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let dest = tmp.join("dest");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("keep.txt"), "1").unwrap();
        std::fs::write(src.join("skip.json"), "2").unwrap();
        std::fs::write(src.join("sub").join("nested.txt"), "3").unwrap();
        copy_dir_excluding(&src, &dest, &["skip.json"]).unwrap();
        assert!(dest.join("keep.txt").is_file());
        assert!(!dest.join("skip.json").exists());
        assert!(dest.join("sub").join("nested.txt").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
