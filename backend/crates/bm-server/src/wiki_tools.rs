//! WIKI 场景聊天工具（xu-wiki agent 驱动的设计初衷迁移）：模型在对话里
//! 直接操作知识库。三个工具：wiki_query / wiki_ingest / wiki_add_relation。
//!
//! 工具面契约（对齐 xu-wiki SKILL.md）：返回 4-key JSON 信封
//! `{status, data, message, hints}`——错误也是信封（status:error + hints 指路），
//! 模型读 hints 当 deferred work。确定性到底，不调 LLM。
//!
//! 挂载：定义进 bm_engine 场景注册点（session.app == "wiki" 才进工具面，
//! 场景隔离由机制保证）；执行在 compat_engine 分派中枢按 `wiki_` 前缀路由。

use std::path::Path;

use bm_loop::model::ToolDef;
use bm_wiki::WikiStore;
use serde_json::{Value, json};

pub const NAMES: [&str; 3] = ["wiki_query", "wiki_ingest", "wiki_add_relation"];

/// 场景工具模型侧 schema。
pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef::new(
            "wiki_query",
            "Search the wiki knowledge base (working_dir/wiki). Keywords are agent-generated (include Chinese+English); returns scored hits {uid,title,layer,score,matched_lines}. No LLM involved.",
            json!({
                "type": "object",
                "properties": {
                    "keywords": { "type": "string", "description": "Comma-separated keywords" },
                    "top": { "type": "integer", "description": "Max hits (default 20)" },
                },
                "required": ["keywords"],
            }),
        ),
        ToolDef::new(
            "wiki_ingest",
            "Create immutable Page node(s) from text. Auto-splits long content (>300 lines) into a page chain. SHA256 dedup applies when a source file is given (not here — text only).",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "content": { "type": "string" },
                    "node_path": { "type": "string", "description": "Optional logical partition, e.g. papers/ml" },
                },
                "required": ["title", "content"],
            }),
        ),
        ToolDef::new(
            "wiki_add_relation",
            "Add/refresh a directed relation edge in the 50-edge LRU graph (hit promotes to head; full evicts tail).",
            json!({
                "type": "object",
                "properties": {
                    "from_uid": { "type": "string" },
                    "to_uid": { "type": "string" },
                    "relation_name": { "type": "string", "description": "e.g. 参访/引用/describes" },
                    "comment": { "type": "string" },
                },
                "required": ["from_uid", "to_uid", "relation_name"],
            }),
        ),
    ]
}

/// 执行 wiki 工具（同步文件 IO，毫秒级；由 compat_engine 分派调用）。
pub fn execute(name: &str, input: &Value, working_dir: &str) -> Result<Value, String> {
    let root = Path::new(working_dir).join("wiki");
    match name {
        "wiki_query" => cmd_query(&root, input),
        "wiki_ingest" => cmd_ingest(&root, input),
        "wiki_add_relation" => cmd_add_relation(&root, input),
        _ => Err(format!("unknown wiki tool: {name}")),
    }
}

// ── 命令实现（4-key 信封）──

fn cmd_query(root: &Path, input: &Value) -> Result<Value, String> {
    let keywords = input
        .get("keywords")
        .and_then(Value::as_str)
        .ok_or("wiki_query: keywords is required")?;
    let top = input.get("top").and_then(Value::as_u64).unwrap_or(20) as usize;
    let store = match WikiStore::at(root.to_path_buf()) {
        Ok(s) => s,
        Err(_) => {
            return Ok(json!({
                "status": "error",
                "message": format!("wiki 库不存在（{}）", root.display()),
                "hints": ["先在 WIKI 应用页建库（API: POST /api/wiki/create）"],
            }));
        }
    };
    let kws: Vec<String> = keywords
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if kws.is_empty() {
        return Ok(json!({ "status": "error", "message": "no keywords", "hints": [] }));
    }
    let hits = store.query(&kws).map_err(|e| e.to_string())?;
    let top_hits: Vec<&bm_wiki::query::ScoredBlock> = hits.iter().take(top).collect();
    Ok(json!({
        "status": "ok",
        "data": {
            "count": hits.len(),
            "returned": top_hits.len(),
            "hits": top_hits,
        },
        "message": format!("{} hit(s); returning top {}", hits.len(), top_hits.len()),
        "hints": [
            "pick UIDs and read them (GET /api/wiki/node/{uid}) to get full bodies",
            "use wiki_add_relation to wire entities/lists after ingesting",
        ],
    }))
}

fn cmd_ingest(root: &Path, input: &Value) -> Result<Value, String> {
    let title = input
        .get("title")
        .and_then(Value::as_str)
        .ok_or("wiki_ingest: title is required")?
        .to_string();
    let content = input
        .get("content")
        .and_then(Value::as_str)
        .ok_or("wiki_ingest: content is required")?
        .to_string();
    let node_path = input.get("node_path").and_then(Value::as_str).unwrap_or("");
    let store = match WikiStore::at(root.to_path_buf()) {
        Ok(s) => s,
        Err(_) => {
            return Ok(json!({
                "status": "error",
                "message": format!("wiki 库不存在（{}）", root.display()),
                "hints": ["先在 WIKI 应用页建库（API: POST /api/wiki/create）"],
            }));
        }
    };
    let res = store
        .ingest(bm_wiki::ingest::IngestRequest {
            title,
            content,
            node_path: node_path.to_string(),
            source_file: None,
        })
        .map_err(|e| e.to_string())?;
    if let Some(dedup) = &res.deduped {
        return Ok(json!({
            "status": "ok",
            "data": { "pages": [], "deduped": dedup },
            "message": "source already ingested — no duplicate written",
            "hints": ["extend the existing node instead of re-ingesting"],
        }));
    }
    let created = &res.pages;
    let summary: Vec<Value> = created
        .iter()
        .map(|p| json!({ "uid": p.uid, "title": p.title, "split_index": p.split_index }))
        .collect();
    Ok(json!({
        "status": "ok",
        "data": { "pages": summary },
        "message": format!("{} page(s) written", created.len()),
        "hints": [
            "run wiki_query with the topic keywords to find existing entities, then wiki_add_relation to wire describes links",
            "run doctor-equivalent (tree check) later if the node landed at root without node_path",
        ],
    }))
}

fn cmd_add_relation(root: &Path, input: &Value) -> Result<Value, String> {
    let from_uid = input
        .get("from_uid")
        .and_then(Value::as_str)
        .ok_or("wiki_add_relation: from_uid is required")?;
    let to_uid = input
        .get("to_uid")
        .and_then(Value::as_str)
        .ok_or("wiki_add_relation: to_uid is required")?;
    let relation_name = input
        .get("relation_name")
        .and_then(Value::as_str)
        .ok_or("wiki_add_relation: relation_name is required")?;
    let comment = input.get("comment").and_then(Value::as_str).unwrap_or("");
    let store = match WikiStore::at(root.to_path_buf()) {
        Ok(s) => s,
        Err(_) => {
            return Ok(json!({
                "status": "error",
                "message": format!("wiki 库不存在（{}）", root.display()),
                "hints": ["先在 WIKI 应用页建库（API: POST /api/wiki/create）"],
            }));
        }
    };
    let out = store
        .add_relation(from_uid, to_uid, relation_name, comment)
        .map_err(|e| e.to_string())?;
    let action = out.get("action").and_then(Value::as_str).unwrap_or("created");
    Ok(json!({
        "status": "ok",
        "data": out,
        "message": format!("relation {action} ({from_uid} → {to_uid}: {relation_name})"),
        "hints": ["50-edge LRU: recent reads keep edges alive; evicted edges fall off"],
    }))
}
