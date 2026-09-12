//! MCP 插件目录扫描与批准接入(两段式,2026-09-02 用户批准)。
//! 自 webadmin/mcp.rs 机械移出(2026-09-12,追 ADR-0048 手法);行为零变化。
//!
//! 目录约定:MCP 插件(可执行文件)放 `<mcp.json 同级>/mcp/`;官方随包插件
//! 位于安装目录 `plugins/`(exe 同级,v0.0.4 起随包发布;升级链路把它装到
//! exe 同级而非数据目录——2026-09-03 修复:扫描/批准同样认该目录,否则
//! 「随包」对在线升级用户不可见)。两处候选均以 `--self-describe` 参数打印
//! 声明 JSON 识别(识别过程会运行候选文件——数据目录是用户手动放入=安装
//! 意图,随包目录随官方主程序一同安装=同等安装意图;正式激活仍以「批准
//! 接入」落盘 mcp.json 为准,显式批准=安装,ADR-0005/0006/0017)。
//! ADR-0035:该「识别即执行」面在管理 UI 显式披露——扫描前一次性确认 +
//! 结果对话框常驻说明;服务端响应 note 保留同款措辞。
//! 同名候选以数据目录(用户手动放置)优先。

use super::lifecycle::{
    build_candidate_entry, read_tombstones, remove_tombstone, write_plugin_manifest,
};
use super::{
    AdminConfig, bad_request, conflict, internal, mcp_file_or_error, not_found, respond_or_fail,
    run_mcp_sync, write_mcp_servers,
};
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use bm_core::ports::mcp_admin::read_mcp_servers;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn mcp_plugins_dir(path: &Path) -> PathBuf {
    path.parent()
        .map(|d| d.join("mcp"))
        .unwrap_or_else(|| path.to_path_buf())
}

/// 运行单个候选的 --self-describe(5s 超时;stdin 接 null 防候选挂住;
/// 超时/退出失败/输出无 JSON = 非候选)。
async fn self_describe(path: &Path) -> Option<Value> {
    let spawn_result = tokio::process::Command::new(path)
        .arg("--self-describe")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match spawn_result {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "候选文件 spawn 失败");
            return None;
        }
    };
    let mut stdout = child.stdout.take()?;
    let mut out = Vec::new();
    let wait = async {
        use tokio::io::AsyncReadExt;
        let _ = stdout.read_to_end(&mut out).await;
        let _ = child.wait().await;
    };
    if tokio::time::timeout(std::time::Duration::from_secs(5), wait)
        .await
        .is_err()
    {
        return None;
    }
    let text = String::from_utf8_lossy(&out);
    for line in text.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line.trim())
            && v.get("name").and_then(Value::as_str).is_some()
        {
            return Some(v);
        }
    }
    eprintln!(
        "[mcp-candidates] 候选 {} 自描述输出 {} 字节,无有效声明行;前 160 字节: {:?}",
        path.display(),
        out.len(),
        text.get(..160).unwrap_or(&text)
    );
    None
}

fn candidate_is_executable(path: &Path) -> bool {
    #[cfg(windows)]
    {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("exe") | Some("cmd") | Some("bat")
        )
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
}

/// 扫一个候选目录:可执行文件 × --self-describe 成功(声明须带非空 name)
/// → (路径, 声明)。扫描/批准/播种三处共用同一读目录-筛文件-自述骨架。
pub(super) async fn scan_candidates(dir: &Path) -> std::io::Result<Vec<(PathBuf, Value)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let p = entry.path();
        if !p.is_file() || !candidate_is_executable(&p) {
            continue;
        }
        if let Some(decl) = self_describe(&p).await
            && decl["name"].as_str().is_some_and(|n| !n.is_empty())
        {
            out.push((p, decl));
        }
    }
    Ok(out)
}

/// 候选清单条目 JSON(数据目录/随包两源共用形状)。
fn candidate_json(
    p: &Path,
    decl: &Value,
    registered: &[String],
    tombstones: &[String],
    source: &str,
) -> Value {
    let name = decl["name"].as_str().unwrap_or_default();
    json!({
        "file": p.display().to_string(),
        "name": name,
        "title": decl.get("title").cloned().unwrap_or(json!("")),
        "description": decl.get("description").cloned().unwrap_or(json!("")),
        "registered": registered.iter().any(|r| r == name),
        "tombstoned": tombstones.iter().any(|t| t == name),
        "source": source,
    })
}

/// POST /admin/mcp/candidates:扫描插件目录,返回可批准接入的候选清单
/// (含已在 mcp.json 中的标记,便于前端过滤)。
pub async fn mcp_candidates(State(cfg): State<AdminConfig>) -> Response {
    let path = respond_or_fail!(mcp_file_or_error(&cfg));
    let dir = mcp_plugins_dir(&path);
    respond_or_fail!(
        std::fs::create_dir_all(&dir).map_err(|e| internal(format!("插件目录创建失败: {e}")))
    );
    let registered: Vec<String> = read_mcp_servers(&path)
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s["name"].as_str().map(String::from))
        .collect();
    // ADR-0023 墓碑名单:候选若在册,前端提示「批准即恢复」
    let tombstones = read_tombstones(&cfg.data_dir);
    let mut candidates: Vec<Value> = Vec::new();
    for (p, decl) in respond_or_fail!(
        scan_candidates(&dir)
            .await
            .map_err(|e| internal(format!("插件目录读取失败: {e}")))
    ) {
        candidates.push(candidate_json(&p, &decl, &registered, &tombstones, "data"));
    }
    // 官方随包目录(exe 同级 plugins/):随包插件免手动拷贝即可被发现;
    // 同名候选以数据目录优先(用户手动放置覆盖官方包)。
    if let Some(bundled) = &cfg.bundled_plugins_dir {
        for (p, decl) in scan_candidates(bundled).await.unwrap_or_default() {
            let name = decl["name"].as_str().unwrap_or_default();
            if candidates.iter().any(|c| c["name"] == json!(name)) {
                continue;
            }
            candidates.push(candidate_json(
                &p,
                &decl,
                &registered,
                &tombstones,
                "bundled",
            ));
        }
    }
    Json(json!({
        "ok": true,
        "dir": dir.display().to_string(),
        "bundled_dir": cfg
            .bundled_plugins_dir
            .as_ref()
            .map(|d| json!(d.display().to_string()))
            .unwrap_or(Value::Null),
        "candidates": candidates,
        "note": "扫描会以 --self-describe 运行候选目录内可执行文件(数据目录 mcp/ 与官方随包 plugins/)以读取自报声明;批准后才落盘 mcp.json 并上线",
    }))
    .into_response()
}

/// POST /admin/mcp/approve:批准候选接入。body {"name": "..."}。
/// 落盘两处:mcp.json 条目(command=候选路径,args 用声明模板替换
/// {config_file} 为数据目录配置路径)+ manifests/<name>.manifest.json
/// (设置页配置表单的声明来源);随后自动热重载上线(ADR-0023,热重载
/// 按钮保留作手动保险),并清除该名墓碑(被卸载/删除过的官方插件由此恢复)。
pub async fn mcp_approve(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let path = respond_or_fail!(mcp_file_or_error(&cfg));
    let dir = mcp_plugins_dir(&path);
    let want_name = body["name"].as_str().unwrap_or_default().to_string();
    if want_name.is_empty() {
        return bad_request("缺少 name");
    }
    // 在候选目录(数据目录 mcp/ 优先,官方随包 plugins/ 次之)内找到声明
    // name 匹配的候选(目录限定,防路径逃逸)
    let mut search_dirs: Vec<PathBuf> = vec![dir];
    if let Some(bundled) = &cfg.bundled_plugins_dir {
        search_dirs.push(bundled.clone());
    }
    let mut found: Option<(PathBuf, Value)> = None;
    for search_dir in &search_dirs {
        found = scan_candidates(search_dir)
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|(_, d)| d["name"].as_str() == Some(want_name.as_str()));
        if found.is_some() {
            break;
        }
    }
    let Some((file, decl)) = found else {
        return not_found(format!("候选目录中没有自声明 name={want_name} 的候选"));
    };

    // 条目构造+manifest 双写(与启动播种同一 helper,形状天然一致)
    let entry = respond_or_fail!(
        build_candidate_entry(&path, &file, &decl, &want_name),
        bad_request
    );

    let mut servers = respond_or_fail!(read_mcp_servers(&path), internal);
    if servers
        .iter()
        .any(|s| s["name"].as_str() == Some(want_name.as_str()))
    {
        return conflict(format!("MCP server '{want_name}' 已存在"));
    }
    servers.push(entry.clone());
    respond_or_fail!(write_mcp_servers(&path, &servers), internal);
    write_plugin_manifest(&path, &want_name, &decl);

    // ADR-0023:显式批准=安装意图最高级——清墓碑(被卸载/删除过的官方
    // 插件由此恢复)+ 立即热重载上线;失败不回滚落盘,可手动重试
    let tombstone_warn = remove_tombstone(&cfg.data_dir, &want_name).err();
    let (reload_ok, reload_payload) = match run_mcp_sync(&cfg).await {
        Ok(o) => {
            let tools = o
                .note_loaded
                .iter()
                .find(|s| s["name"].as_str() == Some(want_name.as_str()))
                .and_then(|s| s.get("tools").cloned())
                .unwrap_or(Value::Null);
            (
                true,
                json!({ "ok": true, "tools": tools, "failed": o.failed }),
            )
        }
        Err((_, m)) => (false, json!({ "ok": false, "skipped": m })),
    };
    let note = if reload_ok {
        "已批准并自动上线;「重载 MCP」按钮保留作手动保险".to_string()
    } else {
        format!(
            "已落盘,但自动上线未完成:{};可点「重载 MCP」重试",
            reload_payload["skipped"].as_str().unwrap_or("未知")
        )
    };
    let note = match &tombstone_warn {
        Some(w) => format!("{note};⚠ 墓碑清除失败:{w}(重启后该官方插件可能再次被移除,请重试批准)"),
        None => note,
    };
    Json(json!({
        "ok": true,
        "entry": entry,
        "reload": reload_payload,
        "note": note,
    }))
    .into_response()
}
