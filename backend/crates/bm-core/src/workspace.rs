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

/// 是否为可安全按 UTF-8 文本展示的媒体类型。
pub fn is_text(mime: &str) -> bool {
    mime.starts_with("text/")
}
