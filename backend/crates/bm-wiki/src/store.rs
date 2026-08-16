//! WikiStore 门面：库定位/创建/树/读/写（线程安全：写操作 Mutex 串行化）。
//!
//! 库布局（三件套 PRIN-ARCH-23，对齐 xu-wiki create.py）：
//! ```text
//! <root>/
//! ├── raws/                        # 源文件副本
//! ├── nodes/{pages,lists,reports,entities}/{node_path}/*.md
//! └── .xu/{config.yaml, audit.jsonl}
//! ```

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::model::{Layer, Node, Relation};
use crate::query::ScoredBlock;

/// 库错误。
#[derive(Debug)]
pub enum StoreError {
    /// wiki 不存在（routes 层据此给前端建库引导）。
    NotFound(String),
    /// 参数/语义错误（消息可直接展示）。
    Invalid(String),
    /// IO 错误。
    Io(std::io::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::NotFound(m) => write!(f, "wiki not found: {m}"),
            StoreError::Invalid(m) => write!(f, "{m}"),
            StoreError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

/// 树条目（前端节点树；created_at 降序）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct TreeEntry {
    pub uid: String,
    pub title: String,
    pub layer: String,
    pub node_path: String,
    pub content_type: String,
    pub created_at: i64,
    pub active: bool,
}

/// 四分区树。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Tree {
    pub pages: Vec<TreeEntry>,
    pub lists: Vec<TreeEntry>,
    pub reports: Vec<TreeEntry>,
    pub entities: Vec<TreeEntry>,
}

/// 库状态（status 端点）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub exists: bool,
    pub root: String,
    pub counts: serde_json::Value,
}

pub struct WikiStore {
    root: PathBuf,
    /// 写操作串行化（ingest/关系/学习层写同一批 frontmatter 文件）。
    write_lock: Mutex<()>,
}

impl WikiStore {
    /// 打开已有库（.xu/config.yaml 存在才成功）。
    pub fn at(root: PathBuf) -> StoreResult<Self> {
        if !is_wiki_root(&root) {
            return Err(StoreError::NotFound(root.display().to_string()));
        }
        Ok(Self { root, write_lock: Mutex::new(()) })
    }

    /// 建库（三件套 + config.yaml + audit.jsonl）。已存在则幂等成功。
    pub fn create(root: &Path, name: &str) -> StoreResult<()> {
        if is_wiki_root(root) {
            return Ok(());
        }
        for d in [
            "raws",
            "nodes/pages",
            "nodes/lists",
            "nodes/reports",
            "nodes/entities",
            ".xu",
        ] {
            std::fs::create_dir_all(root.join(d)).map_err(StoreError::Io)?;
        }
        let cfg = crate::default_wiki_config(name);
        let cfg_text = serde_yaml::to_string(&cfg).map_err(|e| StoreError::Invalid(e.to_string()))?;
        std::fs::write(root.join(".xu/config.yaml"), cfg_text).map_err(StoreError::Io)?;
        // audit.jsonl 占位（对齐三件套语义）
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join(".xu/audit.jsonl"));
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn status(&self) -> StoreResult<Status> {
        let counts = self.tree()?;
        Ok(Status {
            exists: true,
            root: self.root.display().to_string(),
            counts: serde_json::json!({
                "pages": counts.pages.len(),
                "lists": counts.lists.len(),
                "reports": counts.reports.len(),
                "entities": counts.entities.len(),
            }),
        })
    }

    /// 四分区树（created_at 降序，对齐 cmd_nodes 排序）。
    pub fn tree(&self) -> StoreResult<Tree> {
        let nodes = self.nodes_dir();
        let mut pages = Vec::new();
        let mut lists = Vec::new();
        let mut reports = Vec::new();
        let mut entities = Vec::new();
        for entry in scan_md(&nodes) {
            let Ok(text) = std::fs::read_to_string(&entry) else { continue };
            let (fm, _) = crate::model::parse_doc(&text);
            let Some(uid) = crate::model::fm_get_str(&fm, crate::model::FM_UID) else {
                continue;
            };
            let layer = Layer::parse(
                crate::model::fm_get_str(&fm, crate::model::FM_LAYER)
                    .as_deref()
                    .unwrap_or(""),
            );
            let item = TreeEntry {
                uid,
                title: crate::model::fm_get_str(&fm, crate::model::FM_TITLE).unwrap_or_default(),
                layer: layer.as_str().into(),
                node_path: crate::model::fm_get_str(&fm, crate::model::FM_NODE_PATH)
                    .unwrap_or_default(),
                content_type: crate::model::fm_get_str(&fm, crate::model::FM_CONTENT_TYPE)
                    .unwrap_or_else(|| "article".into()),
                created_at: crate::model::fm_get_i64(&fm, crate::model::FM_CREATED).unwrap_or(0),
                active: crate::model::fm_get_bool(&fm, crate::model::FM_ACTIVE, true),
            };
            match layer {
                Layer::Page => pages.push(item),
                Layer::List => lists.push(item),
                Layer::Report => reports.push(item),
                Layer::Entity => entities.push(item),
            }
        }
        let sort = |v: &mut Vec<TreeEntry>| {
            v.sort_by_key(|e| std::cmp::Reverse(e.created_at))
        };
        sort(&mut pages);
        sort(&mut lists);
        sort(&mut reports);
        sort(&mut entities);
        Ok(Tree { pages, lists, reports, entities })
    }

    /// 读节点全文（不触碰关系 LRU——阅读不动图；触碰是 expand 语义）。
    pub fn read(&self, uid: &str) -> StoreResult<Node> {
        crate::query::find_by_uid(&self.nodes_dir(), uid)
            .ok_or_else(|| StoreError::NotFound(format!("node {uid}")))
    }

    /// 文本/文件摄取（写锁内执行）。
    pub fn ingest(&self, req: crate::ingest::IngestRequest) -> StoreResult<crate::ingest::IngestResult> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        crate::ingest::commit(&self.nodes_dir(), &self.raws_dir(), &req)
            .map_err(StoreError::Invalid)
    }

    /// 关键词检索（无锁，只读）。
    pub fn query(&self, keywords: &[String]) -> StoreResult<Vec<ScoredBlock>> {
        Ok(crate::query::scan_all(&self.nodes_dir(), keywords))
    }

    /// 添加关系（写锁内；写回 frontmatter）。
    pub fn add_relation(
        &self,
        from_uid: &str,
        to_uid: &str,
        relation_name: &str,
        comment: &str,
    ) -> StoreResult<serde_json::Value> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut node = self.read(from_uid)?;
        if !crate::uid::is_valid_uid(to_uid) {
            return Err(StoreError::Invalid("invalid to_uid".into()));
        }
        let (action, evicted) = crate::relations::add_relation(
            &mut node.fm,
            from_uid,
            to_uid,
            relation_name,
            comment,
            crate::model::MAX_EDGES,
        );
        if action == "self-loop-rejected" {
            return Err(StoreError::Invalid("self-loop relation rejected".into()));
        }
        self.write_node(&node)?;
        Ok(serde_json::json!({
            "action": action,
            "evicted": evicted.map(|e| e.to_uid),
        }))
    }

    /// 删除关系。返回是否命中。
    pub fn remove_relation(&self, from_uid: &str, to_uid: &str, relation_name: &str) -> StoreResult<bool> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut node = self.read(from_uid)?;
        let removed = crate::relations::remove_relation(&mut node.fm, to_uid, relation_name);
        if removed {
            self.write_node(&node)?;
        }
        Ok(removed)
    }

    /// 节点出边（读 frontmatter；不触碰）。
    pub fn relations(&self, uid: &str) -> StoreResult<Vec<Relation>> {
        let node = self.read(uid)?;
        Ok(crate::relations::relations_from_mapping(&node.fm))
    }

    /// expand 触碰（PRIN-ARCH-10）：命中关系前挪一位，写回 frontmatter。
    /// 返回实际触碰的条数。
    pub fn touch_relations(&self, uid: &str, to_uids: &[&str]) -> StoreResult<usize> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut node = self.read(uid)?;
        let mut touched = 0;
        for to in to_uids {
            if crate::relations::touch(&mut node.fm, to) {
                touched += 1;
            }
        }
        if touched > 0 {
            self.write_node(&node)?;
        }
        Ok(touched)
    }

    /// 创建学习层节点。
    pub fn create_layer(&self, req: crate::layers::LayerCreate) -> StoreResult<Node> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        crate::layers::create(&self.nodes_dir(), &req).map_err(StoreError::Invalid)
    }

    /// 修改学习层节点（Page 拒绝）。
    pub fn update_node(&self, uid: &str, title: Option<&str>, body: Option<&str>) -> StoreResult<Node> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut node = self.read(uid)?;
        crate::layers::update(&mut node, title, body).map_err(StoreError::Invalid)?;
        self.write_node(&node)?;
        Ok(node)
    }

    /// 追加 Page 修订（不可变原则的唯一修订通道）。
    pub fn append_patch(&self, uid: &str, op: &str, delta: &str) -> StoreResult<Node> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut node = self.read(uid)?;
        crate::layers::append_patch(&mut node, op, delta);
        self.write_node(&node)?;
        Ok(node)
    }

    fn nodes_dir(&self) -> PathBuf {
        self.root.join("nodes")
    }

    fn raws_dir(&self) -> PathBuf {
        self.root.join("raws")
    }

    /// 原子写回节点文件（tmp + rename）。
    fn write_node(&self, node: &Node) -> StoreResult<()> {
        let path = self.root.join(std::path::Path::new(&node.rel_path));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(StoreError::Io)?;
        }
        let doc = crate::model::render_doc(&node.fm, &node.body);
        let tmp = path.with_extension(format!("md.tmp.{}", crate::uid::gen_uid()));
        std::fs::write(&tmp, &doc).map_err(StoreError::Io)?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            StoreError::Io(e)
        })
    }
}

/// 库根判定（.xu/config.yaml 存在，对齐 is_wiki_root）。
pub fn is_wiki_root(root: &Path) -> bool {
    root.join(".xu").join("config.yaml").is_file()
}

/// 遍历 nodes 下全部 .md（忽略 hidden，尊重 .ignore）。
fn scan_md(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    for entry in ignore::WalkBuilder::new(dir).hidden(false).build().flatten() {
        let p = entry.path();
        if p.extension().map(|e| e == "md").unwrap_or(false) && p.is_file() {
            out.push(p.to_path_buf());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_wiki() -> PathBuf {
        let d = std::env::temp_dir().join(format!("bm-wiki-st-{}", crate::uid::gen_uid()));
        WikiStore::create(&d, "test").unwrap();
        d
    }

    #[test]
    fn create_then_at() {
        let root = tmp_wiki();
        let store = WikiStore::at(root.clone()).unwrap();
        assert!(store.status().unwrap().exists);
        assert!(WikiStore::at(root.join("nope")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn create_idempotent() {
        let root = tmp_wiki();
        assert!(WikiStore::create(&root, "again").is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ingest_query_relation_roundtrip() {
        let root = tmp_wiki();
        let store = WikiStore::at(root.clone()).unwrap();
        let res = store
            .ingest(crate::ingest::IngestRequest {
                title: "Rust 语言".into(),
                content: "Rust 是系统编程语言，注重内存安全。\n所有权机制是核心。".into(),
                node_path: String::new(),
                source_file: None,
            })
            .unwrap();
        let uid = &res.pages[0].uid;
        assert_eq!(store.tree().unwrap().pages.len(), 1);

        // 检索
        let hits = store.query(&["rust".to_string()]).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].uid, *uid);

        // 读
        let node = store.read(uid).unwrap();
        assert_eq!(node.title(), "Rust 语言");

        // 关系
        let rel = store.add_relation(uid, "AAAA1111", "参访", "测试").unwrap();
        assert_eq!(rel["action"], "created");
        assert_eq!(store.relations(uid).unwrap().len(), 1);

        // 删除关系
        assert!(store.remove_relation(uid, "AAAA1111", "参访").unwrap());
        assert!(!store.remove_relation(uid, "AAAA1111", "参访").unwrap());

        // 学习层
        let entity = store
            .create_layer(crate::layers::LayerCreate {
                layer: Layer::Entity,
                title: "所有权".into(),
                body: "Rust 内存安全核心概念".into(),
                node_path: String::new(),
                members: vec![],
                references: vec![],
                source_page: Some(uid.clone()),
            })
            .unwrap();
        assert_eq!(store.tree().unwrap().entities.len(), 1);
        let updated = store.update_node(&entity.uid().unwrap(), Some("所有权机制"), None).unwrap();
        assert_eq!(updated.title(), "所有权机制");

        // Page 不可变
        assert!(store.update_node(uid, Some("x"), Some("y")).is_err());

        // patches 追加
        let patched = store.append_patch(uid, "patch", "补充说明").unwrap();
        assert_eq!(patched.patches().len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn relations_persist_across_store_opens() {
        let root = tmp_wiki();
        {
            let store = WikiStore::at(root.clone()).unwrap();
            let res = store
                .ingest(crate::ingest::IngestRequest {
                    title: "T".into(),
                    content: "body".into(),
                    node_path: String::new(),
                    source_file: None,
                })
                .unwrap();
            store.add_relation(&res.pages[0].uid, "BBBB9999", "引用", "").unwrap();
        }
        {
            let store = WikiStore::at(root.clone()).unwrap();
            let node = store.tree().unwrap().pages.remove(0);
            assert_eq!(store.relations(&node.uid).unwrap().len(), 1);
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
