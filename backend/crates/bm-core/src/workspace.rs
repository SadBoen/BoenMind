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

/// 目录浏览器结果：指定目录内容 + 父目录（供前端「新建项目」选父目录时逐级上溯）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct BrowseResult {
    /// 规范化后的当前目录
    pub path: String,
    /// 父目录路径（系统根时为空字符串）
    pub parent: String,
    pub entries: Vec<FileEntry>,
}

/// 浏览任意绝对目录（目录选择器用，不校验白名单——浏览是只读操作，
/// 与 `list_workspace`（工作区内浏览）互补；权限不足的目录项会跳过）。
pub fn browse_dir(path: &str) -> Result<BrowseResult, WorkspaceError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        // 系统根：Windows 枚举盘符，Unix 列 /
        return browse_system_root();
    }
    let dir = PathBuf::from(trimmed);
    if !dir.is_dir() {
        return Err(WorkspaceError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("目录不存在或不可读: {}", dir.display()),
        )));
    }
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.clone());
    // 跳过无权限的子目录（系统目录常见），其余照常列出
    let entries = fs::read_dir(&canon)
        .map_err(WorkspaceError::Io)?
        .filter_map(|e| e.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().ok()?.is_dir();
            let size = entry.metadata().ok().map(|m| if is_dir { 0 } else { m.len() }).unwrap_or(0);
            Some(FileEntry {
                path: name.clone(),
                modified: entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                is_dir,
                size,
                name,
            })
        })
        .collect::<Vec<_>>();
    let parent = canon.parent().map(|p| p.display().to_string()).unwrap_or_default();
    Ok(BrowseResult {
        path: canon.display().to_string(),
        parent,
        entries,
    })
}

/// 系统根视图：Windows 枚举逻辑盘符（`C:\`、`D:\`…），Unix 列 `/`。
/// 返回的 path 为空字符串，parent 也为空——前端据此显示根目录面包屑。
fn browse_system_root() -> Result<BrowseResult, WorkspaceError> {
    #[cfg(windows)]
    let entries = {
        let mut v = Vec::new();
        // 枚举 A:\..Z:\，只保留存在的盘符
        for letter in b'A'..=b'Z' {
            let drive = format!("{}:\\", letter as char);
            if Path::new(&drive).exists() {
                v.push(FileEntry {
                    path: drive.clone(),
                    modified: 0,
                    is_dir: true,
                    size: 0,
                    name: drive,
                });
            }
        }
        v
    };
    #[cfg(not(windows))]
    let entries = {
        match fs::read_dir("/") {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().ok()?.is_dir();
                    Some(FileEntry { path: name.clone(), modified: 0, is_dir, size: 0, name })
                })
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        }
    };
    Ok(BrowseResult {
        path: String::new(),
        parent: String::new(),
        entries,
    })
}

/// 创建新项目：在 `parent` 下建 `name` 目录（可选 `git init`），并把项目
/// 路径追加进白名单。返回（项目绝对路径, 新白名单, git 是否初始化成功）。
///
/// 路径语义：项目必须位于调用方（路由层）已校验的信任根之下，本函数只做
/// 目录操作与白名单去重，不重复做越界判断。
pub fn create_project(
    parent: &Path,
    name: &str,
    git_init: bool,
    roots: &[PathBuf],
) -> Result<(PathBuf, Vec<PathBuf>, bool), WorkspaceError> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(WorkspaceError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "项目名称非法（不能为空、不能含路径分隔符）",
        )));
    }
    let norm_parent = parent.canonicalize().map_err(WorkspaceError::Io)?;
    let project = norm_parent.join(name);
    if project.exists() {
        return Err(WorkspaceError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("目录已存在: {}", project.display()),
        )));
    }
    fs::create_dir_all(&project).map_err(WorkspaceError::Io)?;

    let git_ok = if git_init {
        run_git_init(&project)
    } else {
        true
    };
    let mut roots = roots.to_vec();
    if !roots.iter().any(|r| r == &project) {
        roots.push(project.clone());
    }
    Ok((project, roots, git_ok))
}

/// `git init -b main`（失败不致命：项目目录已建好，只是没有版本控制）。
fn run_git_init(project: &Path) -> bool {
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(project)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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

/// 路径是否落在任一已登记根之下（canonicalize 后按前缀比较）。
/// 根本身不存在时回落到字面 `starts_with`，避免未建目录的工作区被误拒。
pub fn path_under_any(candidate: &Path, roots: &[PathBuf]) -> bool {
    let norm_candidate = candidate.canonicalize().unwrap_or_else(|_| candidate.to_path_buf());
    roots.iter().any(|root| {
        let norm_root = root.canonicalize().unwrap_or_else(|_| root.clone());
        norm_candidate == norm_root || norm_candidate.starts_with(&norm_root)
    })
}

/// 配置里可作为工作区根的全部路径：全局 working_dir + APP 覆盖 + 已确认项目。
pub fn trusted_roots(config: &crate::config::AppConfig) -> Vec<PathBuf> {
    let mut roots = vec![config.working_dir.clone()];
    for profile in config.apps.values() {
        if let Some(dir) = &profile.working_dir {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                roots.push(PathBuf::from(trimmed));
            }
        }
    }
    roots.extend(config.trusted_project_roots.iter().cloned());
    roots
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

    #[test]
    fn path_under_any_accepts_child_and_rejects_sibling() {
        let root = temp_root();
        assert!(path_under_any(&root.join("a/b"), &[root.clone()]));
        assert!(!path_under_any(&root.parent().unwrap().join("other"), &[root.clone()]));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn browse_dir_lists_and_reports_parent() {
        let root = temp_root();
        let r = browse_dir(root.to_str().unwrap()).expect("browse 应成功");
        assert!(r.entries.iter().any(|e| e.name == "target.txt"));
        assert!(!r.parent.is_empty(), "应返回父目录");
        let up = browse_dir(&r.parent).expect("父目录浏览应成功");
        assert!(up.path == r.parent);
        assert!(browse_dir(&format!("{}/no-such-dir", root.display())).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn create_project_makes_dir_git_and_whitelist() {
        let root = temp_root();
        let roots: Vec<PathBuf> = vec![root.clone()];
        let (project, new_roots, git_ok) =
            create_project(&root, "my-app", true, &roots).expect("create 应成功");
        assert!(project.join(".git").is_dir(), "git init 应生效");
        assert!(git_ok);
        assert_eq!(new_roots.len(), 2, "白名单应新增项目");
        assert!(new_roots.contains(&project));
        // 已存在目录 → 拒绝
        assert!(create_project(&root, "my-app", false, &roots).is_err());
        // 非法名称 → 拒绝
        assert!(create_project(&root, "../evil", false, &roots).is_err());
        assert!(create_project(&root, "", false, &roots).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
