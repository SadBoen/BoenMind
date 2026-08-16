//! Page 摄取（对齐 xu-wiki ingest-commit 两阶段之提交段；PRIN-ING-1 唯一写入口）。
//!
//! 文本/文件 → 切分（splitter）→ SHA256 去重 → 原子写 Page 群 + raws 副本 +
//! patches v1 + split 链（parent_uid/split_index）。Page 一经写入不可变，修订
//! 只能走 patches（PRIN-ARCH-3）。

use std::path::{Path, PathBuf};

use serde_yaml::Mapping;

use crate::model::{
    FM_ACTIVE, FM_CONTENT_HASH, FM_CONTENT_TYPE, FM_CREATED, FM_LAYER, FM_NODE_PATH,
    FM_PARENT_UID, FM_PATCHES, FM_RAW_PATH, FM_SOURCE_HASH, FM_SPLIT_INDEX, FM_TITLE, FM_UID,
    Node, now_ts, safe_node_path, safe_slug, sha256_bytes, sha256_text,
};
use crate::splitter::split_pages;
use crate::uid::gen_uid;

/// 摄取请求（M1：文本或单文件；文件仅 .md/.txt）。
#[derive(Debug, Clone)]
pub struct IngestRequest {
    pub title: String,
    pub content: String,
    /// 逻辑分区（可选，如 papers/ml）。
    pub node_path: String,
    /// 源文件路径（可选：写入 raws 副本 + source_hash + raw_path）。
    pub source_file: Option<PathBuf>,
}

/// 摄取结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct IngestResult {
    /// 已写入的 Page（多页群）。
    pub pages: Vec<CreatedPage>,
    /// source_hash 去重命中（已存在同名源，未重复写）。
    pub deduped: Option<DedupInfo>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CreatedPage {
    pub uid: String,
    pub title: String,
    pub md_path: String,
    pub split_index: u32,
    pub lines: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DedupInfo {
    pub source_hash: String,
    pub existing_uid: String,
    pub existing_title: String,
}

/// 校验 title/content 合法性。
pub fn validate(req: &IngestRequest) -> Result<(), String> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err("title is required".into());
    }
    if title.len() > 200 {
        return Err("title too long (max 200)".into());
    }
    if req.content.trim().is_empty() {
        return Err("content is empty".into());
    }
    safe_node_path(&req.node_path)?;
    if let Some(file) = &req.source_file {
        let ext = file
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "md" | "txt") {
            return Err(format!("unsupported file type '.{ext}' (M1: md/txt only)"));
        }
        if !file.is_file() {
            return Err(format!("source file not found: {}", file.display()));
        }
    }
    Ok(())
}

/// 提交摄取：切分 + 去重 + 原子写。返回已写 Page 群。
/// `nodes_dir` = 库 nodes 目录，`raws_dir` = raws 目录（均需已存在）。
pub fn commit(
    nodes_dir: &Path,
    raws_dir: &Path,
    req: &IngestRequest,
) -> Result<IngestResult, String> {
    validate(req)?;
    let title = req.title.trim();
    let node_path = safe_node_path(&req.node_path)?;

    // source_hash 去重（文件导入才有的语义；文本导入无源不查）
    let source_hash = match &req.source_file {
        Some(f) => Some(sha256_bytes(&std::fs::read(f).map_err(|e| e.to_string())?)),
        None => None,
    };
    if let Some(sh) = &source_hash
        && let Some(existing) = find_by_source_hash(nodes_dir, sh)
    {
        return Ok(IngestResult {
            pages: Vec::new(),
            deduped: Some(DedupInfo {
                source_hash: sh.clone(),
                existing_uid: existing.0,
                existing_title: existing.1,
            }),
        });
    }

    let page_bodies = split_pages(&req.content, crate::model::PAGE_SPLIT_LINES);
    if page_bodies.is_empty() {
        return Err("content is empty after split".into());
    }
    let multi = page_bodies.len() > 1;
    let ts = now_ts();
    let first_uid = gen_uid();
    let base_slug = safe_slug(title, 80);

    // raws 副本（仅首页 + 源文件存在时；raws/{node_path}/{filename}）
    let mut raw_rel: Option<PathBuf> = None;
    if let Some(src) = &req.source_file {
        let raw_dir = match node_path.is_empty() {
            true => raws_dir.to_path_buf(),
            false => raws_dir.join(&node_path),
        };
        std::fs::create_dir_all(&raw_dir).map_err(|e| e.to_string())?;
        let dst = raw_dir.join(
            src.file_name().unwrap_or_default().to_string_lossy().as_ref(),
        );
        if !dst.exists() {
            std::fs::copy(src, &dst).map_err(|e| e.to_string())?;
        }
        let rel = match node_path.is_empty() {
            true => PathBuf::from("raws").join(src.file_name().unwrap_or_default()),
            false => PathBuf::from("raws")
                .join(&node_path)
                .join(src.file_name().unwrap_or_default()),
        };
        raw_rel = Some(rel);
    }

    let mut written: Vec<(PathBuf, String)> = Vec::new();
    let mut created: Vec<CreatedPage> = Vec::new();

    for (idx, page_body) in page_bodies.iter().enumerate() {
        let uid = if idx == 0 { first_uid.clone() } else { gen_uid() };
        let split_index = (idx + 1) as u32;
        let page_title = if multi {
            format!("{title} (part {}/{})", idx + 1, page_bodies.len())
        } else {
            title.to_string()
        };
        let slug = if multi {
            format!("{base_slug}-{}-{uid}", idx + 1)
        } else {
            format!("{base_slug}-{uid}")
        };

        let mut fm = Mapping::new();
        fm_insert_str(&mut fm, FM_UID, &uid);
        fm_insert_str(&mut fm, FM_TITLE, &page_title);
        fm_insert_str(&mut fm, FM_LAYER, "Page");
        fm_insert_str(&mut fm, FM_CONTENT_TYPE, "article");
        fm_insert_bool(&mut fm, FM_ACTIVE, true);
        fm_insert_i64(&mut fm, FM_CREATED, ts);
        fm_insert_str(&mut fm, FM_CONTENT_HASH, &sha256_text(page_body));
        fm_insert_str(&mut fm, FM_NODE_PATH, &node_path);
        fm_insert_u64(&mut fm, FM_SPLIT_INDEX, split_index.into());
        fm_insert_str(&mut fm, FM_PARENT_UID, &first_uid);
        fm_insert_list(
            &mut fm,
            FM_PATCHES,
            &[crate::model::Patch {
                op: "create".into(),
                delta: sha256_text(page_body),
                created_at: ts,
            }],
        );
        if let Some(sh) = &source_hash {
            fm_insert_str(&mut fm, FM_SOURCE_HASH, sh);
        }
        if idx == 0
            && let Some(raw) = &raw_rel
        {
            fm_insert_str(&mut fm, FM_RAW_PATH, &raw.to_string_lossy());
        }

        let rel_md = match node_path.is_empty() {
            true => format!("nodes/pages/{slug}.md"),
            false => format!("nodes/pages/{node_path}/{slug}.md"),
        };
        let md_path = nodes_dir.parent().unwrap_or(nodes_dir).join(&rel_md);
        // md_path 已含 nodes/… 前缀；nodes_dir.parent() = 库根
        write_atomic(&md_path, &crate::model::render_doc(&fm, page_body))?;

        written.push((md_path.clone(), uid.clone()));
        created.push(CreatedPage {
            uid,
            title: page_title,
            md_path: rel_md,
            split_index,
            lines: page_body.lines().count(),
        });
    }

    // 写后校验（对齐 _verify_committed 核心：可读 + uid 匹配），失败回滚已写文件
    for (path, uid) in &written {
        let ok = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| {
                let (fm, _) = crate::model::parse_doc(&text);
                crate::model::fm_get_str(&fm, FM_UID)
            })
            .as_deref()
            == Some(uid.as_str());
        if !ok {
            for (p, _) in &written {
                let _ = std::fs::remove_file(p);
            }
            return Err(format!("verify failed for {uid}, rolled back"));
        }
    }

    Ok(IngestResult { pages: created, deduped: None })
}

/// 按 source_hash 找既有 Page（对齐 find_by_source_hash：扫全部 Page frontmatter）。
fn find_by_source_hash(nodes_dir: &Path, hash: &str) -> Option<(String, String)> {
    use crate::model::{FM_SOURCE_HASH, FM_SOURCE_HASHES, parse_doc};
    let walker = ignore::WalkBuilder::new(nodes_dir.join("pages")).hidden(false).build();
    for entry in walker.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e != "md").unwrap_or(true) || !path.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let (fm, _) = parse_doc(&text);
        if crate::model::fm_get_str(&fm, FM_SOURCE_HASH).as_deref() == Some(hash) {
            return Some((
                crate::model::fm_get_str(&fm, FM_UID).unwrap_or_default(),
                crate::model::fm_get_str(&fm, FM_TITLE).unwrap_or_default(),
            ));
        }
        if let Some(hashes) = fm.get(serde_yaml::Value::String(FM_SOURCE_HASHES.into()))
            && let Some(list) = hashes.as_sequence()
            && list.iter().any(|v| v.as_str() == Some(hash))
        {
            return Some((
                crate::model::fm_get_str(&fm, FM_UID).unwrap_or_default(),
                crate::model::fm_get_str(&fm, FM_TITLE).unwrap_or_default(),
            ));
        }
    }
    None
}

/// 原子写（对齐 CONST-ARCH-5：tmp + rename）。
fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("md.tmp.{}", gen_uid()));
    std::fs::write(&tmp, content).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

// Mapping 插入 helpers（保插入序；与 xu-wiki dict 顺序语义一致）
fn fm_insert_str(fm: &mut Mapping, key: &str, value: &str) {
    fm.insert(serde_yaml::Value::String(key.into()), serde_yaml::Value::String(value.into()));
}
fn fm_insert_bool(fm: &mut Mapping, key: &str, value: bool) {
    fm.insert(serde_yaml::Value::String(key.into()), serde_yaml::Value::Bool(value));
}
fn fm_insert_i64(fm: &mut Mapping, key: &str, value: i64) {
    fm.insert(serde_yaml::Value::String(key.into()), serde_yaml::Value::Number(value.into()));
}
fn fm_insert_u64(fm: &mut Mapping, key: &str, value: u64) {
    fm.insert(serde_yaml::Value::String(key.into()), serde_yaml::Value::Number(value.into()));
}
fn fm_insert_list<T: serde::Serialize>(fm: &mut Mapping, key: &str, items: &[T]) {
    let arr: Vec<serde_yaml::Value> = items
        .iter()
        .filter_map(|it| serde_yaml::to_value(it).ok())
        .collect();
    fm.insert(serde_yaml::Value::String(key.into()), serde_yaml::Value::Sequence(arr));
}

/// 读节点文件为 Node（供 verify/工具层复用）。
pub fn read_node_file(path: &Path) -> Result<Node, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (fm, body) = crate::model::parse_doc(&text);
    let rel = path
        .to_string_lossy()
        .replace('\\', "/");
    Ok(Node { rel_path: rel, fm, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Layer;

    fn tmp_root() -> PathBuf {
        let d = std::env::temp_dir().join(format!("bm-wiki-ing-{}", gen_uid()));
        std::fs::create_dir_all(d.join("nodes/pages")).unwrap();
        std::fs::create_dir_all(d.join("raws")).unwrap();
        d
    }

    #[test]
    fn text_ingest_single_page() {
        let root = tmp_root();
        let res = commit(
            &root.join("nodes"),
            &root.join("raws"),
            &IngestRequest {
                title: "Python 简介".into(),
                content: "Python 是高级编程语言。".into(),
                node_path: String::new(),
                source_file: None,
            },
        )
        .unwrap();
        assert_eq!(res.pages.len(), 1);
        assert!(res.deduped.is_none());
        let p = &res.pages[0];
        assert!(p.md_path.starts_with("nodes/pages/python-简介-"));
        // 文件可读且 uid 匹配
        let node = read_node_file(&root.join(&p.md_path)).unwrap();
        assert_eq!(node.uid().unwrap(), p.uid);
        assert_eq!(node.title(), "Python 简介");
        assert_eq!(node.layer(), Layer::Page);
        assert_eq!(node.patches().len(), 1);
        assert_eq!(node.patches()[0].op, "create");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn multi_page_splits_with_chain() {
        let root = tmp_root();
        let body: String = (1..=700).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let res = commit(
            &root.join("nodes"),
            &root.join("raws"),
            &IngestRequest {
                title: "长文".into(),
                content: body,
                node_path: "docs".into(),
                source_file: None,
            },
        )
        .unwrap();
        assert_eq!(res.pages.len(), 3);
        let first = &res.pages[0];
        assert_eq!(first.split_index, 1);
        let n0 = read_node_file(&root.join(&first.md_path)).unwrap();
        let n1 = read_node_file(&root.join(&res.pages[1].md_path)).unwrap();
        assert_eq!(n0.fm.get("parent_uid").and_then(|v| v.as_str()).unwrap(), first.uid);
        assert_eq!(
            n1.fm.get("parent_uid").and_then(|v| v.as_str()).unwrap(),
            first.uid
        );
        assert!(n1.title().contains("(part 2/3)"));
        assert!(first.md_path.contains("-1-"));
        assert!(res.pages[1].md_path.contains("-2-"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn file_ingest_dedup_and_raw() {
        let root = tmp_root();
        let src = std::env::temp_dir().join(format!("src-{}.md", gen_uid()));
        std::fs::write(&src, "源文件内容\n第二行").unwrap();
        let req = IngestRequest {
            title: "源文档".into(),
            content: std::fs::read_to_string(&src).unwrap(),
            node_path: String::new(),
            source_file: Some(src.clone()),
        };
        let res = commit(&root.join("nodes"), &root.join("raws"), &req).unwrap();
        assert!(res.deduped.is_none());
        // 再次提交同一源 → 去重命中
        let res2 = commit(&root.join("nodes"), &root.join("raws"), &req).unwrap();
        let dedup = res2.deduped.unwrap();
        assert_eq!(dedup.existing_uid, res.pages[0].uid);
        // raws 副本存在
        let node = read_node_file(&root.join(&res.pages[0].md_path)).unwrap();
        let raw_path = node.fm.get("raw_path").and_then(|v| v.as_str()).unwrap();
        assert!(root.join(raw_path).is_file());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&src);
    }

    #[test]
    fn validation_rejects_bad_input() {
        let root = tmp_root();
        let r = commit(
            &root.join("nodes"),
            &root.join("raws"),
            &IngestRequest {
                title: "".into(),
                content: "x".into(),
                node_path: String::new(),
                source_file: None,
            },
        );
        assert!(r.is_err());
        let r = commit(
            &root.join("nodes"),
            &root.join("raws"),
            &IngestRequest {
                title: "t".into(),
                content: "  ".into(),
                node_path: "../escape".into(),
                source_file: None,
            },
        );
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_after_split_rejected() {
        let root = tmp_root();
        let r = commit(
            &root.join("nodes"),
            &root.join("raws"),
            &IngestRequest {
                title: "空白".into(),
                content: "\n\n  \n".into(),
                node_path: String::new(),
                source_file: None,
            },
        );
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
