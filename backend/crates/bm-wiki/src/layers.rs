//! 学习层节点：List / Report / Entity（PRIN-ARCH-4~6，对齐 xu-wiki layers.py）。
//!
//! 三者与 Page 的关键差异：**可改可重建**（Page 不可变）。全部 .md-only
//! 存储（DESIGN-ARCH-1）：
//! - List：对既有节点的比较/聚合，frontmatter `members` 成员 UID 列表；
//! - Report：推理 + 结论，**强制 ≥1 证据链**（BAN-ARCH-5 references）；
//! - Entity：一等实体，body 即笔记，source_page 回链源 Page。

use std::path::Path;

use serde_yaml::Mapping;

use crate::model::{
    FM_ACTIVE, FM_CONTENT_HASH, FM_CREATED, FM_LAYER, FM_MEMBERS, FM_NODE_PATH, FM_PATCHES,
    FM_TITLE, FM_UID, Layer, Member, Node, RefEntry, now_ts, safe_node_path, safe_slug,
    sha256_text,
};
use crate::uid::gen_uid;

/// 新建学习层节点请求。
#[derive(Debug, Clone)]
pub struct LayerCreate {
    pub layer: Layer,
    pub title: String,
    pub body: String,
    pub node_path: String,
    /// List 成员 UID（List 专用）。
    pub members: Vec<String>,
    /// 证据链（Report 专用，强制 ≥1）。
    pub references: Vec<RefEntry>,
    /// 回链源 Page（Entity 专用）。
    pub source_page: Option<String>,
}

/// 创建学习层节点（写 nodes/{dir}/{slug}-{uid}.md），返回新节点。
pub fn create(nodes_dir: &Path, req: &LayerCreate) -> Result<Node, String> {
    let layer = req.layer;
    if layer == Layer::Page {
        return Err("use ingest for Page nodes".into());
    }
    let title = req.title.trim();
    if title.is_empty() {
        return Err("title is required".into());
    }
    if req.body.trim().is_empty() {
        return Err("body is empty".into());
    }
    let node_path = safe_node_path(&req.node_path)?;
    if layer == Layer::Report && req.references.is_empty() {
        return Err("Report requires at least 1 evidence reference (BAN-ARCH-5)".into());
    }
    if layer == Layer::List && req.members.is_empty() {
        return Err("List requires at least 1 member uid".into());
    }

    let uid = gen_uid();
    let ts = now_ts();
    let mut fm = Mapping::new();
    fm_insert_str(&mut fm, FM_UID, &uid);
    fm_insert_str(&mut fm, FM_TITLE, title);
    fm_insert_str(&mut fm, FM_LAYER, layer.as_str());
    fm_insert_str(&mut fm, "content_type", "article");
    fm_insert_bool(&mut fm, FM_ACTIVE, true);
    fm_insert_i64(&mut fm, FM_CREATED, ts);
    fm_insert_str(&mut fm, FM_CONTENT_HASH, &sha256_text(req.body.trim_end()));
    fm_insert_str(&mut fm, FM_NODE_PATH, &node_path);

    if layer == Layer::List {
        let members: Vec<Member> = req
            .members
            .iter()
            .enumerate()
            .map(|(i, m)| Member {
                uid: m.clone(),
                note: String::new(),
                position: i as u32,
            })
            .collect();
        fm_insert_list(&mut fm, FM_MEMBERS, &members);
    }
    if layer == Layer::Report {
        fm_insert_list(&mut fm, "references", &req.references);
    }
    if layer == Layer::Entity
        && let Some(sp) = &req.source_page
    {
        fm_insert_str(&mut fm, "source_page", sp);
    }

    let slug = safe_slug(title, 80);
    let rel_md = match node_path.is_empty() {
        true => format!("nodes/{}/{slug}-{uid}.md", layer.dir_name()),
        false => format!("nodes/{}/{node_path}/{slug}-{uid}.md", layer.dir_name()),
    };
    let md_path = nodes_dir.parent().unwrap_or(nodes_dir).join(&rel_md);
    if let Some(parent) = md_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let doc = crate::model::render_doc(&fm, req.body.trim_end());
    let tmp = md_path.with_extension(format!("md.tmp.{}", gen_uid()));
    std::fs::write(&tmp, &doc).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &md_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;

    Ok(Node { rel_path: rel_md, fm, body: req.body.trim_end().to_string() })
}

/// 修改学习层节点（title/body 可改；Page 拒绝——不可变 PRIN-ARCH-3）。
/// 返回更新后的 Node；members/references 保持原样（M1 不改结构，只改正文）。
pub fn update(node: &mut Node, title: Option<&str>, body: Option<&str>) -> Result<(), String> {
    if node.layer() == Layer::Page {
        return Err("Page is immutable (PRIN-ARCH-3); revisions go through patches".into());
    }
    if let Some(t) = title {
        let t = t.trim();
        if t.is_empty() {
            return Err("title is required".into());
        }
        fm_insert_str(&mut node.fm, FM_TITLE, t);
    }
    if let Some(b) = body {
        let b = b.trim_end();
        if b.trim().is_empty() {
            return Err("body is empty".into());
        }
        node.body = b.to_string();
        fm_insert_str(&mut node.fm, FM_CONTENT_HASH, &sha256_text(b));
    }
    Ok(())
}

/// 追加 Page 修订（PRIN-ARCH-3 唯一修订通道；M1 语义：op=patch 增量记录）。
pub fn append_patch(node: &mut Node, op: &str, delta: &str) {
    let mut patches = node.patches();
    patches.push(crate::model::Patch {
        op: op.into(),
        delta: delta.into(),
        created_at: now_ts(),
    });
    fm_insert_list(&mut node.fm, FM_PATCHES, &patches);
}

/// 解析 references 中目标节点的标题（前端显示证据链时查名）。
pub fn resolve_titles(nodes_dir: &Path, uids: &[String]) -> std::collections::HashMap<String, String> {
    use crate::query::find_by_uid;
    let mut out = std::collections::HashMap::new();
    for uid in uids {
        if let Some(n) = find_by_uid(nodes_dir, uid) {
            out.insert(uid.clone(), n.title());
        }
    }
    out
}

// Mapping helpers（与 ingest.rs 同款；避免跨模块私有可见性重复定义）
fn fm_insert_str(fm: &mut Mapping, key: &str, value: &str) {
    fm.insert(serde_yaml::Value::String(key.into()), serde_yaml::Value::String(value.into()));
}
fn fm_insert_bool(fm: &mut Mapping, key: &str, value: bool) {
    fm.insert(serde_yaml::Value::String(key.into()), serde_yaml::Value::Bool(value));
}
fn fm_insert_i64(fm: &mut Mapping, key: &str, value: i64) {
    fm.insert(serde_yaml::Value::String(key.into()), serde_yaml::Value::Number(value.into()));
}
fn fm_insert_list<T: serde::Serialize>(fm: &mut Mapping, key: &str, items: &[T]) {
    let arr: Vec<serde_yaml::Value> = items
        .iter()
        .filter_map(|it| serde_yaml::to_value(it).ok())
        .collect();
    fm.insert(serde_yaml::Value::String(key.into()), serde_yaml::Value::Sequence(arr));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_root() -> PathBuf {
        let d = std::env::temp_dir().join(format!("bm-wiki-ly-{}", gen_uid()));
        std::fs::create_dir_all(d.join("nodes")).unwrap();
        d
    }

    #[test]
    fn list_create_with_members() {
        let root = tmp_root();
        let node = create(
            &root.join("nodes"),
            &LayerCreate {
                layer: Layer::List,
                title: "Rust 学习清单".into(),
                body: "对比三个教程".into(),
                node_path: String::new(),
                members: vec!["AAAA1111".into(), "BBBB2222".into()],
                references: vec![],
                source_page: None,
            },
        )
        .unwrap();
        assert_eq!(node.layer(), Layer::List);
        assert_eq!(node.members().len(), 2);
        assert_eq!(node.members()[0].position, 0);
        assert!(node.rel_path.starts_with("nodes/lists/"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn report_requires_evidence() {
        let root = tmp_root();
        let r = create(
            &root.join("nodes"),
            &LayerCreate {
                layer: Layer::Report,
                title: "无证据报告".into(),
                body: "空对空推理".into(),
                node_path: String::new(),
                members: vec![],
                references: vec![],
                source_page: None,
            },
        );
        assert!(r.is_err());
        let ok = create(
            &root.join("nodes"),
            &LayerCreate {
                layer: Layer::Report,
                title: "有证据报告".into(),
                body: "基于事实".into(),
                node_path: String::new(),
                members: vec![],
                references: vec![RefEntry { ref_uid: "AAAA1111".into(), note: "来源".into() }],
                source_page: None,
            },
        )
        .unwrap();
        assert_eq!(ok.references().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn entity_with_source_page() {
        let root = tmp_root();
        let node = create(
            &root.join("nodes"),
            &LayerCreate {
                layer: Layer::Entity,
                title: "Python 语言".into(),
                body: "解释型语言".into(),
                node_path: "lang".into(),
                members: vec![],
                references: vec![],
                source_page: Some("AAAA1111".into()),
            },
        )
        .unwrap();
        assert_eq!(
            node.fm.get("source_page").and_then(|v| v.as_str()).unwrap(),
            "AAAA1111"
        );
        assert!(node.rel_path.starts_with("nodes/entities/lang/"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn page_immutable_under_update() {
        let root = tmp_root();
        let mut node = Node {
            rel_path: "nodes/pages/x.md".into(),
            fm: {
                let mut fm = Mapping::new();
                fm_insert_str(&mut fm, FM_UID, &gen_uid());
                fm_insert_str(&mut fm, FM_LAYER, "Page");
                fm
            },
            body: "旧内容".into(),
        };
        assert!(update(&mut node, Some("新标题"), Some("新内容")).is_err());
        assert_eq!(node.body, "旧内容");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn layer_update_changes_body_and_hash() {
        let root = tmp_root();
        let mut node = create(
            &root.join("nodes"),
            &LayerCreate {
                layer: Layer::Entity,
                title: "实体".into(),
                body: "原始".into(),
                node_path: String::new(),
                members: vec![],
                references: vec![],
                source_page: None,
            },
        )
        .unwrap();
        update(&mut node, Some("实体2"), Some("更新后")).unwrap();
        assert_eq!(node.title(), "实体2");
        assert_eq!(node.body, "更新后");
        assert_eq!(
            node.fm.get(FM_CONTENT_HASH).and_then(|v| v.as_str()).unwrap(),
            sha256_text("更新后")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
