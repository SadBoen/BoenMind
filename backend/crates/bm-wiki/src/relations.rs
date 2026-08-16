//! 50 边 LRU 关系（PRIN-ARCH-7~10，对齐 xu-wiki ingest/relations_lru.py）。
//!
//! 节点出边 = frontmatter `relations` 有序列表（position=0 队首）：
//! 建立进队首 → 命中前挪 → 满 50 弹队尾。无分类无打分；要固化走 List。

use serde_yaml::{Mapping, Value};

use crate::model::{FM_RELATIONS, Relation, MAX_EDGES, now_ts};

/// 从 frontmatter 读取关系列表（按 position 排序）。
pub fn load_relations(fm: &Mapping) -> Vec<Relation> {
    let mut rels: Vec<Relation> = crate::model::parse_list(fm, FM_RELATIONS);
    rels.sort_by_key(|r| r.position);
    rels
}

/// 添加/刷新一条关系。返回 (action, evicted)。
/// - 已有相同 (to_uid, relation_name) → 刷新（先移除再进队首）；
/// - 满 max_edges 弹队尾，返回被置换条目。
pub fn add_relation(
    fm: &mut Mapping,
    from_uid: &str,
    to_uid: &str,
    relation_name: &str,
    comment: &str,
    max_edges: usize,
) -> (String, Option<Relation>) {
    if to_uid == from_uid {
        return ("self-loop-rejected".into(), None);
    }
    let mut rels = load_relations(fm);
    let existing_idx = rels
        .iter()
        .position(|r| r.to_uid == to_uid && r.relation_name == relation_name);

    let ts = now_ts();
    let mut new_entry = Relation {
        to_uid: to_uid.to_string(),
        relation_name: relation_name.to_string(),
        comment: comment.to_string(),
        created_at: ts,
        position: 0,
    };
    let action = if existing_idx.is_some() { "refreshed" } else { "created" };
    if let Some(idx) = existing_idx {
        // 刷新：保留原 created_at（对齐 xu-wiki：新建条目前 pop 旧条目）
        new_entry.created_at = rels[idx].created_at;
        rels.remove(idx);
    }
    for r in &mut rels {
        r.position += 1;
    }
    rels.insert(0, new_entry);
    let mut evicted = None;
    if rels.len() > max_edges {
        evicted = rels.pop();
    }
    for (i, r) in rels.iter_mut().enumerate() {
        r.position = i as u32;
    }
    crate::model::set_list(fm, FM_RELATIONS, &rels);
    (action.into(), evicted)
}

/// 删除一条关系（按 to_uid + relation_name）。返回是否命中。
pub fn remove_relation(fm: &mut Mapping, to_uid: &str, relation_name: &str) -> bool {
    let mut rels = load_relations(fm);
    let before = rels.len();
    rels.retain(|r| !(r.to_uid == to_uid && r.relation_name == relation_name));
    if rels.len() == before {
        return false;
    }
    for (i, r) in rels.iter_mut().enumerate() {
        r.position = i as u32;
    }
    crate::model::set_list(fm, FM_RELATIONS, &rels);
    true
}

/// 关系 → JSON 值（REST/工具边界）。
pub fn relations_to_json(rels: &[Relation]) -> serde_json::Value {
    serde_json::json!({
        "count": rels.len(),
        "max": MAX_EDGES,
        "edges": rels.iter().map(|r| {
            serde_json::json!({
                "to_uid": r.to_uid,
                "relation_name": r.relation_name,
                "comment": r.comment,
                "created_at": r.created_at,
                "position": r.position,
            })
        }).collect::<Vec<_>>(),
    })
}

/// 从 Mapping 取关系（供 store 层直接调用）。
pub fn relations_from_mapping(fm: &Mapping) -> Vec<Relation> {
    load_relations(fm)
}

/// 命中即前挪一位（PRIN-ARCH-10 查询触碰）：节点被 expand 时调用，
/// 写回由 store 层负责。返回是否命中。
pub fn touch(fm: &mut Mapping, to_uid: &str) -> bool {
    let mut rels = load_relations(fm);
    let Some(idx) = rels.iter().position(|r| r.to_uid == to_uid) else {
        return false;
    };
    if idx == 0 {
        return true;
    }
    let mut target = rels.remove(idx);
    target.position = idx as u32 - 1;
    rels.insert(idx - 1, target);
    for (i, r) in rels.iter_mut().enumerate() {
        r.position = i as u32;
    }
    crate::model::set_list(fm, FM_RELATIONS, &rels);
    true
}

/// 关系条目 → YAML Value（测试辅助）。
pub fn relation_to_yaml_value(r: &Relation) -> Value {
    serde_yaml::to_value(r).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping_with_relations() -> Mapping {
        let mut fm = Mapping::new();
        let empty: Vec<Relation> = Vec::new();
        crate::model::set_list(&mut fm, FM_RELATIONS, &empty);
        fm
    }

    #[test]
    fn add_creates_at_head() {
        let mut fm = mapping_with_relations();
        let (action, evicted) = add_relation(&mut fm, "A0000001", "B0000001", "引用", "", 50);
        assert_eq!(action, "created");
        assert!(evicted.is_none());
        let rels = load_relations(&fm);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].to_uid, "B0000001");
        assert_eq!(rels[0].position, 0);
    }

    #[test]
    fn add_refreshes_existing() {
        let mut fm = mapping_with_relations();
        add_relation(&mut fm, "A0000001", "B0000001", "引用", "旧注释", 50);
        add_relation(&mut fm, "A0000001", "C0000001", "引用", "", 50);
        let (action, _) = add_relation(&mut fm, "A0000001", "B0000001", "引用", "新注释", 50);
        assert_eq!(action, "refreshed");
        let rels = load_relations(&fm);
        assert_eq!(rels.len(), 2);
        assert_eq!(rels[0].to_uid, "B0000001");
        assert_eq!(rels[0].comment, "新注释");
        // 刷新保留原 created_at
        assert!(rels[0].created_at > 0);
    }

    #[test]
    fn lru_evicts_tail() {
        let mut fm = mapping_with_relations();
        for i in 0..52 {
            let to = format!("B00000{i:02}");
            add_relation(&mut fm, "A0000001", &to, "边", "", 50);
        }
        let rels = load_relations(&fm);
        assert_eq!(rels.len(), 50);
        // 队首 = 最新（第 52 次加入的 B0000051）
        assert_eq!(rels[0].to_uid, "B0000051");
        // 队尾：第 51 次弹 B0000000、第 52 次弹 B0000001 → 队尾 B0000002
        assert_eq!(rels[49].to_uid, "B0000002");
    }

    #[test]
    fn remove_deletes_and_reindexes() {
        let mut fm = mapping_with_relations();
        add_relation(&mut fm, "A0000001", "B0000001", "引用", "", 50);
        add_relation(&mut fm, "A0000001", "C0000001", "引用", "", 50);
        assert!(remove_relation(&mut fm, "B0000001", "引用"));
        assert!(!remove_relation(&mut fm, "B0000001", "引用"));
        let rels = load_relations(&fm);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].to_uid, "C0000001");
        assert_eq!(rels[0].position, 0);
    }

    #[test]
    fn touch_promotes_by_one() {
        let mut fm = mapping_with_relations();
        add_relation(&mut fm, "A0000001", "B0000001", "引用", "", 50);
        add_relation(&mut fm, "A0000001", "C0000001", "引用", "", 50);
        add_relation(&mut fm, "A0000001", "D0000001", "引用", "", 50);
        // 顺序 D, C, B —— touch B → D, B, C
        assert!(touch(&mut fm, "B0000001"));
        let rels = load_relations(&fm);
        assert_eq!(rels[0].to_uid, "D0000001");
        assert_eq!(rels[1].to_uid, "B0000001");
        assert_eq!(rels[2].to_uid, "C0000001");
    }

    #[test]
    fn self_loop_rejected() {
        let mut fm = mapping_with_relations();
        let (action, _) = add_relation(&mut fm, "A0000001", "A0000001", "自引用", "", 50);
        assert_eq!(action, "self-loop-rejected");
        assert!(load_relations(&fm).is_empty());
    }
}
