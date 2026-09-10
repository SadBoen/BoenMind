//! 工作区文件浏览(X-01 先例:组件白名单 + 拒链 + realpath 包含)与
//! 全盘目录浏览(工作目录选择器)/改名/下载/删除/新建目录。

use super::{AdminConfig, admin_error};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

/// 相对路径安全解析:仅接受 Normal 段;逐级拒符号链接;canonicalize 后
/// 必须仍位于 workspace root 内。rel 为空 = 根。
fn safe_resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.starts_with(['/', '\\']) || rel.as_bytes().get(1) == Some(&b':') {
        return Err("拒绝绝对路径".to_string());
    }
    let rel = rel.trim_start_matches(['/', '\\']);
    let mut cur = root.to_path_buf();
    if !rel.is_empty() {
        for seg in Path::new(rel).components() {
            match seg {
                Component::Normal(s) => cur.push(s),
                _ => return Err("路径含非法段(拒绝 .. 与绝对路径)".to_string()),
            }
        }
        // 逐级拒链:从 root 下第一级起检查(根自身由启动方保证)
        let mut probe = root.to_path_buf();
        for seg in Path::new(rel).components() {
            if let Component::Normal(s) = seg {
                probe.push(s);
                let meta =
                    std::fs::symlink_metadata(&probe).map_err(|_| "路径不存在".to_string())?;
                if meta.file_type().is_symlink() {
                    return Err("拒绝符号链接".to_string());
                }
            }
        }
    }
    // realpath 包含校验(末端可不存在于 list 场景;此处两场景都要求存在)
    let canon_root = std::fs::canonicalize(root).map_err(|_| "工作区根不可用".to_string())?;
    let canon = std::fs::canonicalize(&cur).map_err(|_| "路径不存在".to_string())?;
    if !canon.starts_with(&canon_root) {
        return Err("路径越出工作区".to_string());
    }
    Ok(canon)
}

#[derive(serde::Deserialize)]
pub struct FsPathParams {
    #[serde(default)]
    pub path: String,
}

pub async fn fs_list(State(cfg): State<AdminConfig>, Query(p): Query<FsPathParams>) -> Response {
    let dir = match safe_resolve(&cfg.workspace_root, &p.path) {
        Ok(d) => d,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    if !dir.is_dir() {
        return admin_error(StatusCode::BAD_REQUEST, "目标不是目录");
    }
    let mut entries = Vec::new();
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => {
            return admin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("读取目录失败: {e}"),
            );
        }
    };
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        // 目录内容里的符号链接不跟随:显示为 file 但不带 size(读取会被拒链)
        let is_dir = meta.is_dir();
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "kind": if is_dir { "dir" } else { "file" },
            "size": if is_dir { Value::Null } else { json!(meta.len()) },
        }));
    }
    entries.sort_by(|a, b| {
        let ka = if a["kind"] == "dir" { 0 } else { 1 };
        let kb = if b["kind"] == "dir" { 0 } else { 1 };
        ka.cmp(&kb).then_with(|| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
        })
    });
    Json(json!({
        "path": p.path,
        "entries": entries,
        "root": cfg.workspace_root.display().to_string(),
    }))
    .into_response()
}

pub async fn fs_file(State(cfg): State<AdminConfig>, Query(p): Query<FsPathParams>) -> Response {
    let file = match safe_resolve(&cfg.workspace_root, &p.path) {
        Ok(f) => f,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    let Ok(meta) = std::fs::metadata(&file) else {
        return admin_error(StatusCode::NOT_FOUND, "文件不存在");
    };
    if meta.is_dir() {
        return admin_error(StatusCode::BAD_REQUEST, "目标是目录,请先展开目录树");
    }
    if meta.len() > cfg.limits.get().fs_preview_max_bytes {
        return admin_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "文件超过预览上限({}KB > {}KB)",
                meta.len() / 1024,
                cfg.limits.get().fs_preview_max_bytes / 1024
            ),
        );
    }
    match std::fs::read(&file) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Json(json!({
                "path": p.path,
                "name": file.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
                "size": meta.len(),
                "content": text,
            }))
            .into_response(),
            Err(_) => admin_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "二进制文件不支持预览(仅 UTF-8 文本)",
            ),
        },
        Err(e) => admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("读取失败: {e}")),
    }
}

/// W7 反馈:目录树右键菜单——重命名(路径防护同 X-01;新名校验)。
/// 新条目名校验(rename/mkdir 共用):非空、≤200、拒 `.`/`..`、拒路径分隔。
/// `kind` =「文件」/「目录」,进错误文案。
fn validate_entry_name(
    body: &serde_json::Value,
    kind: &str,
) -> Result<String, (StatusCode, String)> {
    let Some(name) = body["name"].as_str().map(|s| s.trim()) else {
        return Err((StatusCode::BAD_REQUEST, "name 必须是字符串".into()));
    };
    if name.is_empty() || name.len() > 200 || name == "." || name == ".." {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("name 必须是非空{kind}名(≤200 字符,不含路径分隔)"),
        ));
    }
    if name.contains(['/', '\\']) {
        return Err((StatusCode::BAD_REQUEST, "name 不允许包含路径分隔符".into()));
    }
    Ok(name.to_string())
}

pub async fn fs_rename(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let Some(path) = body["path"].as_str() else {
        return admin_error(StatusCode::BAD_REQUEST, "path 必须是字符串");
    };
    let target = match safe_resolve(&cfg.workspace_root, path) {
        Ok(t) => t,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    let new_name = match validate_entry_name(&body, "文件") {
        Ok(n) => n,
        Err((code, msg)) => return admin_error(code, msg),
    };
    let Some(parent) = target.parent() else {
        return admin_error(StatusCode::BAD_REQUEST, "目标无父目录");
    };
    let new_path = parent.join(&new_name);
    if new_path.exists() {
        return admin_error(StatusCode::CONFLICT, format!("「{new_name}」已存在"));
    }
    match std::fs::rename(&target, &new_path) {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("重命名失败: {e}"),
        ),
    }
}

/// W7 反馈:目录树右键菜单——下载(单文件原样)与打包下载(文件夹 zip)。
/// 仅工作区内(safe_resolve 防逃逸);总量守门 256MB / 5000 条目。
pub async fn fs_download(
    State(cfg): State<AdminConfig>,
    Query(p): Query<FsPathParams>,
) -> Response {
    let target = match safe_resolve(&cfg.workspace_root, &p.path) {
        Ok(t) => t,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    let Ok(meta) = std::fs::metadata(&target) else {
        return admin_error(StatusCode::NOT_FOUND, "路径不存在");
    };
    let download_name = if meta.is_dir() {
        target
            .file_name()
            .map(|n| format!("{}.zip", n.to_string_lossy()))
            .unwrap_or_else(|| "workspace.zip".to_string())
    } else {
        target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "download".to_string())
    };
    let content_type = if meta.is_dir() {
        "application/zip"
    } else {
        "application/octet-stream"
    };
    let bytes = if meta.is_dir() {
        match zip_dir(
            &target,
            cfg.limits.get().fs_download_max_entries,
            cfg.limits.get().fs_download_max_bytes,
        ) {
            Ok(b) => b,
            Err(e) => {
                return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("打包失败: {e}"));
            }
        }
    } else {
        if meta.len() > cfg.limits.get().fs_download_max_bytes {
            return admin_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "文件超过下载上限({}MB)",
                    cfg.limits.get().fs_download_max_bytes / 1024 / 1024
                ),
            );
        }
        match std::fs::read(&target) {
            Ok(b) => b,
            Err(e) => {
                return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("读取失败: {e}"));
            }
        }
    };
    // Content-Disposition:ASCII 兜底 + RFC 5987 UTF-8(中文文件名)
    let ascii_name: String = download_name
        .chars()
        .map(|c| if c.is_ascii() { c } else { '_' })
        .collect();
    let mut resp = (StatusCode::OK, bytes).into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&format!(
        "attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{}",
        utf8_percent_encode(&download_name)
    )) {
        resp.headers_mut()
            .insert(axum::http::header::CONTENT_DISPOSITION, v);
    }
    if let Ok(v) = axum::http::HeaderValue::from_str(content_type) {
        resp.headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, v);
    }
    resp
}

fn utf8_percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 2026-09-07 目录树批次:删除(工作区文件树右键,支持多选批量)。
/// 仅工作区内:逐条 safe_resolve 防逃逸(天然拒 `..`/绝对路径/符号链接);
/// 目录整棵递归删;永久删除不进回收站,防误删由前端确认弹窗承担。
pub async fn fs_delete(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let Some(paths) = body["paths"].as_array() else {
        return admin_error(StatusCode::BAD_REQUEST, "paths 必须是字符串数组");
    };
    if paths.is_empty() {
        return admin_error(StatusCode::BAD_REQUEST, "paths 不能为空");
    }
    if paths.len() > cfg.limits.get().fs_delete_batch_max {
        return admin_error(
            StatusCode::BAD_REQUEST,
            format!("单次最多删除 {} 项", cfg.limits.get().fs_delete_batch_max),
        );
    }
    // 规范化去重 + 祖孙归并:多选可能同时含目录与其子项(如 ["a","a/b.txt"]),
    // 父目录整棵删后子项必然 NotFound;先剔除被父路径覆盖的冗余条目并合并
    // 重复项,批量结果才不会混入必然失败的记录。
    let norm = |s: &str| -> String {
        s.trim()
            .trim_start_matches(['/', '\\'])
            .trim_end_matches('/')
            .to_string()
    };
    let mut uniq: Vec<String> = Vec::new();
    for p in paths {
        if let Some(raw) = p.as_str() {
            let s = norm(raw);
            if !s.is_empty() && !uniq.contains(&s) {
                uniq.push(s);
            }
        }
    }
    let rels: Vec<String> = uniq
        .iter()
        .filter(|s| {
            !uniq
                .iter()
                .any(|r| r != *s && s.starts_with(format!("{r}/").as_str()))
        })
        .cloned()
        .collect();
    let rel_set: std::collections::HashSet<&str> = rels.iter().map(|s| s.as_str()).collect();
    let mut results = Vec::new();
    // 本批已实际处理(或被吸收)的条目:保证重复项只删一次、结果按入参序
    let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
    for p in paths {
        let Some(rel) = p.as_str().map(|s| s.trim()) else {
            results.push(json!({ "path": Value::Null, "ok": false, "error": "路径必须是字符串" }));
            continue;
        };
        if rel.is_empty() {
            results.push(json!({ "path": rel, "ok": false, "error": "拒绝删除工作区根" }));
            continue;
        }
        let reln = norm(rel);
        if !rel_set.contains(reln.as_str()) || !done.insert(reln.clone()) {
            // 被父目录条目吸收,或本批重复出现:随父目录/首次删除收口,不再单列
            continue;
        }
        let target = match safe_resolve(&cfg.workspace_root, &reln) {
            Ok(t) => t,
            Err(e) => {
                results.push(json!({ "path": reln, "ok": false, "error": e }));
                continue;
            }
        };
        let meta = match std::fs::symlink_metadata(&target) {
            Ok(m) => m,
            Err(_) => {
                results.push(json!({ "path": reln, "ok": false, "error": "路径不存在" }));
                continue;
            }
        };
        let r = if meta.is_dir() {
            std::fs::remove_dir_all(&target)
        } else {
            std::fs::remove_file(&target)
        };
        match r {
            Ok(_) => results.push(json!({ "path": reln, "ok": true })),
            Err(e) => results.push(json!({
                "path": reln, "ok": false, "error": format!("删除失败: {e}")
            })),
        }
    }
    let deleted = results
        .iter()
        .filter(|r| r["ok"].as_bool() == Some(true))
        .count();
    Json(json!({
        "ok": deleted == results.len(),
        "deleted": deleted,
        "results": results,
    }))
    .into_response()
}

// ---- 任意目录浏览(只读;设置页工作目录选择器专用)------------------------
// 与 /fs/list 的工作区沙箱浏览互补:「添加工作目录」的路径选择器需要覆盖
// 全盘任意绝对路径。守门:只列目录、只报名字,零文件内容零大小;上限 1000 条。

/// GET /admin/fs/browse?path=<绝对路径;空 = 根视图(Windows 盘符 / Unix /)>
pub async fn fs_browse(State(cfg): State<AdminConfig>, Query(p): Query<FsPathParams>) -> Response {
    let raw = p.path.trim().to_string();
    if raw.is_empty() {
        return Json(json!({
            "path": "",
            "parent": Value::Null,
            "entries": browse_roots(),
            "truncated": false,
        }))
        .into_response();
    }
    let path = std::path::Path::new(&raw);
    if !path.exists() {
        return admin_error(StatusCode::BAD_REQUEST, format!("路径不存在: {raw}"));
    }
    let canon = match std::fs::canonicalize(path) {
        Ok(c) => c,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, format!("路径解析失败: {e}")),
    };
    if !canon.is_dir() {
        return admin_error(StatusCode::BAD_REQUEST, "目标不是目录");
    }
    let pretty =
        |p: &std::path::Path| crate::workspace_admin::pretty_normalized(p.display().to_string());
    let mut entries = Vec::new();
    let mut truncated = false;
    let mut unreadable = false;
    match std::fs::read_dir(&canon) {
        Ok(rd) => {
            for entry in rd.flatten() {
                if entries.len() >= cfg.limits.get().fs_browse_max_entries {
                    truncated = true;
                    break;
                }
                // 只收目录:实体目录直收;符号链接/junction 跟随判一次
                // (选择器允许经由链接进入目录;文件一律不出现)
                let Ok(ft) = entry.file_type() else { continue };
                let is_dir = if ft.is_dir() {
                    true
                } else if ft.is_symlink() {
                    std::fs::metadata(entry.path())
                        .map(|m| m.is_dir())
                        .unwrap_or(false)
                } else {
                    false
                };
                if !is_dir {
                    continue;
                }
                entries.push(json!({
                    "name": entry.file_name().to_string_lossy(),
                    "path": pretty(&entry.path()),
                }));
            }
        }
        Err(_) => {
            // 无权限等读取失败:返回空列表而非 500,用户可回上级继续浏览
            unreadable = true;
        }
    }
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    Json(json!({
        "path": pretty(&canon),
        "parent": parent_json(&canon),
        "entries": entries,
        "truncated": truncated,
        "note": if unreadable { "目录不可读或为空" } else { "" },
    }))
    .into_response()
}

/// 根视图条目:Windows 枚举现存盘符;Unix 统一一条 /。
fn browse_roots() -> Vec<Value> {
    #[cfg(windows)]
    {
        (b'A'..=b'Z')
            .map(|c| format!("{}:\\", c as char))
            .filter(|d| std::path::Path::new(d).is_dir())
            .map(|d| json!({ "name": d, "path": d }))
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![json!({ "name": "/", "path": "/" })]
    }
}

/// 上级目录;盘符根/文件系统根无上级,返回 null。
fn parent_json(canon: &std::path::Path) -> Value {
    canon
        .parent()
        .map(|p| {
            let t = crate::workspace_admin::pretty_normalized(p.display().to_string());
            if t.is_empty() { Value::Null } else { json!(t) }
        })
        .unwrap_or(Value::Null)
}

/// 2026-09-07 目录树批次:新建目录(工作目录选择器「新建文件夹」配套)。
/// 与 /fs/browse 同一全盘信任域:browse 全盘只读浏览,本端点是其唯一配套
/// 写例外——只建空目录、单级、零内容;name 校验与 fs_rename 同规;门户墙保护。
pub async fn fs_mkdir(State(_cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let Some(parent) = body["parent"].as_str().map(|s| s.trim()) else {
        return admin_error(StatusCode::BAD_REQUEST, "parent 必须是字符串");
    };
    if parent.is_empty() {
        return admin_error(StatusCode::BAD_REQUEST, "请先进入某个盘符或目录再新建");
    }
    let name = match validate_entry_name(&body, "目录") {
        Ok(n) => n,
        Err((code, msg)) => return admin_error(code, msg),
    };
    let parent_path = std::path::Path::new(parent);
    if !parent_path.exists() {
        return admin_error(StatusCode::BAD_REQUEST, format!("父目录不存在: {parent}"));
    }
    let canon_parent = match std::fs::canonicalize(parent_path) {
        Ok(c) => c,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, format!("父目录解析失败: {e}")),
    };
    if !canon_parent.is_dir() {
        return admin_error(StatusCode::BAD_REQUEST, "父路径不是目录");
    }
    let target = canon_parent.join(&name);
    if target.exists() {
        return admin_error(StatusCode::CONFLICT, format!("「{name}」已存在"));
    }
    match std::fs::create_dir(&target) {
        Ok(_) => Json(json!({
            "ok": true,
            "path": crate::workspace_admin::pretty_normalized(target.display().to_string()),
        }))
        .into_response(),
        Err(e) => admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("新建目录失败: {e}"),
        ),
    }
}

/// 递归打包目录为 zip(内存;守门条目/总量上限走 limits,W10)。
fn zip_dir(dir: &std::path::Path, max_entries: usize, max_bytes: u64) -> Result<Vec<u8>, String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut count = 0usize;
        let mut total = 0u64;
        #[allow(clippy::too_many_arguments)] // zip 递归走签名面,钳制值不私挂全局
        fn walk(
            zip: &mut zip::ZipWriter<&mut std::io::Cursor<Vec<u8>>>,
            options: &zip::write::FileOptions,
            prefix: &str,
            base: &std::path::Path,
            count: &mut usize,
            total: &mut u64,
            max_entries: usize,
            max_bytes: u64,
        ) -> Result<(), String> {
            for entry in
                std::fs::read_dir(base).map_err(|e| format!("read_dir {}: {e}", base.display()))?
            {
                let entry = entry.map_err(|e| format!("{e}"))?;
                let file_type = entry.file_type().map_err(|e| format!("{e}"))?;
                if file_type.is_symlink() {
                    // 安全防护:工作区打包下载不跟随符号链接,防止链接到工作区外敏感文件
                    continue;
                }
                let path = entry.path();
                let rel = if prefix.is_empty() {
                    entry.file_name().to_string_lossy().to_string()
                } else {
                    format!("{prefix}/{}", entry.file_name().to_string_lossy())
                };
                if file_type.is_dir() {
                    zip.add_directory(rel.clone(), *options)
                        .map_err(|e| format!("{e}"))?;
                    walk(
                        zip,
                        options,
                        &rel,
                        &path,
                        count,
                        total,
                        max_entries,
                        max_bytes,
                    )?;
                } else {
                    *count += 1;
                    if *count > max_entries {
                        return Err(format!("条目超过 {max_entries},拒绝打包"));
                    }
                    let data = std::fs::read(&path).map_err(|e| format!("read {rel}: {e}"))?;
                    *total += data.len() as u64;
                    if *total > max_bytes {
                        return Err("总量超过下载上限,拒绝打包".into());
                    }
                    zip.start_file(rel.clone(), *options)
                        .map_err(|e| format!("{e}"))?;
                    std::io::Write::write_all(zip, &data).map_err(|e| format!("{e}"))?;
                }
            }
            Ok(())
        }
        walk(
            &mut zip,
            &options,
            "",
            dir,
            &mut count,
            &mut total,
            max_entries,
            max_bytes,
        )?;
        zip.finish().map_err(|e| format!("{e}"))?;
    }
    Ok(buf.into_inner())
}
