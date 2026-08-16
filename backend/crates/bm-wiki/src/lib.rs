//! bm-wiki：WIKI 应用引擎（xu-wiki 迁移，Rust 重写）。
//!
//! 关系驱动 wiki 引擎：Page（不可变知识切片）+ Entity/List/Report（可重建
//! 学习层）+ 50 边 LRU 关系图 + 关键词检索打分。存储 = 纯 .md + YAML
//! frontmatter（字段名/目录布局与 xu-wiki 完全兼容，既有知识库可无缝打开）。
//! CLI 无 LLM 调用、确定性到底（语义判断归 agent/前端，引擎只管存储与排序）。

pub mod ingest;
pub mod layers;
pub mod model;
pub mod query;
pub mod relations;
pub mod splitter;
pub mod store;
pub mod uid;

pub use model::{Layer, Node, RefEntry};
pub use store::{Status, StoreError, StoreResult, Tree, TreeEntry, WikiStore};

/// 默认库内配置（对齐 xu-wiki default_wiki_config；体验参照，均可调）。
pub fn default_wiki_config(name: &str) -> serde_json::Value {
    serde_json::json!({
        "version": model::WIKI_FORMAT_VERSION,
        "name": name,
        "query": {
            "slice": { "chars": 50, "merge_radius": 80 },
            "blocks": 50,
            "uid_batch": 30,
            "query_max_expand": 10,
            "timeout_seconds": 10,
            "max_rounds": 5,
        },
        "relation": { "max_edges": model::MAX_EDGES, "policy": "lru" },
        "asset": { "compress_over": 2 * 1024 * 1024, "preserve_exif": true },
        "ingest": { "page_split_lines": model::PAGE_SPLIT_LINES },
        "rebuild": { "granularity": ["keep-l1", "keep-l1-l2", "full"] },
    })
}

/// 节点完整视图（read/expand 端点复用；含关系 + 修订 + 证据/成员）。
pub fn node_view(node: &Node) -> serde_json::Value {
    serde_json::json!({
        "uid": node.uid(),
        "title": node.title(),
        "layer": node.layer().as_str(),
        "content_type": node.content_type(),
        "node_path": node.node_path(),
        "active": node.active(),
        "created_at": node.created_at(),
        "created": node.created_at(),
        "body": node.body,
        "relations": node.relations().iter().map(|r| {
            serde_json::json!({
                "to_uid": r.to_uid,
                "relation_name": r.relation_name,
                "comment": r.comment,
                "created_at": r.created_at,
                "position": r.position,
            })
        }).collect::<Vec<_>>(),
        "patches": node.patches().iter().map(|p| {
            serde_json::json!({ "op": p.op, "delta": p.delta, "created_at": p.created_at })
        }).collect::<Vec<_>>(),
        "members": node.members().iter().map(|m| {
            serde_json::json!({ "uid": m.uid, "note": m.note, "position": m.position })
        }).collect::<Vec<_>>(),
        "references": node.references().iter().map(|r| {
            serde_json::json!({ "ref_uid": r.ref_uid, "note": r.note })
        }).collect::<Vec<_>>(),
        "raw_path": model::fm_get_str(&node.fm, model::FM_RAW_PATH),
        "source_hash": model::fm_get_str(&node.fm, model::FM_SOURCE_HASH),
        "source_page": model::fm_get_str(&node.fm, "source_page"),
        "parent_uid": model::fm_get_str(&node.fm, model::FM_PARENT_UID),
        "split_index": node.fm.get(serde_yaml::Value::String(model::FM_SPLIT_INDEX.into()))
            .and_then(|v| v.as_u64()),
        "frontmatter": model::to_json_mapping(&node.fm),
    })
}
