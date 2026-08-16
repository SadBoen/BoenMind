//! 记忆插件（最小实现，架构 §6.1 memory-file 雏形）：facts.md 文件传送带。
//!
//! 最小闭环——验证核心挂点（v0.17 双向奔赴：插件边写边测核心）：
//! - **注入**：[`LoopHooks::on_request`] 把已记住的事实作为追加 system 段
//!   注入模型请求（核心挂点 on_request 的第一个真实使用者）；
//! - **记忆**：[`MemoryFilePlugin::remember`] 公共 API（调用方 = 集成方 /
//!   未来 Steward 的 governance.memorize——策略在插件，核心零改动）；
//! - **持久化**：facts.md 一行一条事实，指纹去重（同内容不重复写）；
//! - **跨会话**：构造时加载 facts.md——新会话自动带上前世的记忆，
//!   不随上下文压缩/会话重建消失。
//!
//! 记忆桶（2026-08-16 专家接线）：目录模式下每个桶一个文件
//! `memory/<bucket>.md`（默认桶 `facts` = 历史 facts.md 路径不变）。
//! 专家模板自动绑定同名桶——角色记忆互不污染；删除专家保留桶
//! （记忆资产），使用记录由 experts 模块写 memory/usage.json。
//!
//! 不做（超出最小验证，二期见 §6.1 表）：LLM 摘要、向量检索、淡化分级
//! （Hana 传送带 today→week→longterm）、memory/write 事件投影（Steward 接）。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bm_loop::points::{LoopHooks, RequestCtx};

/// 默认记忆桶（对应历史 `memory/facts.md`）。
pub const DEFAULT_BUCKET: &str = "facts";

/// 记忆插件：文件传送带 + 注入（目录模式，每桶一个文件）。
///
/// 桶文件格式：每行一条事实（空行忽略），加载时按行读回——
/// 文件可手改（用户可直接编辑，指纹去重防重复写入）。
pub struct MemoryFilePlugin {
    /// 记忆目录（桶文件 = `dir/<bucket>.md`）。
    dir: PathBuf,
    /// 默认桶（`open` 兼容构造时从文件名推导；目录构造为 `facts`）。
    bucket: String,
    /// 注入上限（条数，超出截断——记忆注入是有界字符的原则，对齐
    /// Hermes MemoryProvider 的 system_prompt_block）
    max_facts: usize,
}

impl MemoryFilePlugin {
    /// 兼容构造：单文件路径 → 目录 + 桶名（文件名去 .md）。
    pub fn open(path: PathBuf, max_facts: usize) -> Self {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let bucket = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BUCKET.to_string());
        Self {
            dir,
            bucket,
            max_facts,
        }
    }

    /// 目录构造（默认桶 = facts，对应 `dir/facts.md`）。
    pub fn open_dir(dir: PathBuf, max_facts: usize) -> Self {
        Self {
            dir,
            bucket: DEFAULT_BUCKET.to_string(),
            max_facts,
        }
    }

    /// 当前默认桶名。
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// 桶文件路径（不存在不创建）。
    pub fn bucket_path(&self, bucket: &str) -> PathBuf {
        self.dir.join(format!("{bucket}.md"))
    }

    /// 读取某桶现有事实（最旧在前；文件缺失 = 空）。
    fn load_bucket(&self, bucket: &str) -> VecDeque<String> {
        std::fs::read_to_string(self.bucket_path(bucket))
            .map(|s| {
                s.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(str::to_string)
                    .collect::<VecDeque<_>>()
            })
            .unwrap_or_default()
    }

    /// 记住一条事实（默认桶）：去重 → 追加 → 落盘。
    /// 失败只 warn（记忆是增强不是正确性依赖——fail-safe 对齐压缩）。
    pub fn remember(&mut self, fact: impl Into<String>) {
        let bucket = self.bucket.clone();
        self.remember_to(&bucket, fact);
    }

    /// 记住一条事实到指定桶（去重；文件缺失自动建目录）。
    pub fn remember_to(&mut self, bucket: &str, fact: impl Into<String>) {
        let fact = fact.into().trim().to_string();
        if fact.is_empty() || self.load_bucket(bucket).iter().any(|f| f == &fact) {
            return;
        }
        let path = self.bucket_path(bucket);
        let _ = path.parent().map(std::fs::create_dir_all);
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{fact}")
            })
            .inspect_err(|e| tracing::warn!(event = "bm.memory_write_failed", bucket, error = %e));
    }

    /// 当前事实（默认桶；最旧在前）。
    pub fn facts(&self) -> impl Iterator<Item = String> {
        self.facts_of(&self.bucket).into_iter()
    }

    /// 指定桶的事实（最旧在前）。
    pub fn facts_of(&self, bucket: &str) -> Vec<String> {
        self.load_bucket(bucket).into_iter().collect()
    }

    /// 注入块（system 追加段文本）：上限截断，空记忆 = 空串（不注入）。
    fn inject_block(&self, bucket: &str) -> String {
        let facts = self.load_bucket(bucket);
        if facts.is_empty() {
            return String::new();
        }
        let mut block = String::from("\n\n[长期记忆]");
        for fact in facts.iter().take(self.max_facts) {
            block.push_str("\n- ");
            block.push_str(fact);
        }
        block
    }

    /// 把注入块追加进 payload 的 system 消息（有则改、无则插）。
    fn inject_payload(&self, payload: &mut serde_json::Value, bucket: &str) {
        let block = self.inject_block(bucket);
        if block.is_empty() {
            return;
        }
        let messages = payload
            .get_mut("messages")
            .and_then(|m| m.as_array_mut())
            .expect("payload.messages 是数组（engine build_payload 保证）");
        if let Some(system) = messages
            .iter_mut()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        {
            let content = system.get_mut("content").expect("system 消息有 content");
            if let Some(s) = content.as_str() {
                *content = serde_json::json!(format!("{s}{block}"));
            }
        } else {
            messages.insert(0, serde_json::json!({"role": "system", "content": block}));
        }
    }

    /// 默认桶注入（`open` 单文件兼容构造与 LoopHooks 用）。
    pub fn inject_default(&self, payload: &mut serde_json::Value) {
        let bucket = self.bucket.clone();
        self.inject_payload(payload, &bucket);
    }
}

/// 挂进 loop：构造模型请求前注入记忆（核心挂点 on_request 的第一个真实使用者）。
/// 默认注入默认桶；会话级桶由组装层在构造时指定（专家接线）。
impl LoopHooks for MemoryFilePlugin {
    fn on_request(&mut self, _ctx: &RequestCtx, payload: &mut serde_json::Value) {
        self.inject_default(payload);
    }
}

/// 服务面适配（SERVICE_FACES 图纸 #3）：newtype 打破孤儿规则——
/// `MemoryPortAdapter` 内部持共享实例，实现 `MemoryPort`。
/// 组装层注册 "memory" 服务，插件/宿主经
/// `ctx.port::<dyn MemoryPort>("memory")` 取用（remember 入口）；
/// 请求注入仍走 LoopHooks 轨（双轨各自职责不变）。
pub struct MemoryPortAdapter(pub Arc<std::sync::Mutex<MemoryFilePlugin>>);

impl MemoryPortAdapter {
    /// 锁内执行（fail-safe：锁中毒跳过，不 panic）。
    fn with<T>(&self, f: impl FnOnce(&mut MemoryFilePlugin) -> T) -> Option<T> {
        match self.0.lock() {
            Ok(mut m) => Some(f(&mut m)),
            Err(_) => {
                tracing::warn!(event = "bm.memory_lock_failed", "记忆锁中毒，本次跳过");
                None
            }
        }
    }
}

impl bm_protocol::MemoryPort for MemoryPortAdapter {
    fn remember(&self, fact: String) {
        self.with(|m| m.remember(fact));
    }

    fn facts(&self) -> Vec<String> {
        self.with(|m| m.facts().collect()).unwrap_or_default()
    }

    fn inject_into_payload(&self, payload: &mut serde_json::Value) {
        self.with(|m| m.inject_default(payload));
    }

    fn remember_to(&self, bucket: &str, fact: String) {
        self.with(|m| m.remember_to(bucket, fact));
    }

    fn inject_bucket(&self, bucket: &str, payload: &mut serde_json::Value) {
        self.with(|m| m.inject_payload(payload, bucket));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_facts(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("bm-memory-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn remember_dedupes_and_persists_across_reopen() {
        let path = temp_facts("a.md");
        let _ = std::fs::remove_file(&path);
        {
            let mut m = MemoryFilePlugin::open(path.clone(), 10);
            m.remember("用户偏好：大屏显示器");
            m.remember("用户偏好：大屏显示器"); // 去重
            m.remember("项目：BoenMind 阶段 1");
            assert_eq!(m.facts().count(), 2);
        }
        // 跨会话：重新打开（模拟新会话）读回
        let m2 = MemoryFilePlugin::open(path.clone(), 10);
        let facts: Vec<String> = m2.facts().collect();
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&"用户偏好：大屏显示器".to_string()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn buckets_are_isolated_per_expert() {
        let dir = temp_facts("buckets");
        let _ = std::fs::remove_dir_all(&dir);
        let mut m = MemoryFilePlugin::open_dir(dir.clone(), 10);
        m.remember_to("coding-architect", "架构决策：用 turso");
        m.remember_to("coding-architect", "架构决策：用 turso"); // 去重
        m.remember_to("coding-coder", "实现：QuickJS 桥");
        assert_eq!(m.facts_of("coding-architect"), vec!["架构决策：用 turso"]);
        assert_eq!(m.facts_of("coding-coder"), vec!["实现：QuickJS 桥"]);
        assert!(m.facts_of("facts").is_empty(), "默认桶不受影响");
        // 跨实例读回（会话各自开实例也能读到同一桶）
        let m2 = MemoryFilePlugin::open_dir(dir.clone(), 10);
        assert_eq!(m2.facts_of("coding-architect"), vec!["架构决策：用 turso"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inject_appends_to_existing_system_message() {
        let path = temp_facts("b.md");
        let _ = std::fs::remove_file(&path);
        let mut m = MemoryFilePlugin::open(path.clone(), 10);
        m.remember("事实一");
        let mut payload = serde_json::json!({
            "messages": [
                {"role": "system", "content": "你是助手"},
                {"role": "user", "content": "hi"},
            ]
        });
        m.inject_default(&mut payload);
        let sys = payload["messages"][0]["content"].as_str().unwrap();
        assert!(sys.starts_with("你是助手"), "原文保留");
        assert!(sys.contains("[长期记忆]") && sys.contains("事实一"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn inject_bucket_uses_specified_bucket() {
        let dir = temp_facts("inject-bucket");
        let _ = std::fs::remove_dir_all(&dir);
        let mut m = MemoryFilePlugin::open_dir(dir.clone(), 10);
        m.remember_to("coding-reviewer", "审查发现：边界未校验");
        let mut payload = serde_json::json!({
            "messages": [{"role": "system", "content": "你是助手"}]
        });
        m.inject_payload(&mut payload, "coding-reviewer");
        let sys = payload["messages"][0]["content"].as_str().unwrap();
        assert!(sys.contains("审查发现：边界未校验"));
        assert!(sys.contains("[长期记忆]"), "注入块带记忆标题");
        // 默认桶无该事实
        let mut p2 = serde_json::json!({"messages": [{"role": "system", "content": "x"}]});
        m.inject_default(&mut p2);
        assert!(!p2["messages"][0]["content"].as_str().unwrap().contains("审查发现"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inject_inserts_system_message_when_missing() {
        let path = temp_facts("c.md");
        let _ = std::fs::remove_file(&path);
        let mut m = MemoryFilePlugin::open(path.clone(), 10);
        m.remember("事实二");
        let mut payload = serde_json::json!({
            "messages": [{"role": "user", "content": "hi"}]
        });
        m.inject_default(&mut payload);
        assert_eq!(payload["messages"][0]["role"], "system");
        assert!(payload["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("事实二"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_memory_injects_nothing() {
        let path = temp_facts("d.md");
        let _ = std::fs::remove_file(&path);
        let m = MemoryFilePlugin::open(path.clone(), 10);
        let mut payload = serde_json::json!({
            "messages": [{"role": "user", "content": "hi"}]
        });
        m.inject_default(&mut payload);
        assert_eq!(payload["messages"].as_array().unwrap().len(), 1, "空记忆不注入");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn inject_caps_at_max_facts() {
        let path = temp_facts("e.md");
        let _ = std::fs::remove_file(&path);
        let mut m = MemoryFilePlugin::open(path.clone(), 2);
        m.remember("一");
        m.remember("二");
        m.remember("三");
        let block = m.inject_block(&m.bucket.clone());
        assert!(block.contains("一") && block.contains("二"));
        assert!(!block.contains("三"), "上限截断");
        let _ = std::fs::remove_file(&path);
    }
}
