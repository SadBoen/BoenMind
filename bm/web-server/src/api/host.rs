//! host.* handler 领域子模块（api.rs 拆分）。
//! host 工作目录作用域文件面 + 通用目录/隐藏路径判断（全部经 host_fs）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::api::AppState;
use host_fs::{self, HostFsError};
use crate::rpc::{err, err_with_details, ok};

pub(super) fn host_pick_directory(state: &AppState) -> Value {
    ok(json!({ "path": state.host_cwd }))
}

/// host.listWorkdir（特权）：列工作目录（或其中相对子目录）的条目。
/// 懒加载契约：前端每次展开传 path（相对路径，空 = workdir 根）。
/// 条目 {name, path(相对), isDir, size, hidden}；目录优先 + 名字排序；
/// 单目录上限 2000（超限截断 + truncated 标记）。未设置 workdir → workdir-not-configured。
pub(super) fn host_list_workdir(state: &AppState, payload: Value) -> Value {
    let Some(wd) = host_workdir(state) else {
        return err("workdir-not-configured", "work directory not set (settings → 工作目录)");
    };
    let rel = payload.get("path").and_then(Value::as_str).unwrap_or("");
    let target = match host_fs::resolve_in_workdir(&wd, rel) {
        Ok(t) => t,
        Err(e) => return hostfs_err(e),
    };
    if !target.is_dir() {
        return err_with_details(
            "wrong-file-kind",
            "not a directory",
            json!({ "path": rel }),
        );
    }
    let read = match std::fs::read_dir(&target) {
        Ok(rd) => rd,
        Err(e) => {
            return err_with_details(
                "directory-unreadable",
                format!("cannot read directory: {e}"),
                json!({ "path": rel }),
            );
        }
    };
    let mut entries: Vec<Value> = Vec::new();
    for item in read.flatten() {
        let path = item.path();
        let is_dir = path.is_dir();
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        entries.push(json!({
            "name": item.file_name().to_string_lossy(),
            "path": rel_from_workdir(&wd, &path),
            "isDir": is_dir,
            "size": size,
            "hidden": host_fs::is_hidden_path(&path),
        }));
        if entries.len() >= host_fs::MAX_ENTRIES_PER_DIR {
            break;
        }
    }
    // 目录优先，其后按名（大小写不敏感）。
    entries.sort_by(|a, b| {
        let ad = a["isDir"].as_bool().unwrap_or(false);
        let bd = b["isDir"].as_bool().unwrap_or(false);
        if ad != bd {
            return bd.cmp(&ad);
        }
        let an = a["name"].as_str().unwrap_or("");
        let bn = b["name"].as_str().unwrap_or("");
        an.to_lowercase().cmp(&bn.to_lowercase())
    });
    let truncated = entries.len() >= host_fs::MAX_ENTRIES_PER_DIR;
    ok(json!({
        "path": rel,
        "workdir": wd.to_string_lossy().replace('\\', "/"),
        "entries": entries,
        "truncated": truncated,
    }))
}

/// host.readFile（特权）：读 UTF-8 文本（md/txt/rs/... 均可，按扩展名不管，
/// 内容须合法 UTF-8 且 ≤ 2MiB）。图片/二进制 → 前端走 download 端点。
pub(super) fn host_read_file(state: &AppState, payload: Value) -> Value {
    let Some(wd) = host_workdir(state) else {
        return err("workdir-not-configured", "work directory not set (settings → 工作目录)");
    };
    let Some(rel) = payload.get("path").and_then(Value::as_str) else {
        return err("bad-request", "missing path");
    };
    let target = match host_fs::resolve_in_workdir(&wd, rel) {
        Ok(t) => t,
        Err(e) => return hostfs_err(e),
    };
    if !target.exists() {
        return err_with_details("file-not-found", "file not found", json!({ "path": rel }));
    }
    if !target.is_file() {
        return err_with_details("wrong-file-kind", "not a file", json!({ "path": rel }));
    }
    let len = target.metadata().map(|m| m.len()).unwrap_or(0);
    if len > host_fs::MAX_TEXT_BYTES {
        return err_with_details(
            "file-too-large",
            format!("text file exceeds {}-byte limit; open as download instead", host_fs::MAX_TEXT_BYTES),
            json!({ "path": rel, "size": len, "limit": host_fs::MAX_TEXT_BYTES }),
        );
    }
    let bytes = match std::fs::read(&target) {
        Ok(b) => b,
        Err(e) => return err_with_details("file-io-error", format!("read failed: {e}"), json!({ "path": rel })),
    };
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            return err_with_details(
                "invalid-utf8",
                "file is not valid UTF-8 text; open as download instead",
                json!({ "path": rel }),
            )
        }
    };
    ok(json!({ "path": rel_from_workdir(&wd, &target), "content": content, "size": len }))
}

/// host.writeFile（特权）：原子写文本（tmp + rename）。`overwrite` 缺省 false：
/// 目标已存在 → file-exists（显式 overwrite:true 才覆盖）。拒绝写目录。
pub(super) fn host_write_file(state: &AppState, payload: Value) -> Value {
    let Some(wd) = host_workdir(state) else {
        return err("workdir-not-configured", "work directory not set (settings → 工作目录)");
    };
    let Some(rel) = payload.get("path").and_then(Value::as_str) else {
        return err("bad-request", "missing path");
    };
    let Some(content) = payload.get("content").and_then(Value::as_str) else {
        return err("bad-request", "missing content (string)");
    };
    let overwrite = payload.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
    if (content.len() as u64) > host_fs::MAX_TEXT_BYTES {
        return err_with_details(
            "file-too-large",
            format!("content exceeds {}-byte limit", host_fs::MAX_TEXT_BYTES),
            json!({ "path": rel, "limit": host_fs::MAX_TEXT_BYTES }),
        );
    }
    let target = match host_fs::resolve_in_workdir(&wd, rel) {
        Ok(t) => t,
        Err(e) => return hostfs_err(e),
    };
    if let Err(e) = host_fs::atomic_write(&target, content.as_bytes(), overwrite) {
        return hostfs_err(e);
    }
    ok(json!({ "path": rel_from_workdir(&wd, &target), "overwritten": overwrite }))
}

/// host.createWorkdirDirectory（特权）：在 workdir 内某目录下新建单段文件夹。
/// name 须单路径段（禁 / \ . .. 与 Windows 保留字符）；已存在 → directory-exists。
pub(super) fn host_create_workdir_directory(state: &AppState, payload: Value) -> Value {
    let Some(wd) = host_workdir(state) else {
        return err("workdir-not-configured", "work directory not set (settings → 工作目录)");
    };
    let Some(rel) = payload.get("path").and_then(Value::as_str) else {
        return err("bad-request", "missing path (parent directory)");
    };
    let Some(name) = payload.get("name").and_then(Value::as_str) else {
        return err("bad-request", "missing name");
    };
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.chars().any(|c| c.is_control() || ":\"*?<>|".contains(c))
    {
        return err(
            "bad-request",
            "host.createWorkdirDirectory requires a single valid path segment name",
        );
    }
    let parent = match host_fs::resolve_in_workdir(&wd, rel) {
        Ok(t) => t,
        Err(e) => return hostfs_err(e),
    };
    if !parent.is_dir() {
        return err_with_details("wrong-file-kind", "parent is not a directory", json!({ "path": rel }));
    }
    let dir = parent.join(name);
    if dir.exists() {
        return err_with_details("directory-exists", "directory already exists", json!({ "path": rel_from_workdir(&wd, &dir) }));
    }
    match std::fs::create_dir(&dir) {
        Ok(()) => ok(json!({ "path": rel_from_workdir(&wd, &dir) })),
        Err(e) => err_with_details(
            "directory-create-failed",
            format!("create directory failed: {e}"),
            json!({ "path": rel_from_workdir(&wd, &dir) }),
        ),
    }
}

/// 目录条目隐藏判定：`.` 前缀（Unix 惯例）+ Windows FILE_ATTRIBUTE_HIDDEN。
pub(super) fn is_hidden_path(p: &Path) -> bool {
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

/// host.listDirectory（特权）：列一个目录层级 + 祖先 breadcrumb。
/// 缺省 path = 家目录；不可读 → `directory-unreadable {path}`。
pub(super) fn host_list_directory(payload: Value) -> Value {
    use std::path::{Component, PathBuf};

    let raw = payload.get("path").and_then(Value::as_str);
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let home_str = home.to_string_lossy().to_string();
    let target: PathBuf = match raw {
        Some(p) if !p.is_empty() => {
            let p = PathBuf::from(p);
            if p.is_absolute() { p } else { home.join(p) }
        }
        _ => home,
    };

    // 目录必须存在且可读。
    let read = match std::fs::read_dir(&target) {
        Ok(rd) => rd,
        Err(e) => {
            return err_with_details(
                "directory-unreadable",
                format!("cannot read directory: {e}"),
                json!({ "path": target.to_string_lossy() }),
            );
        }
    };

    let mut entries: Vec<Value> = Vec::new();
    for item in read.flatten() {
        let path = item.path();
        entries.push(json!({
            "name": item.file_name().to_string_lossy(),
            "path": path.to_string_lossy(),
            "hidden": is_hidden_path(&path),
        }));
    }
    entries.sort_by(|a, b| {
        let an = a["name"].as_str().unwrap_or("");
        let bn = b["name"].as_str().unwrap_or("");
        an.to_lowercase().cmp(&bn.to_lowercase())
    });

    // crumbs：从根到当前目录每层一个 {name, path}（hidden 恒 false）。
    // Windows 上 Prefix("D:") + RootDir("\") 合并为一段 "D:\"。
    let mut crumbs: Vec<Value> = Vec::new();
    let mut acc = PathBuf::new();
    let mut pending_prefix: Option<String> = None;
    for comp in target.components() {
        match comp {
            Component::Prefix(_) => {
                acc.push(comp.as_os_str());
                pending_prefix = Some(acc.to_string_lossy().to_string());
            }
            Component::RootDir => {
                acc.push(comp.as_os_str());
                crumbs.push(json!({
                    "name": pending_prefix
                        .take()
                        .unwrap_or_else(|| acc.to_string_lossy().trim_end_matches(['/', '\\']).to_string()),
                    "path": acc.to_string_lossy(),
                    "hidden": false,
                }));
            }
            Component::Normal(seg) => {
                acc.push(seg);
                crumbs.push(json!({
                    "name": seg.to_string_lossy(),
                    "path": acc.to_string_lossy(),
                    "hidden": false,
                }));
            }
            _ => {}
        }
    }
    // 家目录缺省空 crumbs 时的兜底：至少一段。
    if crumbs.is_empty() {
        crumbs.push(json!({
            "name": home_str,
            "path": home_str,
            "hidden": false,
        }));
    }

    ok(json!({
        "path": target.to_string_lossy(),
        "home": home_str,
        "crumbs": crumbs,
        "entries": entries,
        "truncated": false,
    }))
}

/// host.createDirectory（特权）：name 须单路径段；已存在 → `directory-exists`；
/// 创建失败 → `directory-create-failed`。返回创建后的绝对路径。
pub(super) fn host_create_directory(payload: Value) -> Value {
    use std::path::PathBuf;

    let Some(path) = payload.get("path").and_then(Value::as_str) else {
        return err("bad-request", "missing path");
    };
    let Some(name) = payload.get("name").and_then(Value::as_str) else {
        return err("bad-request", "missing name");
    };
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return err(
            "bad-request",
            "host.createDirectory requires a single non-blank path segment name",
        );
    }
    let dir = PathBuf::from(path).join(name);
    if dir.exists() {
        return err_with_details(
            "directory-exists",
            "directory already exists",
            json!({ "path": dir.to_string_lossy() }),
        );
    }
    match std::fs::create_dir(&dir) {
        Ok(()) => ok(json!({ "path": dir.to_string_lossy() })),
        Err(e) => err_with_details(
            "directory-create-failed",
            format!("create directory failed: {e}"),
            json!({ "path": dir.to_string_lossy() }),
        ),
    }
}

/// 读取当前工作目录（settings host.workdir；缺失/空 → None）。
/// 文件管理器全部 FS 操作的服务端唯一事实源——不信任客户端随请求带来的路径。
pub fn host_workdir(state: &AppState) -> Option<PathBuf> {
    state
        .settings
        .lock()
        .unwrap()
        .get("host")
        .and_then(|v| v.get("workdir"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// 宿主工具（plugin-host-tools）的 workdir 源：从 AppState.settings 现读，
/// 与 [`host_workdir`] 同一条事实源（设置页改 workdir 后下一工具调用即时生效）。
#[derive(Clone)]
pub struct SettingsWorkdir(Arc<AppState>);

impl std::fmt::Debug for SettingsWorkdir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsWorkdir").finish_non_exhaustive()
    }
}

impl SettingsWorkdir {
    pub fn new(state: Arc<AppState>) -> Self {
        Self(state)
    }
}

impl bm_ports::WorkdirPort for SettingsWorkdir {
    fn current_workdir(&self) -> Option<PathBuf> {
        host_workdir(&self.0)
    }
}

/// host_fs 错误 → RPC 信封（{ok:false, error:{code,message,details}}）。
pub(super) fn hostfs_err(e: HostFsError) -> Value {
    err_with_details(
        e.code(),
        e.to_string(),
        json!({ "path": match &e {
            HostFsError::InvalidPath(p) | HostFsError::NotInside(p)
            | HostFsError::NotFound(p) | HostFsError::AlreadyExists(p)
            | HostFsError::WrongKind(p) | HostFsError::WorkdirInvalid(p) => p.clone(),
            _ => String::new(),
        } }),
    )
}

/// 目标相对 workdir 的 POSIX 风格路径（前端统一 / 分隔）。
/// 用字符串前缀剥离（Windows 下 canonical 路径可能带 `\\?\` / `//?/` 前缀而
/// read_dir 返回普通路径，Path::strip_prefix 会失败 → 归一化去前缀后按字符串比）。
pub(super) fn rel_from_workdir(wd: &Path, target: &Path) -> String {
    let norm = |s: &std::path::Path| {
        s.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string()
    };
    // 统一去掉 Windows extended-length 前缀（`\\?\` → `//?/`）。
    let strip_win_prefix = |s: String| {
        if let Some(rest) = s.strip_prefix("//?/") {
            rest.to_string()
        } else {
            s
        }
    };
    let wd_s = strip_win_prefix(norm(wd));
    let target_s = strip_win_prefix(norm(target));
    let stripped = target_s
        .strip_prefix(&wd_s)
        .and_then(|r| r.strip_prefix('/'))
        .unwrap_or(&target_s);
    stripped.to_string()
}
