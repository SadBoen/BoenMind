//! 日志查询 API（设置中心「日志」页）：从内存环形缓冲读最近日志，
//! 支持最低级别 / 关键字筛选与分页。进程内无落盘，重启即清空。

use axum::extract::Query;
use axum::Json;
use serde::Deserialize;

use crate::{ApiResult, log_buffer::LogBuffer};

#[derive(Deserialize)]
pub struct LogQuery {
    /// 最低级别：trace/debug/info/warn/error（>= 语义；缺省 = 全部）
    level: Option<String>,
    /// 关键字：target/message 包含即命中（大小写不敏感）
    q: Option<String>,
    /// 返回条数（默认 200，上限 2000）
    limit: Option<usize>,
    /// 跳过条数（配合 limit 翻页，从最新往旧数）
    offset: Option<usize>,
}

pub async fn get_logs(Query(q): Query<LogQuery>) -> ApiResult<Json<serde_json::Value>> {
    let limit = q.limit.unwrap_or(200).min(2000);
    let entries = match LogBuffer::global() {
        Some(buf) => buf.query(q.level.as_deref(), q.q.as_deref(), limit, q.offset.unwrap_or(0)),
        // 未安装缓冲（内嵌壳未初始化 tracing）→ 空列表
        None => Vec::new(),
    };
    Ok(Json(serde_json::json!({ "entries": entries })))
}
