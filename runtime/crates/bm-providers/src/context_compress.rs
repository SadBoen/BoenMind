//! issue #2:上下文压缩独立工具(context.compress)。
//!
//! 默认裁决(保守可回退):压缩 = **派生摘要工件**,不触碰 context-log.jsonl
//! (对话正文唯一落盘,A4;单写者语义零变更)。工具读会话的
//! user_message/assistant_final 事件流,产出人类可读摘要,落
//! `<data>/compress/{session_id}.json`;回合组装面(spawn)发现该文件存在即
//! 以 System 消息注入前缀——删除文件即回退,历史原文永不改写。
//!
//! 权限零新通道:经 Broker 七步管线(查表/审批/审计),manifest =
//! read-only effect(产物为内核数据目录内的派生工件,可随时重生成/
//! 删除;非用户域文件)+ not-required 审批 + 幂等。摘要为确定性抽取
//! (目标/脉络/近 N 轮原文),不嵌套模型调用。

use bm_contract::capability::CapabilityManifest;
use bm_core::registry::CapabilityProvider;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

pub const CAPABILITY: &str = "context.compress";

/// 单段文本截断(字符级;防超长历史拖爆摘要)。
fn truncate(s: &str, cap_chars: usize) -> String {
    if s.chars().count() <= cap_chars {
        return s.to_string();
    }
    let cut: String = s.chars().take(cap_chars).collect();
    format!("{cut}…(截断)")
}

pub struct ContextCompressProvider {
    data_dir: PathBuf,
}

impl ContextCompressProvider {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    fn compress(&self, args: &Value) -> Result<Value, String> {
        let sid = args["session_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("session_id 必须是非空字符串")?;
        let keep_recent = args["keep_recent"].as_u64().unwrap_or(10).clamp(1, 200) as usize;

        let log_path = self.data_dir.join("context-log.jsonl");
        let text =
            std::fs::read_to_string(&log_path).map_err(|e| format!("context-log 读取失败: {e}"))?;

        // 顺序扫描:按 user_message/assistant_final 事件还原轮次序列
        let mut user_inputs: Vec<String> = Vec::new();
        let mut pairs: Vec<(String, String)> = Vec::new();
        let mut pending_user: Option<String> = None;
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if v["session_id"].as_str() != Some(sid) {
                continue;
            }
            let Some(kind) = v["kind"].as_str() else {
                continue;
            };
            match kind {
                "user_message" => {
                    let c = v["data"]["content"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    if let Some(u) = pending_user.take() {
                        pairs.push((u, String::new()));
                    }
                    user_inputs.push(c.clone());
                    pending_user = Some(c);
                }
                "assistant_final" => {
                    let c = v["data"]["content"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    if let Some(u) = pending_user.take() {
                        pairs.push((u, c));
                    }
                }
                _ => {}
            }
        }
        if let Some(u) = pending_user.take() {
            pairs.push((u, String::new()));
        }
        if pairs.is_empty() {
            return Err(format!(
                "会话 {sid} 无可压缩历史(context-log 中未找到 user_message/assistant_final)"
            ));
        }

        let turns_total = pairs.len();
        let keep = keep_recent.min(turns_total);
        let mut summary = String::new();
        summary.push_str(&format!(
            "会话共 {turns_total} 轮;本摘要为确定性抽取(非模型改写),最近 {keep} 轮附原文。\n"
        ));
        if let Some(first) = user_inputs.first() {
            summary.push_str(&format!(
                "\n[会话目标/首条输入]\n{}\n",
                truncate(first, 500)
            ));
        }
        if user_inputs.len() > 1 {
            summary.push_str("\n[用户输入脉络]\n");
            for (i, u) in user_inputs.iter().take(20).enumerate() {
                summary.push_str(&format!("{}. {}\n", i + 1, truncate(u, 200)));
            }
            if user_inputs.len() > 20 {
                summary.push_str(&format!("(其余 {} 条略)\n", user_inputs.len() - 20));
            }
        }
        summary.push_str(&format!("\n[最近 {keep} 轮原文]\n"));
        for (i, (u, a)) in pairs[turns_total - keep..].iter().enumerate() {
            summary.push_str(&format!(
                "\n— 轮 {} —\n用户: {}\n助手: {}\n",
                turns_total - keep + i + 1,
                truncate(u, 400),
                truncate(a, 800)
            ));
        }

        let dir = self.data_dir.join("compress");
        std::fs::create_dir_all(&dir).map_err(|e| format!("compress 目录创建失败: {e}"))?;
        let file = bm_core::context_log::compress_summary_path(&self.data_dir, sid);
        let payload = json!({
            "session_id": sid,
            "turns_total": turns_total,
            "turns_summarized": turns_total - keep,
            "keep_recent": keep,
            "summary": summary,
        });
        let text = crate::context_compress::pretty(&payload)?;
        bm_persist::atomic_write(&file, text.as_bytes())
            .map_err(|e| format!("摘要文件写入失败: {e}"))?;

        Ok(json!({
            "ok": true,
            "session_id": sid,
            "turns_total": turns_total,
            "turns_summarized": turns_total - keep,
            "summary_file": file.display().to_string(),
            "summary": summary,
        }))
    }
}

fn pretty(v: &Value) -> Result<String, String> {
    let mut s = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    s.push('\n');
    Ok(s)
}

impl CapabilityProvider for ContextCompressProvider {
    fn invoke(&self, args: Value) -> Result<Value, String> {
        self.compress(&args)
    }
}

fn manifest() -> CapabilityManifest {
    serde_json::from_value(json!({
        "capability": CAPABILITY,
        "provider": "context",
        "version": "0.1.0",
        "input_schema": {
            "type": "object",
            "properties": {
                "session_id": {"type": "string"},
                "keep_recent": {"type": "integer", "minimum": 1, "maximum": 200}
            },
            "required": ["session_id"]
        },
        "output_schema": {"type": "object"},
        "effect": "read-only",
        "idempotent": true,
        "cancellable": false,
        "timeout_ms": 5000,
        "approval": "not-required",
        "scopes": [],
        "execution_mode": "sync",
        "description": "上下文压缩:抽取会话历史生成人类可读摘要并落盘(不改写原文);此后每回合自动注入摘要前缀,删除摘要文件即回退"
    }))
    .expect("context.compress manifest 合法")
}

/// 装配入口(server 启动期注册进能力面)。
pub fn capability_entries(
    data_dir: PathBuf,
) -> Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)> {
    vec![(
        manifest(),
        Arc::new(ContextCompressProvider::new(data_dir)) as Arc<dyn CapabilityProvider>,
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn seed_log(dir: &Path) {
        let mut lines = Vec::new();
        let mut seq = 0u64;
        let mut ev = |sid: &str, kind: &str, content: &str| {
            seq += 1;
            json!({
                "seq": seq, "ts": "2026-09-09T00:00:00.000Z", "session_id": sid,
                "operation_id": "op_x", "turn_index": seq, "kind": kind,
                "data": {"content": content}
            })
            .to_string()
        };
        lines.push(ev("sessA", "user_message", "帮我整理项目文档"));
        lines.push(ev("sessA", "assistant_final", "好的,我先读目录结构。"));
        lines.push(ev("sessA", "user_message", "第二问:总结依赖清单"));
        lines.push(ev("sessA", "assistant_final", "依赖共 12 项。"));
        // 其他会话的行(应被过滤)
        lines.push(ev("sessB", "user_message", "别的不相关会话"));
        std::fs::write(dir.join("context-log.jsonl"), lines.join("\n")).expect("写");
    }

    #[test]
    fn compress_builds_summary_and_writes_artifact() {
        let dir = tempfile::tempdir().expect("tmp");
        seed_log(dir.path());
        let p = ContextCompressProvider::new(dir.path().to_path_buf());
        let out = p
            .invoke(json!({"session_id": "sessA", "keep_recent": 1}))
            .expect("压缩成功");
        assert_eq!(out["turns_total"], 2);
        assert_eq!(out["turns_summarized"], 1);
        let summary = out["summary"].as_str().unwrap();
        assert!(summary.contains("整理项目文档"), "含首条目标");
        assert!(summary.contains("依赖共 12 项"), "近轮原文");
        assert!(!summary.contains("别的不相关会话"), "他会话隔离");

        // 工件落盘,load_summary 可读回
        let roundtrip =
            bm_core::context_log::load_compress_summary(dir.path(), "sessA").expect("摘要文件存在");
        assert!(roundtrip.contains("会话共 2 轮"));
    }

    #[test]
    fn compress_rejects_unknown_session() {
        let dir = tempfile::tempdir().expect("tmp");
        seed_log(dir.path());
        let p = ContextCompressProvider::new(dir.path().to_path_buf());
        let err = p.invoke(json!({"session_id": "nope"})).unwrap_err();
        assert!(err.contains("无可压缩历史"));
        assert!(bm_core::context_log::load_compress_summary(dir.path(), "nope").is_none());
    }
}
