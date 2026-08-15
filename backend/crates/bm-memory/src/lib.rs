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
//! 不做（超出最小验证，二期见 §6.1 表）：LLM 摘要、向量检索、淡化分级
//! （Hana 传送带 today→week→longterm）、memory/write 事件投影（Steward 接）。
//!
//! 组装层（bm-server）把本插件与 StreamHooks 组合进 loop；本 crate 不依赖
//! bm-core/bm-server（插件依赖核心，方向守卫见 tests/architecture.rs）。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use bm_loop::points::{LoopHooks, RequestCtx};

/// 记忆插件：文件传送带 + 注入。
///
/// facts.md 格式：每行一条事实（空行忽略），加载时按行读回——
/// 文件可手改（用户可直接编辑，指纹去重防重复写入）。
pub struct MemoryFilePlugin {
    path: PathBuf,
    facts: VecDeque<String>,
    /// 注入上限（条数，超出截断——记忆注入是有界字符的原则，对齐
    /// Hermes MemoryProvider 的 system_prompt_block）
    max_facts: usize,
}

impl MemoryFilePlugin {
    /// 打开（不存在则创建）：加载已有事实，跨会话记忆由此而来。
    pub fn open(path: PathBuf, max_facts: usize) -> Self {
        // 目录不存在先建（remember 的 append 依赖）
        let _ = path.parent().map(std::fs::create_dir_all);
        let facts = std::fs::read_to_string(&path)
            .map(|s| {
                s.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(str::to_string)
                    .collect::<VecDeque<_>>()
            })
            .unwrap_or_default();
        Self {
            path,
            facts,
            max_facts,
        }
    }

    /// 记住一条事实：去重（同内容已有则跳过）→ 追加 → 落盘。
    /// 失败只 warn（记忆是增强不是正确性依赖——fail-safe 对齐压缩）。
    pub fn remember(&mut self, fact: impl Into<String>) {
        let fact = fact.into().trim().to_string();
        if fact.is_empty() || self.facts.iter().any(|f| f == &fact) {
            return;
        }
        self.facts.push_back(fact.clone());
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{fact}")
            })
            .inspect_err(|e| tracing::warn!(event = "bm.memory_write_failed", error = %e));
    }

    /// 当前事实（最旧在前）。
    pub fn facts(&self) -> impl Iterator<Item = &str> {
        self.facts.iter().map(String::as_str)
    }

    /// 注入块（system 追加段文本）：上限截断，空记忆 = 空串（不注入）。
    fn inject_block(&self) -> String {
        if self.facts.is_empty() {
            return String::new();
        }
        let mut block = String::from("\n\n[长期记忆]");
        for fact in self.facts.iter().take(self.max_facts) {
            block.push_str("\n- ");
            block.push_str(fact);
        }
        block
    }

    /// 把注入块追加进 payload 的 system 消息（有则改、无则插）。
    fn inject_payload(&self, payload: &mut serde_json::Value) {
        let block = self.inject_block();
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
}

/// 挂进 loop：构造模型请求前注入记忆（核心挂点 on_request 的第一个真实使用者）。
impl LoopHooks for MemoryFilePlugin {
    fn on_request(&mut self, _ctx: &RequestCtx, payload: &mut serde_json::Value) {
        self.inject_payload(payload);
    }
}

/// 服务面适配（SERVICE_FACES 图纸 #3）：newtype 打破孤儿规则——
/// `MemoryPortAdapter` 内部持共享实例，实现 `MemoryPort`。
/// 组装层注册 "memory" 服务，插件/宿主经
/// `ctx.port::<dyn MemoryPort>("memory")` 取用（remember 入口）；
/// 请求注入仍走 LoopHooks 轨（双轨各自职责不变）。
pub struct MemoryPortAdapter(pub Arc<std::sync::Mutex<MemoryFilePlugin>>);

impl bm_protocol::MemoryPort for MemoryPortAdapter {
    fn remember(&self, fact: String) {
        // fail-safe（记忆是增强不是正确性依赖）：锁中毒跳过，不 panic
        if let Ok(mut m) = self.0.lock() {
            m.remember(fact);
        } else {
            tracing::warn!(event = "bm.memory_lock_failed", "记忆锁中毒，本次跳过");
        }
    }

    fn facts(&self) -> Vec<String> {
        match self.0.lock() {
            Ok(m) => m.facts().map(String::from).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn inject_into_payload(&self, payload: &mut serde_json::Value) {
        if let Ok(m) = self.0.lock() {
            m.inject_payload(payload);
        }
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
        let facts: Vec<&str> = m2.facts().collect();
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&"用户偏好：大屏显示器"));
        let _ = std::fs::remove_file(&path);
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
        m.inject_payload(&mut payload);
        let sys = payload["messages"][0]["content"].as_str().unwrap();
        assert!(sys.starts_with("你是助手"), "原文保留");
        assert!(sys.contains("[长期记忆]") && sys.contains("事实一"));
        let _ = std::fs::remove_file(&path);
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
        m.inject_payload(&mut payload);
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
        m.inject_payload(&mut payload);
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
        let block = m.inject_block();
        assert!(block.contains("一") && block.contains("二"));
        assert!(!block.contains("三"), "上限截断");
        let _ = std::fs::remove_file(&path);
    }
}
