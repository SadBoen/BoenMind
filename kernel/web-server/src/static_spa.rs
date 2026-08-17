//! 静态 SPA 服务（契约台账 §1 面 4）：兜底 200 / 405 / 403 / octet-stream。
//! boot 3 槽注入（面 6）：index.html 的 `<head>` 首部注入 `__DSH_BOOT__`。

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};

/// 扩展名 → MIME（台账 §1 面 4：未知扩展 octet-stream）。
fn mime_for(ext: &str) -> &'static str {
    match ext {
        "html" => "text/html",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// 越界判定：normalize(join(dist_root, pathname)) 必须仍在 dist_root 之下。
fn resolve_within_root(dist_root: &Path, pathname: &str) -> Option<PathBuf> {
    // 规范化：拒绝 .. 段越界
    let mut joined = dist_root.to_path_buf();
    for seg in pathname.split('/') {
        match seg {
            "" | "." => {}
            ".." => return None,
            s => joined.push(s),
        }
    }
    // 规范化后必须仍在 dist_root 下
    let canonical = joined.canonicalize().ok()?;
    let root = dist_root.canonicalize().ok()?;
    if canonical.starts_with(&root) {
        Some(canonical)
    } else {
        None
    }
}

fn response(status: StatusCode, content_type: &'static str, body: Vec<u8>) -> Response<Body> {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static(content_type));
    (status, headers, Body::from(body)).into_response()
}

/// SPA 静态处理器：GET/HEAD 之外 405；越界 403；miss 回退 index.html；boot 注入。
pub async fn static_handler(
    method: Method,
    uri: axum::http::Uri,
    dist_root: PathBuf,
    boot_json: Option<String>,
) -> Response<Body> {
    if method != Method::GET && method != Method::HEAD {
        return response(StatusCode::METHOD_NOT_ALLOWED, "text/plain", b"".to_vec());
    }
    let pathname = uri.path();
    let Some(target) = resolve_within_root(&dist_root, pathname) else {
        return response(StatusCode::FORBIDDEN, "text/plain", b"forbidden".to_vec());
    };

    if target.is_file() {
        let ext = target
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let content_type = mime_for(&ext);
        match std::fs::read(&target) {
            Ok(body) => return response(StatusCode::OK, content_type, body),
            Err(_) => {
                // 文件不可读：SPA 兜底（台账：miss 回退 index.html）
            }
        }
    }

    // SPA 兜底：回退 index.html（若存在）；boot 注入。
    let index = dist_root.join("index.html");
    match std::fs::read(&index) {
        Ok(mut body) => {
            if let Some(boot) = boot_json {
                if let Ok(html) = std::str::from_utf8(&body) {
                    let injection = format!(
                        "<script>window.__DSH_BOOT__ = {}</script>",
                        boot
                    );
                    let new_html = if let Some(head_end) = html.find("<head>") {
                        let pos = head_end + "<head>".len();
                        format!("{}{}{}", &html[..pos], injection, &html[pos..])
                    } else {
                        format!("{injection}{html}")
                    };
                    body = new_html.into_bytes();
                }
            }
            response(StatusCode::OK, "text/html", body)
        }
        Err(_) => response(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec()),
    }
}
