//! host_fs —— 工作目录作用域的文件系统路径约束模块。
//!
//! 文件管理器窗口单元的全部 FS 操作（list/read/write/upload/download/mkdir）
//! 必须经 [`resolve_in_workdir`] 把**相对路径**解析为 workdir 内的绝对路径：
//! 拒绝绝对路径 / `..` / null 字节；`canonicalize` 解析最终目标后按**路径组件**
//! 前缀检查（`Path::starts_with` 是组件级比较，`/data/workdir` 不会匹配
//! `/data/workdir-evil`）；symlink 逃逸（workdir 内软链指向外部）在此被拒。
//!
//! 设计边界：本模块是纯路径逻辑，不依赖 AppState（workdir 由调用方从
//! settings 读取），保证安全规则可独立单测。

use std::path::{Component, Path, PathBuf};

/// 文本 read/write 的大小上限（字节）。超过 → `file-too-large`。
/// 前端提示"作为下载打开"。二进制一律走 download 端点，不经 readFile。
pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB

/// 上传单文件大小上限（字节）。
pub const MAX_UPLOAD_BYTES: u64 = 100 * 1024 * 1024; // 100 MiB

/// 单目录列表条目上限（懒加载树上单层保护，防巨目录拖死前端）。
pub const MAX_ENTRIES_PER_DIR: usize = 2000;

/// 路径约束错误。每个变体映射一个稳定的 RPC 错误码（前端按码提示，不做字符串匹配）。
#[derive(Debug)]
pub enum HostFsError {
    /// 路径本身非法：空 / "." / 绝对路径 / `..` / null 字节。
    InvalidPath(String),
    /// 工作目录未设置（settings host.workdir 缺失/空）。
    WorkdirNotConfigured,
    /// 工作目录不可用（不存在 / 不是目录 / 不可读）。
    WorkdirInvalid(String),
    /// 解析结果（canonicalize 后）逃出工作目录（symlink 逃逸 / 前缀混淆）。
    NotInside(String),
    /// 目标不存在（read/stat 场景）。
    NotFound(String),
    /// 目标已存在（writeFile/upload 且未显式 overwrite）。
    AlreadyExists(String),
    /// 目标应是文件却是目录 / 应是目录却是文件。
    WrongKind(String),
    /// 文本超出 MAX_TEXT_BYTES。
    TooLarge(u64),
    /// 底层 IO 失败。
    Io(std::io::Error),
}

impl HostFsError {
    /// RPC 错误码（对齐 api.rs 现有 host.* 错误风格：snake-case 短码）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPath(_) => "invalid-path",
            Self::WorkdirNotConfigured => "workdir-not-configured",
            Self::WorkdirInvalid(_) => "workdir-invalid",
            Self::NotInside(_) => "not-inside-workdir",
            Self::NotFound(_) => "file-not-found",
            Self::AlreadyExists(_) => "file-exists",
            Self::WrongKind(_) => "wrong-file-kind",
            Self::TooLarge(_) => "file-too-large",
            Self::Io(_) => "file-io-error",
        }
    }
}

impl std::fmt::Display for HostFsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath(p) => write!(f, "invalid path: {p}"),
            Self::WorkdirNotConfigured => write!(f, "work directory not configured"),
            Self::WorkdirInvalid(p) => write!(f, "work directory invalid: {p}"),
            Self::NotInside(p) => write!(f, "path escapes work directory: {p}"),
            Self::NotFound(p) => write!(f, "not found: {p}"),
            Self::AlreadyExists(p) => write!(f, "already exists: {p}"),
            Self::WrongKind(p) => write!(f, "wrong file kind: {p}"),
            Self::TooLarge(n) => write!(f, "file too large: {n} bytes"),
            Self::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl From<std::io::Error> for HostFsError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// 相对路径词法校验：拒绝 "." / 绝对路径 / `..` / null 字节 / 盘符前缀。
/// **空字符串合法**（表示 workdir 根——文件管理器列根/建目录到根都传 ""）。
/// 允许 `./a`（CurDir 跳过）。返回规范化后的相对 PathBuf。
fn validate_rel(rel: &str) -> Result<PathBuf, HostFsError> {
    let rel = rel.trim();
    if rel == "." {
        return Err(HostFsError::InvalidPath(rel.to_string()));
    }
    if rel.contains('\0') {
        return Err(HostFsError::InvalidPath(rel.to_string()));
    }
    if rel.is_empty() {
        return Ok(PathBuf::new());
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err(HostFsError::InvalidPath(rel.to_string()));
    }
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => return Err(HostFsError::InvalidPath(rel.to_string())),
            Component::Prefix(_) | Component::RootDir => {
                return Err(HostFsError::InvalidPath(rel.to_string()))
            }
            Component::Normal(seg) => out.push(seg),
        }
    }
    Ok(out)
}

/// 工作目录作用域解析（核心入口）。
///
/// 规则：
/// 1. `rel` 必须是相对路径（绝对/`..`/null → InvalidPath）。
/// 2. workdir 必须存在且是目录（否则 WorkdirInvalid）。
/// 3. 目标已存在 → canonicalize 全目标；不存在 → 父目录 canonicalize + 文件名
///    （父目录必须存在——写新文件时目标目录不能凭空出现）。
/// 4. 解析结果必须组件级 `starts_with` canonical workdir（symlink 逃逸/前缀混淆 → NotInside）。
pub fn resolve_in_workdir(workdir: &Path, rel: &str) -> Result<PathBuf, HostFsError> {
    let rel_path = validate_rel(rel)?;
    let wd = workdir.canonicalize().map_err(|_| {
        HostFsError::WorkdirInvalid(workdir.to_string_lossy().to_string())
    })?;
    if !wd.is_dir() {
        return Err(HostFsError::WorkdirInvalid(workdir.to_string_lossy().to_string()));
    }
    // 空 rel = workdir 根（列根/建目录到根）。
    if rel_path.as_os_str().is_empty() {
        return Ok(wd);
    }
    let joined = wd.join(&rel_path);
    let target = if joined.exists() {
        joined.canonicalize().map_err(|_| {
            HostFsError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("cannot canonicalize {}", joined.display()),
            ))
        })?
    } else {
        let parent = joined.parent().ok_or_else(|| {
            HostFsError::InvalidPath(joined.to_string_lossy().to_string())
        })?;
        let parent_canon = parent.canonicalize().map_err(|_| {
            HostFsError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("parent directory missing: {}", parent.display()),
            ))
        })?;
        let name = joined.file_name().ok_or_else(|| {
            HostFsError::InvalidPath(joined.to_string_lossy().to_string())
        })?;
        parent_canon.join(name)
    };
    if !target.starts_with(&wd) {
        return Err(HostFsError::NotInside(target.to_string_lossy().to_string()));
    }
    Ok(target)
}

/// 校验 workdir 本身可用（存在、是目录、可读）。设置保存与启动时调用。
pub fn validate_workdir(workdir: &Path) -> Result<(), HostFsError> {
    let wd = workdir.canonicalize().map_err(|_| {
        HostFsError::WorkdirInvalid(workdir.to_string_lossy().to_string())
    })?;
    if !wd.is_dir() {
        return Err(HostFsError::WorkdirInvalid(workdir.to_string_lossy().to_string()));
    }
    // 读探测：read_dir 失败 → 不可读。
    std::fs::read_dir(&wd).map_err(|e| {
        HostFsError::Io(std::io::Error::new(e.kind(), format!("unreadable: {e}")))
    })?;
    Ok(())
}

/// 隐藏条目判定：`.` 前缀（Unix 惯例）+ Windows FILE_ATTRIBUTE_HIDDEN。
/// 与 api.rs `is_hidden_path` 同规则，模块内自持一份（避免循环依赖 api.rs）。
pub fn is_hidden_path(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with('.') && !name.is_empty() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if let Ok(meta) = p.metadata() {
            return meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0;
        }
    }
    false
}

/// 文本可预览扩展名白名单（readFile 只认这些 + 其他一律走 download 端点）。
pub fn is_text_previewable(ext: &str) -> bool {
    matches!(ext, "md" | "markdown" | "txt" | "log" | "json" | "toml" | "yaml" | "yml" | "rs" | "ts" | "tsx" | "js" | "jsx" | "css" | "html" | "py" | "sh")
}

/// 图片预览扩展名白名单（download 端点对它们 `Content-Disposition: inline`，
/// 可直接 `<img src>`；svg/html 永远 attachment——防 XSS）。
pub fn is_image_previewable(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "avif")
}

/// download 端点的 MIME 映射（未知扩展 → octet-stream；svg → image/svg+xml
/// 但仅在 is_image_previewable 之外时按附件处理）。
pub fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "log" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "toml" => "application/toml",
        "yaml" | "yml" => "application/yaml",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "rs" => "text/plain; charset=utf-8",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "exe" => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

/// 原子写：同目录 tmp + rename。`overwrite=false` 且目标已存在 → AlreadyExists；
/// 目标是目录 → WrongKind（绝不 remove 目录）。Windows rename 不覆盖目标 → 先删再换名。
pub fn atomic_write(target: &Path, bytes: &[u8], overwrite: bool) -> Result<(), HostFsError> {
    if target.is_dir() {
        return Err(HostFsError::WrongKind(target.to_string_lossy().to_string()));
    }
    if target.exists() && !overwrite {
        return Err(HostFsError::AlreadyExists(target.to_string_lossy().to_string()));
    }
    let mut tmp = target.to_path_buf();
    tmp.set_file_name(format!(
        ".{}.bm-tmp",
        target.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    std::fs::write(&tmp, bytes)?;
    if target.exists() {
        // 走到这里只可能是文件（is_dir 已拒）且 overwrite=true → 删旧换新。
        std::fs::remove_file(target)?;
    }
    std::fs::rename(&tmp, target).map_err(|e| {
        // rename 失败：清理 tmp 后再报错。
        let _ = std::fs::remove_file(&tmp);
        HostFsError::Io(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_workdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bm-hostfs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ---- 词法校验 ----

    #[test]
    fn rejects_absolute_and_parent() {
        assert_eq!(
            validate_rel("/etc/passwd").unwrap_err().code(),
            "invalid-path"
        );
        assert_eq!(
            validate_rel("../etc/passwd").unwrap_err().code(),
            "invalid-path"
        );
        assert_eq!(
            validate_rel("a/../../b").unwrap_err().code(),
            "invalid-path"
        );
    }

    #[test]
    fn rejects_dot_and_null() {
        assert_eq!(validate_rel(".").unwrap_err().code(), "invalid-path");
        assert_eq!(validate_rel("a\0b").unwrap_err().code(), "invalid-path");
    }

    #[test]
    fn empty_is_workdir_root() {
        // 空字符串合法（= workdir 根）：list/mkdir 到根都传 ""。
        assert_eq!(validate_rel("").unwrap(), PathBuf::new());
        let wd = tmp_workdir("empty-root");
        std::fs::create_dir_all(wd.join("x")).unwrap();
        assert_eq!(resolve_in_workdir(&wd, "").unwrap(), wd.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[test]
    fn allows_normal_and_curdir() {
        assert_eq!(validate_rel("./a/b.md").unwrap(), PathBuf::from("a/b.md"));
        assert_eq!(validate_rel("src/app.rs").unwrap(), PathBuf::from("src/app.rs"));
    }

    // ---- resolve_in_workdir 矩阵 ----

    #[test]
    fn resolve_existing_file() {
        let wd = tmp_workdir("existing");
        std::fs::create_dir_all(wd.join("src")).unwrap();
        std::fs::write(wd.join("src").join("a.md"), "x").unwrap();
        let out = resolve_in_workdir(&wd, "src/a.md").unwrap();
        assert_eq!(out, wd.canonicalize().unwrap().join("src").join("a.md"));
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[test]
    fn resolve_new_file_parent_must_exist() {
        let wd = tmp_workdir("newfile");
        std::fs::create_dir_all(wd.join("sub")).unwrap();
        let out = resolve_in_workdir(&wd, "sub/b.txt").unwrap();
        assert_eq!(out, wd.canonicalize().unwrap().join("sub").join("b.txt"));
        // 父目录不存在 → 错误（不凭空创建）。
        assert!(resolve_in_workdir(&wd, "missing/c.txt").is_err());
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[test]
    fn resolve_rejects_escape() {
        let wd = tmp_workdir("escape");
        assert_eq!(
            resolve_in_workdir(&wd, "../x").unwrap_err().code(),
            "invalid-path"
        );
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[test]
    fn resolve_rejects_workdir_missing() {
        let wd = tmp_workdir("missing");
        let gone = wd.join("nope");
        assert_eq!(
            resolve_in_workdir(&gone, "a.txt").unwrap_err().code(),
            "workdir-invalid"
        );
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[test]
    fn prefix_confusion_is_component_based() {
        // Path::starts_with 组件级：/data/workdir-evil 不匹配 /data/workdir。
        let base = Path::new("/data/workdir");
        assert!(base.join("evil.txt").starts_with(base));
        assert!(!Path::new("/data/workdir-evil/x").starts_with(base));
        assert!(!Path::new("/data/workdir-2/x").starts_with(base));
    }

    #[test]
    fn encoded_dotdot_is_literal_filename() {
        // %2e%2e 只是字面文件名，不解释为 ..，resolve 后仍在 workdir 内。
        // 目录必须实际存在（否则是"父目录缺失"，与穿越无关）。
        let wd = tmp_workdir("encoded");
        std::fs::create_dir_all(wd.join("%2e%2e")).unwrap();
        std::fs::write(wd.join("%2e%2e").join("passwd"), "x").unwrap();
        let out = resolve_in_workdir(&wd, "%2e%2e/passwd").unwrap();
        assert!(out.starts_with(wd.canonicalize().unwrap()));
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_rejected() {
        use std::os::unix::fs::symlink;
        let wd = tmp_workdir("symlink");
        let outside = tmp_workdir("symlink-outside");
        std::fs::write(outside.join("secret.txt"), "topsecret").unwrap();
        // workdir 内软链指向外部文件。
        let _ = symlink(outside.join("secret.txt"), wd.join("link.txt"));
        let err = resolve_in_workdir(&wd, "link.txt").unwrap_err();
        assert_eq!(err.code(), "not-inside-workdir");
        // workdir 内软链指向外部目录 → 其下文件同样逃逸。
        let _ = symlink(&outside, wd.join("dirlink"));
        assert_eq!(
            resolve_in_workdir(&wd, "dirlink/secret.txt").unwrap_err().code(),
            "not-inside-workdir"
        );
        // workdir 内正常文件不受影响。
        std::fs::write(wd.join("ok.txt"), "ok").unwrap();
        assert!(resolve_in_workdir(&wd, "ok.txt").is_ok());
        let _ = std::fs::remove_dir_all(&wd);
        let _ = std::fs::remove_dir_all(&outside);
    }

    // ---- 其他辅助 ----

    #[test]
    fn hidden_and_mime_rules() {
        assert!(is_hidden_path(Path::new("/x/.git")));
        assert!(!is_hidden_path(Path::new("/x/src")));
        assert!(is_image_previewable("png"));
        assert!(is_image_previewable("webp"));
        assert!(!is_image_previewable("svg")); // svg 防 XSS
        assert!(!is_image_previewable("html"));
        assert!(is_text_previewable("md"));
        assert!(is_text_previewable("rs"));
        assert_eq!(mime_for_ext("png"), "image/png");
        assert_eq!(mime_for_ext("unknown-ext"), "application/octet-stream");
    }

    #[test]
    fn workdir_validation() {
        let wd = tmp_workdir("valid");
        assert!(validate_workdir(&wd).is_ok());
        let file = wd.join("f.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(validate_workdir(&file).is_err()); // 文件不是目录
        assert!(validate_workdir(&wd.join("nope")).is_err());
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[test]
    fn atomic_write_rules() {
        let wd = tmp_workdir("atomic");
        let target = wd.join("a.txt");
        // 新文件：无需 overwrite。
        atomic_write(&target, b"one", false).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "one");
        // 已存在 + overwrite=false → AlreadyExists，内容不变。
        let err = atomic_write(&target, b"two", false).unwrap_err();
        assert_eq!(err.code(), "file-exists");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "one");
        // overwrite=true → 覆盖。
        atomic_write(&target, b"two", true).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "two");
        // 目标是目录 → WrongKind，不碰目录。
        let dir = wd.join("d");
        std::fs::create_dir(&dir).unwrap();
        assert_eq!(atomic_write(&dir, b"x", true).unwrap_err().code(), "wrong-file-kind");
        assert!(dir.is_dir());
        let _ = std::fs::remove_dir_all(&wd);
    }
}
