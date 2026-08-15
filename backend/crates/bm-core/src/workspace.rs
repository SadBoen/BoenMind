//! 工作文件夹：目录枚举与文件读取。
//!
//! 所有路径操作都以工作文件夹为根，杜绝路径穿越（`..` 或绝对路径会被拒绝）。

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("路径越界：{0}")]
    OutsideRoot(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    /// 相对工作文件夹的路径（正斜杠分隔）
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: i64,
}

/// 将相对路径安全地解析到工作文件夹内。
///
/// 返回规范化后的绝对路径；解析失败或越界返回 `OutsideRoot`。
pub fn safe_join(root: &Path, rel: &str) -> Result<PathBuf, WorkspaceError> {
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() {
        return Ok(root.to_path_buf());
    }
    // 拒绝任何含 ".." 的路径，避免绕过 canonicalize 的解析
    if rel.split('/').any(|seg| seg == "..") {
        return Err(WorkspaceError::OutsideRoot(rel.to_string()));
    }
    let candidate = root.join(rel);
    let norm_root = root.canonicalize().map_err(WorkspaceError::Io)?;
    let norm = candidate.canonicalize().map_err(WorkspaceError::Io)?;
    // Path::starts_with 按路径组件比较，符号链接已被 canonicalize 解析到真实位置
    if !norm.starts_with(&norm_root) {
        return Err(WorkspaceError::OutsideRoot(rel.to_string()));
    }
    Ok(norm)
}

/// 列出目录内容（目录在前，按名称排序）。
pub fn list_dir(root: &Path, rel: &str) -> Result<Vec<FileEntry>, WorkspaceError> {
    let dir = safe_join(root, rel)?;
    let mut entries: Vec<FileEntry> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type()?.is_dir();
        let meta = entry.metadata()?;
        let rel_path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", rel.trim_end_matches('/'), name)
        };
        entries.push(FileEntry {
            path: rel_path,
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            is_dir,
            size: if is_dir { 0 } else { meta.len() },
            name,
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(entries)
}

/// 按扩展名推断媒体类型。
pub fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => "text/markdown",
        "txt" | "log" | "text" => "text/plain",
        "json" => "application/json",
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "java" | "c" | "cpp" | "h" | "hpp"
        | "toml" | "yaml" | "yml" | "xml" | "html" | "css" | "sh" | "sql" | "vue" | "svelte"
        | "zig" | "rb" | "php" | "kt" | "swift" | "lua" | "r" | "scala" | "dart" | "ex" | "exs"
        | "clj" | "hs" | "ml" | "jl" | "astro" | "mjs" | "cjs" => "text/code",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// 读取工作文件夹内的文件字节。
pub fn read_file(root: &Path, rel: &str) -> Result<Vec<u8>, WorkspaceError> {
    let path = safe_join(root, rel)?;
    if !path.is_file() {
        return Err(WorkspaceError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "不是文件",
        )));
    }
    Ok(fs::read(path)?)
}

/// 写文本文件到工作文件夹内（M2 编辑器保存用；内容整体覆盖）。
///
/// 与 `safe_join` 的差异：目标文件可能尚不存在（新建），canonicalize 对
/// 不存在的路径会失败——因此对**父目录** canonicalize 校验在根内，文件名
/// 在其上 join（拒绝 `..` 与空名）。父目录必须存在（不递归建目录，目录
/// 创建留给文件树操作）。
pub fn write_file(root: &Path, rel: &str, content: &str) -> Result<(), WorkspaceError> {
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() {
        return Err(WorkspaceError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "路径为空",
        )));
    }
    if rel.split('/').any(|seg| seg == "..") {
        return Err(WorkspaceError::OutsideRoot(rel.to_string()));
    }
    let norm_root = root.canonicalize().map_err(WorkspaceError::Io)?;
    let (parent, name) = match rel.rfind('/') {
        Some(i) => (&rel[..i], &rel[i + 1..]),
        None => ("", rel),
    };
    let parent_path = if parent.is_empty() {
        norm_root.clone()
    } else {
        let p = norm_root.join(parent).canonicalize().map_err(WorkspaceError::Io)?;
        if !p.starts_with(&norm_root) {
            return Err(WorkspaceError::OutsideRoot(rel.to_string()));
        }
        p
    };
    if name.is_empty() || name == "." || name == ".." {
        return Err(WorkspaceError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "非法文件名",
        )));
    }
    fs::write(parent_path.join(name), content.as_bytes())?;
    Ok(())
}

/// 是否为可安全按 UTF-8 文本展示的媒体类型。
/// json 走 application/json（mime_for 特例），实质是文本——编辑/预览按
/// 文本处理（M2 编辑器实测发现：不认则 package.json 被判 binary 只读）。
pub fn is_text(mime: &str) -> bool {
    mime.starts_with("text/") || matches!(mime, "application/json" | "application/xml")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个带目录结构的临时根（root/{a/b, target.txt}），返回规范化后的根路径
    /// （macOS 上 /var → /private/var 为符号链接，canonicalize 保证断言一致）。
    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bm-workspace-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("target.txt"), "secret").unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn safe_join_allows_internal_paths() {
        let root = temp_root();
        assert_eq!(safe_join(&root, "a/b").unwrap(), root.join("a/b"));
        // 前导斜杠与空路径
        assert_eq!(safe_join(&root, "/a/b").unwrap(), safe_join(&root, "a/b").unwrap());
        assert_eq!(safe_join(&root, "").unwrap(), root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn safe_join_rejects_traversal() {
        let root = temp_root();
        // `..` 组件
        assert!(matches!(
            safe_join(&root, "../x"),
            Err(WorkspaceError::OutsideRoot(_))
        ));
        assert!(matches!(
            safe_join(&root, "a/../../x"),
            Err(WorkspaceError::OutsideRoot(_))
        ));
        // 绝对路径：前导斜杠被剥离、按相对路径处理，不会指向真实根
        assert_eq!(safe_join(&root, "/a/b").unwrap(), root.join("a/b"));
        // 不存在的路径（canonicalize 失败 → Io，同样不会越界）
        assert!(safe_join(&root, "nope").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn safe_join_rejects_symlink_escape() {
        let root = temp_root();
        #[cfg(unix)]
        {
            // 符号链接指向根外：canonicalize 解析后应在 root 之外 → 拒绝
            let outside = std::env::temp_dir().join("bm-workspace-outside-target");
            fs::write(&outside, "outside").unwrap();
            std::os::unix::fs::symlink(&outside, root.join("a/link")).unwrap();
            assert!(matches!(
                safe_join(&root, "a/link"),
                Err(WorkspaceError::OutsideRoot(_))
            ));
            fs::remove_file(&outside).ok();
        }
        let _ = fs::remove_dir_all(&root);
    }
}
