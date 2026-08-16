//! 关键词检索（DESIGN-ARCH-3 ripgrep 库族 + PRIN-ARCH-13 打分）。
//!
//! 扫描 nodes 下全部 *.md（ignore::WalkBuilder 尊重 .gitignore/.ignore），
//! 对每个关键词做固定串忽略大小写匹配；节点级打分：
//! `score = 标题命中 ×5 + body 命中 + 层权重（Entity=2, Report=3, List=1, Page=0）`。
//! CLI 不做语义判断（PRIN-ARCH-12 关键词由 agent/前端生成）。

use std::path::Path;

use ignore::WalkBuilder;
use regex::Regex;

use crate::model::{Layer, Node, TITLE_HIT_WEIGHT, parse_doc};

/// 一条检索命中（节点级聚合，按 score 降序）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScoredBlock {
    pub uid: String,
    pub title: String,
    pub layer: String,
    pub node_path: String,
    pub score: f64,
    /// 命中行号（1 起）——前端跳转用。
    pub matched_lines: Vec<usize>,
    /// 标题命中的关键词数。
    pub title_hits: usize,
    /// body 命中次数（行 × 关键词）。
    pub body_hits: usize,
}

/// 扫描整个库。返回按 score 降序的节点命中列表。
pub fn scan_all(nodes_dir: &Path, keywords: &[String]) -> Vec<ScoredBlock> {
    let kws: Vec<String> = keywords.iter().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
    if kws.is_empty() || !nodes_dir.is_dir() {
        return Vec::new();
    }
    let patterns: Vec<Regex> = kws
        .iter()
        .filter_map(|k| Regex::new(&format!("(?i){}", regex::escape(k))).ok())
        .collect();

    let mut hits: Vec<ScoredBlock> = Vec::new();
    let walker = WalkBuilder::new(nodes_dir).hidden(false).build();
    for entry in walker.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e != "md").unwrap_or(true) || !path.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let (fm, body) = parse_doc(&text);
        let Some(uid) = crate::model::fm_get_str(&fm, crate::model::FM_UID) else {
            continue;
        };
        let title = crate::model::fm_get_str(&fm, crate::model::FM_TITLE).unwrap_or_default();
        let layer = Layer::parse(crate::model::fm_get_str(&fm, crate::model::FM_LAYER).as_deref().unwrap_or(""));
        let node_path = crate::model::fm_get_str(&fm, crate::model::FM_NODE_PATH).unwrap_or_default();

        // 标题命中（每个关键词至多计 1）
        let title_hits = patterns.iter().filter(|re| re.is_match(&title)).count();
        // body 命中：逐行逐关键词计数 + 收集命中行号
        let mut body_hits = 0usize;
        let mut matched_lines: Vec<usize> = Vec::new();
        for (i, line) in body.lines().enumerate() {
            let mut line_hits = 0;
            for re in &patterns {
                line_hits += re.find_iter(line).count();
            }
            if line_hits > 0 {
                body_hits += line_hits;
                matched_lines.push(i + 1);
            }
        }
        if title_hits == 0 && body_hits == 0 {
            continue;
        }
        let score = title_hits as f64 * TITLE_HIT_WEIGHT + body_hits as f64 + layer.bonus();
        hits.push(ScoredBlock {
            uid,
            title,
            layer: layer.as_str().into(),
            node_path,
            score,
            matched_lines,
            title_hits,
            body_hits,
        });
    }
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits
}

/// 按 uid 在库中找节点文件（四分区全扫；uid 全局唯一）。
pub fn find_by_uid(nodes_dir: &Path, uid: &str) -> Option<Node> {
    if !crate::uid::is_valid_uid(uid) {
        return None;
    }
    let walker = WalkBuilder::new(nodes_dir).hidden(false).build();
    for entry in walker.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e != "md").unwrap_or(true) || !path.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let (fm, body) = parse_doc(&text);
        if crate::model::fm_get_str(&fm, crate::model::FM_UID).as_deref() == Some(uid) {
            let rel = path.to_string_lossy().replace('\\', "/");
            return Some(Node { rel_path: rel, fm, body });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_wiki(root: &Path, dir: &str, name: &str, layer: Layer, body: &str, title: &str) {
        let d = root.join("nodes").join(dir);
        std::fs::create_dir_all(&d).unwrap();
        let mut fm = serde_yaml::Mapping::new();
        crate::model::fm_set_str(&mut fm, "uid", crate::uid::gen_uid());
        crate::model::fm_set_str(&mut fm, "title", title);
        crate::model::fm_set_str(&mut fm, "layer", layer.as_str());
        crate::model::fm_set_str(&mut fm, "content_type", "article");
        crate::model::fm_set_bool(&mut fm, "active", true);
        crate::model::fm_set_i64(&mut fm, "created_at", 1);
        crate::model::fm_set_str(&mut fm, "content_hash", crate::model::sha256_text(body));
        std::fs::write(d.join(format!("{name}.md")), crate::model::render_doc(&fm, body)).unwrap();
    }

    #[test]
    fn scores_title_over_body_and_layer_bonus() {
        let tmp = std::env::temp_dir().join(format!("bm-wiki-q-{}", crate::uid::gen_uid()));
        let nodes = tmp.join("nodes");
        mk_wiki(&tmp, "pages", "p1", Layer::Page, "rust 语言指南正文", "rust 入门");
        mk_wiki(&tmp, "entities", "e1", Layer::Entity, "关于 rust 的实体笔记", "rust 实体");
        mk_wiki(&tmp, "reports", "r1", Layer::Report, "结论：rust 值得学", "rust 调研报告");
        mk_wiki(&tmp, "lists", "l1", Layer::List, "对比表", "rust 对比");

        let hits = scan_all(&nodes, &["rust".to_string()]);
        assert_eq!(hits.len(), 4);
        // Report 标题×5 + layer3(8) > Entity 标题×5 + layer2(7) > List 标题×5 + layer1(6) > Page 标题×5(5)
        assert_eq!(hits[0].title, "rust 调研报告");
        assert_eq!(hits[1].title, "rust 实体");
        assert_eq!(hits[2].title, "rust 对比");
        assert_eq!(hits[3].title, "rust 入门");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn keyword_missing_returns_empty() {
        let tmp = std::env::temp_dir().join(format!("bm-wiki-q-{}", crate::uid::gen_uid()));
        mk_wiki(&tmp, "pages", "p1", Layer::Page, "hello world", "greeting");
        assert!(scan_all(&tmp.join("nodes"), &["不存在的词".to_string()]).is_empty());
        assert!(scan_all(&tmp.join("nodes"), &[]).is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn matched_lines_collected() {
        let tmp = std::env::temp_dir().join(format!("bm-wiki-q-{}", crate::uid::gen_uid()));
        mk_wiki(&tmp, "pages", "p1", Layer::Page, "第一行\n第二行 rust\n第三行", "标题 rust 主题");
        let hits = scan_all(&tmp.join("nodes"), &["rust".to_string()]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].matched_lines, vec![2]);
        assert_eq!(hits[0].title_hits, 1);
        assert_eq!(hits[0].body_hits, 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_by_uid_works() {
        let tmp = std::env::temp_dir().join(format!("bm-wiki-q-{}", crate::uid::gen_uid()));
        let nodes = tmp.join("nodes");
        mk_wiki(&tmp, "pages", "p1", Layer::Page, "body", "title");
        let text = std::fs::read_to_string(nodes.join("pages").join("p1.md")).unwrap();
        let (fm, _) = parse_doc(&text);
        let uid = crate::model::fm_get_str(&fm, "uid").unwrap();
        let found = find_by_uid(&nodes, &uid);
        assert!(found.is_some());
        assert_eq!(found.unwrap().title(), "title");
        assert!(find_by_uid(&nodes, "BADUID!").is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
