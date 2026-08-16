//! 静态文件服务：SPA fallback 处理器。
//!
//! 两种形态：
//! - `--features embed`（服务器版）：前端产物内嵌进二进制，单文件自包含部署；
//! - `BOENMIND_WEB_DIR` 环境变量（便携版多文件形态）：从包内 `web/` 目录读磁盘
//!   文件（壳启动 bm-server 时设置；未设置时与旧行为一致——非 embed 一律 404）。

use axum::{
    extract::Request,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};

/// 便携版 web 目录（BOENMIND_WEB_DIR；None = 未启用磁盘形态）。
fn web_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("BOENMIND_WEB_DIR")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
}

/// 缓存策略：`/assets/` 下为 vite 内容哈希文件名（内容不变名不变），
/// 可长缓存；其余（index.html / favicon / docs 等）一律 no-cache——
/// 曾现便携版与开发版同源 127.0.0.1:17321 共用 WebView2 缓存导致
/// 加载旧页面/设置页异常（2026-08-16 模型提供商"打不开"排查定论）。
fn cache_control(path: &str) -> &'static str {
    if path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

/// 静态资源 + SPA fallback 处理器，挂在主 router 的 fallback 上，
/// 只处理 `/api` 未命中的 GET 请求（含 history 路由，如 `/settings`）。
pub async fn handle_static(request: Request) -> Response {
    if request.method() != Method::GET {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    // 便携版磁盘形态优先（无需 embed feature）
    if let Some(dir) = web_dir() {
        return serve_disk(&dir, request).await;
    }

    #[cfg(not(feature = "embed"))]
    {
        let _ = request;
        StatusCode::NOT_FOUND.into_response()
    }

    #[cfg(feature = "embed")]
    {
        use axum::{body::Body, http::header};

        let path = request.uri().path().trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };
        // SPA history 路由（如 /settings）回退到 index.html，content-type 固定 text/html
        let (data, mime) = match assets::Assets::get(path) {
            Some(file) => {
                let mime = mime_guess::from_path(path).first_or_octet_stream();
                (file.data.to_vec(), mime)
            }
            None => match assets::Assets::get("index.html") {
                Some(file) => (file.data.to_vec(), mime_guess::mime::TEXT_HTML),
                None => return StatusCode::NOT_FOUND.into_response(),
            },
        };
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        Response::builder()
            .header(header::CONTENT_TYPE, mime.as_ref())
            .header(header::CACHE_CONTROL, cache)
            .body(Body::from(data))
            .unwrap()
    }
}

#[cfg(feature = "embed")]
mod assets {
    use rust_embed::RustEmbed;

    /// 前端构建产物（`pnpm build` 输出）。路径相对 crate 根
    /// （`backend/crates/bm-server/`）→ 仓库根 `frontend/dist`。
    #[derive(RustEmbed)]
    #[folder = "../../../frontend/dist"]
    pub struct Assets;
}

/// 磁盘目录形态：读 `dir/<path>`，路径穿越防护 + SPA fallback。
async fn serve_disk(
    dir: &std::path::Path,
    request: Request,
) -> Response {
    use axum::{body::Body, http::header};

    let raw = request.uri().path().trim_start_matches('/');
    let raw = if raw.is_empty() { "index.html" } else { raw };
    // 归一化后仍含 `..` 的路径直接拒绝（防穿越到包外）
    let normalized = raw.split('/').fold(String::new(), |acc, seg| {
        if seg == "." || seg.is_empty() {
            acc
        } else {
            format!("{acc}/{seg}")
        }
    });
    if normalized.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = dir.join(normalized.trim_start_matches('/'));
    let cache = cache_control(&normalized);
    let data = tokio::fs::read(&path).await;
    match data {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(header::CACHE_CONTROL, cache)
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(_) => {
            // SPA fallback：未命中资源回退 index.html（history 路由）
            let index = dir.join("index.html");
            match tokio::fs::read(&index).await {
                Ok(bytes) => Response::builder()
                    .header(header::CONTENT_TYPE, mime_guess::mime::TEXT_HTML.as_ref())
                    .header(header::CACHE_CONTROL, "no-cache")
                    .body(Body::from(bytes))
                    .unwrap(),
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disk_mode_serves_file_and_spa_fallback() {
        use axum::body::Body;
        use axum::http::{Request as HttpRequest, Uri};

        // 临时 web 目录：index.html + assets/app.js
        let dir = std::env::temp_dir().join(format!("bm-webdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), b"<html>index</html>").unwrap();
        std::fs::write(dir.join("assets/app.js"), b"console.log(1)").unwrap();

        // 命中文件
        let req = HttpRequest::builder()
            .uri(Uri::from_static("/assets/app.js"))
            .body(Body::empty())
            .unwrap();
        let resp = serve_disk(&dir, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        assert_eq!(&body[..], b"console.log(1)");

        // SPA fallback：/settings → index.html
        let req = HttpRequest::builder()
            .uri(Uri::from_static("/settings"))
            .body(Body::empty())
            .unwrap();
        let resp = serve_disk(&dir, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        assert_eq!(&body[..], b"<html>index</html>");

        // 缓存策略：哈希资源长缓存；HTML no-cache（防 WebView2 缓存旧页面）
        let req = HttpRequest::builder()
            .uri(Uri::from_static("/assets/app.js"))
            .body(Body::empty())
            .unwrap();
        let resp = serve_disk(&dir, req).await;
        assert_eq!(
            resp.headers().get("cache-control").map(|v| v.to_str().unwrap()),
            Some("public, max-age=31536000, immutable")
        );
        let req = HttpRequest::builder()
            .uri(Uri::from_static("/index.html"))
            .body(Body::empty())
            .unwrap();
        let resp = serve_disk(&dir, req).await;
        assert_eq!(
            resp.headers().get("cache-control").map(|v| v.to_str().unwrap()),
            Some("no-cache")
        );

        // 路径穿越：/../secret.txt → 404
        let req = HttpRequest::builder()
            .uri(Uri::from_static("/../secret.txt"))
            .body(Body::empty())
            .unwrap();
        let resp = serve_disk(&dir, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
