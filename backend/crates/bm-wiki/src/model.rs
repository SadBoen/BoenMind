//! 节点模型与 frontmatter 契约（字段名/布局对齐 xu-wiki 01-wiki-architecture.md）。
//!
//! 存储格式：`nodes/{pages|lists|reports|entities}/{node_path}/{slug}-{uid}.md`，
//! `---\n{YAML frontmatter}\n---\n\n{body}`。frontmatter 以 `serde_yaml::Mapping`
//! 动态持有（保插入序 + 未知字段透传），与 xu-wiki 的 dict 语义一致——任何
//! 额外字段（如相册 attrs）写回不丢。

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

// ── frontmatter 字段名（对齐 xu-wiki utils/constants.py）──
pub const FM_UID: &str = "uid";
pub const FM_TITLE: &str = "title";
pub const FM_LAYER: &str = "layer";
pub const FM_CONTENT_TYPE: &str = "content_type";
pub const FM_ACTIVE: &str = "active";
pub const FM_CREATED: &str = "created_at";
pub const FM_CONTENT_HASH: &str = "content_hash";
pub const FM_NODE_PATH: &str = "node_path";
pub const FM_RAW_PATH: &str = "raw_path";
pub const FM_SOURCE_HASH: &str = "source_hash";
pub const FM_SOURCE_HASHES: &str = "source_hashes";
pub const FM_SPLIT_INDEX: &str = "split_index";
pub const FM_PARENT_UID: &str = "parent_uid";
pub const FM_RELATIONS: &str = "relations";
pub const FM_PATCHES: &str = "patches";
pub const FM_EVIDENCE: &str = "references";
pub const FM_MEMBERS: &str = "members";

/// 体验参照默认值（对齐 xu-wiki constants.py；均可在库内 config 调）。
pub const WIKI_FORMAT_VERSION: &str = "1.0.0";
/// Page 切分粒度：正文超 300 行切页（DESIGN-ARCH-2）。
pub const PAGE_SPLIT_LINES: usize = 300;
/// 每节点出边上限（PRIN-ARCH-7~9，LRU 满 50 弹队尾）。
pub const MAX_EDGES: usize = 50;
/// 检索打分：标题命中 ×5 + body 命中 + 层权重（PRIN-ARCH-13）。
pub const TITLE_HIT_WEIGHT: f64 = 5.0;

/// 四类节点层（PRIN-ARCH-1 两层各司其职：Page=知识层，List/Report/Entity=学习层）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Layer {
    Page,
    List,
    Report,
    Entity,
}

impl Layer {
    /// 检索层权重（Entity=2, Report=3, List=1, Page=0）。
    pub fn bonus(self) -> f64 {
        match self {
            Layer::Page => 0.0,
            Layer::List => 1.0,
            Layer::Entity => 2.0,
            Layer::Report => 3.0,
        }
    }

    /// 节点目录名（nodes 下四分区）。
    pub fn dir_name(self) -> &'static str {
        match self {
            Layer::Page => "pages",
            Layer::List => "lists",
            Layer::Report => "reports",
            Layer::Entity => "entities",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Page => "Page",
            Layer::List => "List",
            Layer::Report => "Report",
            Layer::Entity => "Entity",
        }
    }

    pub fn parse(s: &str) -> Layer {
        match s {
            "List" => Layer::List,
            "Report" => Layer::Report,
            "Entity" => Layer::Entity,
            _ => Layer::Page,
        }
    }
}

/// 关系出边条目（frontmatter `relations` 元素；LRU 位置 = position，队首 0）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub to_uid: String,
    pub relation_name: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub position: u32,
}

/// 修订条目（frontmatter `patches` 元素；Page 不可变，修订叠加）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub op: String,
    pub delta: String,
    #[serde(default)]
    pub created_at: i64,
}

/// 证据链条目（Report 强制 ≥1；frontmatter `references` 元素）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefEntry {
    pub ref_uid: String,
    #[serde(default)]
    pub note: String,
}

/// List 成员条目（frontmatter `members` 元素）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub uid: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub position: u32,
}

/// 一个节点 = 磁盘上的 .md 文件（frontmatter 动态 Mapping + 正文）。
#[derive(Debug, Clone)]
pub struct Node {
    /// 相对库根的路径（nodes/pages/xxx.md；**正斜杠**，跨平台 + xu-wiki 契约）。
    pub rel_path: String,
    pub fm: Mapping,
    pub body: String,
}

impl Node {
    pub fn uid(&self) -> Option<String> {
        fm_get_str(&self.fm, FM_UID)
    }
    pub fn title(&self) -> String {
        fm_get_str(&self.fm, FM_TITLE).unwrap_or_default()
    }
    pub fn layer(&self) -> Layer {
        Layer::parse(fm_get_str(&self.fm, FM_LAYER).as_deref().unwrap_or(""))
    }
    pub fn content_type(&self) -> String {
        fm_get_str(&self.fm, FM_CONTENT_TYPE).unwrap_or_else(|| "article".into())
    }
    pub fn active(&self) -> bool {
        fm_get_bool(&self.fm, FM_ACTIVE, true)
    }
    pub fn created_at(&self) -> i64 {
        fm_get_i64(&self.fm, FM_CREATED).unwrap_or(0)
    }
    pub fn node_path(&self) -> String {
        fm_get_str(&self.fm, FM_NODE_PATH).unwrap_or_default()
    }
    pub fn relations(&self) -> Vec<Relation> {
        parse_list(&self.fm, FM_RELATIONS)
    }
    pub fn patches(&self) -> Vec<Patch> {
        parse_list(&self.fm, FM_PATCHES)
    }
    pub fn members(&self) -> Vec<Member> {
        parse_list(&self.fm, FM_MEMBERS)
    }
    pub fn references(&self) -> Vec<RefEntry> {
        parse_list(&self.fm, FM_EVIDENCE)
    }
}

// ── frontmatter 解析/渲染（对齐 xu-wiki utils/frontmatter.py）──

/// 拆 `---\n{fm}\n---\n\n{body}` 为 (frontmatter Mapping, body)。
pub fn parse_doc(text: &str) -> (Mapping, String) {
    if !text.starts_with("---") {
        return (Mapping::new(), text.to_string());
    }
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        return (Mapping::new(), text.to_string());
    }
    let mut end = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            end = Some(i);
            break;
        }
    }
    let Some(end) = end else {
        return (Mapping::new(), text.to_string());
    };
    let fm_text = lines[1..end].join("\n");
    let mut body = lines[end + 1..].join("\n");
    if body.starts_with('\n') {
        body = body[1..].to_string();
    }
    let fm = serde_yaml::from_str::<Value>(&fm_text)
        .ok()
        .and_then(|v| v.as_mapping().cloned())
        .unwrap_or_default();
    (fm, body)
}

/// 渲染完整文档（body rstrip + 末尾换行；与 xu-wiki render 一致）。
pub fn render_doc(fm: &Mapping, body: &str) -> String {
    let fm_text = serde_yaml::to_string(fm)
        .unwrap_or_default()
        .trim_end()
        .to_string();
    format!("---\n{fm_text}\n---\n\n{}\n", body.trim_end())
}

// ── Mapping 字段 helpers（读写时类型转换）──

pub fn fm_get_str(fm: &Mapping, key: &str) -> Option<String> {
    fm.get(Value::String(key.into()))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

pub fn fm_get_bool(fm: &Mapping, key: &str, default: bool) -> bool {
    match fm.get(Value::String(key.into())) {
        Some(Value::Bool(b)) => *b,
        _ => default,
    }
}

pub fn fm_get_i64(fm: &Mapping, key: &str) -> Option<i64> {
    fm.get(Value::String(key.into())).and_then(|v| v.as_i64())
}

pub fn fm_set_str(fm: &mut Mapping, key: &str, value: impl Into<String>) {
    fm.insert(Value::String(key.into()), Value::String(value.into()));
}

pub fn fm_set_bool(fm: &mut Mapping, key: &str, value: bool) {
    fm.insert(Value::String(key.into()), Value::Bool(value));
}

pub fn fm_set_i64(fm: &mut Mapping, key: &str, value: i64) {
    fm.insert(Value::String(key.into()), Value::Number(value.into()));
}

pub fn fm_remove(fm: &mut Mapping, key: &str) {
    fm.shift_remove(Value::String(key.into()));
}

/// 解析 frontmatter 中某键为结构化列表（缺失/类型不符 → 空）。
pub fn parse_list<T: for<'de> Deserialize<'de>>(fm: &Mapping, key: &str) -> Vec<T> {
    let Some(v) = fm.get(Value::String(key.into())) else {
        return Vec::new();
    };
    let v = match v {
        Value::Sequence(_) => v.clone(),
        Value::Null => return Vec::new(),
        _ => return Vec::new(),
    };
    serde_json::from_value(to_json_value(v)).unwrap_or_default()
}

/// 把序列化的结构体列表写回 frontmatter 键。
pub fn set_list<T: Serialize>(fm: &mut Mapping, key: &str, items: &[T]) {
    let arr: Vec<Value> = items
        .iter()
        .filter_map(|it| serde_yaml::to_value(it).ok())
        .collect();
    fm.insert(Value::String(key.into()), Value::Sequence(arr));
}

/// serde_yaml Value → serde_json Value（结构体序列化统一走 JSON 边界）。
pub fn to_json_value(v: Value) -> serde_json::Value {
    serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)
}

pub fn to_json_mapping(fm: &Mapping) -> serde_json::Value {
    serde_json::to_value(fm).unwrap_or(serde_json::Value::Null)
}

/// 当前 unix 秒（对齐 xu-wiki now_ts）。
pub fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// SHA-256 hex（正文/源文件哈希）。
pub fn sha256_text(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    hex(h.finalize().as_slice())
}

pub fn sha256_bytes(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    hex(h.finalize().as_slice())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 规范化 node_path（BAN-ARCH-7）：相对逻辑分区，拒绝绝对路径/`..`。
pub fn safe_node_path(node_path: &str) -> Result<String, String> {
    let np = node_path.trim().replace('\\', "/").trim_matches('/').to_string();
    if np.is_empty() {
        return Ok(String::new());
    }
    if np.starts_with('/') || np.contains(':') {
        return Err(format!("node_path must be relative: {node_path:?}"));
    }
    if np.split('/').any(|part| part == "..") {
        return Err(format!("node_path must not contain '..': {node_path:?}"));
    }
    Ok(np)
}

/// slug 化文件名（对齐 xu-wiki safe_slug：非词字符 → `-`，80 上限）。
pub fn safe_slug(text: &str, maxlen: usize) -> String {
    let mut s = String::with_capacity(text.len());
    let mut last_dash = false;
    for c in text.trim().chars().flat_map(|c| c.to_lowercase()) {
        if c.is_alphanumeric() || c == '_' {
            s.push(c);
            last_dash = false;
        } else if !last_dash && !s.is_empty() {
            s.push('-');
            last_dash = true;
        }
    }
    while s.ends_with('-') {
        s.pop();
    }
    if s.is_empty() {
        s = "untitled".into();
    }
    s.truncate(maxlen);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_roundtrip() {
        let doc = "---\nuid: ABC12345\ntitle: 测试\nlayer: Page\nactive: true\n---\n\n正文第一行\n";
        let (fm, body) = parse_doc(doc);
        assert_eq!(fm_get_str(&fm, "uid").unwrap(), "ABC12345");
        assert_eq!(fm_get_str(&fm, "title").unwrap(), "测试");
        assert!(fm_get_bool(&fm, "active", false));
        assert_eq!(body, "正文第一行\n");

        let rendered = render_doc(&fm, &body);
        let (fm2, body2) = parse_doc(&rendered);
        assert_eq!(fm2, fm);
        assert_eq!(body2, body);
    }

    #[test]
    fn no_frontmatter_returns_raw_body() {
        let (fm, body) = parse_doc("plain text\nno frontmatter");
        assert!(fm.is_empty());
        assert_eq!(body, "plain text\nno frontmatter");
    }

    #[test]
    fn layer_bonus_order() {
        assert!(Layer::Report.bonus() > Layer::Entity.bonus());
        assert!(Layer::Entity.bonus() > Layer::List.bonus());
        assert!(Layer::List.bonus() > Layer::Page.bonus());
    }

    #[test]
    fn relations_roundtrip_through_mapping() {
        let mut fm = Mapping::new();
        let rels = vec![Relation {
            to_uid: "ZZZZ0001".into(),
            relation_name: "参访".into(),
            comment: String::new(),
            created_at: 1,
            position: 0,
        }];
        set_list(&mut fm, FM_RELATIONS, &rels);
        let out: Vec<Relation> = parse_list(&fm, FM_RELATIONS);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].to_uid, "ZZZZ0001");
        assert_eq!(out[0].relation_name, "参访");
        // 序列化保序（insertion order）
        assert!(serde_yaml::to_string(&fm).unwrap().contains("to_uid: ZZZZ0001"));
    }

    #[test]
    fn slug_cleanup() {
        assert_eq!(safe_slug("Hello World! 你好", 80), "hello-world-你好");
        assert_eq!(safe_slug("", 80), "untitled");
        assert_eq!(safe_slug("--x--", 80), "x");
        assert_eq!(safe_slug("a".repeat(200).as_str(), 80).len(), 80);
    }

    #[test]
    fn node_path_validation() {
        assert_eq!(safe_node_path("papers/ml").unwrap(), "papers/ml");
        assert_eq!(safe_node_path(" /a/b/ ").unwrap(), "a/b");
        assert!(safe_node_path("..").is_err());
        assert!(safe_node_path("a/../b").is_err());
        assert!(safe_node_path("C:\\evil").is_err());
    }

    #[test]
    fn sha_known_vector() {
        assert_eq!(sha256_text("hello"), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }
}
