//! 服务器版静态文件服务。
//!
//! 仅 `--features embed` 编译：把前端构建产物（`frontend/dist`）内嵌进二进制，
//! 使 bm-server 成为自包含的单文件服务 —— 同源 serve SPA 与 `/api`，
//! 用户浏览器直接访问 `http://服务器IP:端口` 即可。
//! 桌面壳（Tauri 内嵌后端）不启用该 feature，行为与之前完全一致（一律 404）。

use axum::{
    extract::Request,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};

#[cfg(feature = "embed")]
mod assets {
    use rust_embed::RustEmbed;

    /// 前端构建产物（`pnpm build` 输出）。路径相对 crate 根
    /// （`backend/crates/bm-server/`）→ 仓库根 `frontend/dist`。
    #[derive(RustEmbed)]
    #[folder = "../../../frontend/dist"]
    pub struct Assets;
}

/// 静态资源 + SPA fallback 处理器，挂在主 router 的 fallback 上，
/// 只处理 `/api` 未命中的 GET 请求（含 history 路由，如 `/settings`）。
pub async fn handle_static(request: Request) -> Response {
    if request.method() != Method::GET {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
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
        Response::builder()
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(data))
            .unwrap()
    }
}
