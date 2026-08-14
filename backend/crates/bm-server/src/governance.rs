//! governance 雏形（HANDOFF_KERNEL_PHASE1.md §九 第 2 条）：
//! `governance.memorize` 的规则型占位实现——用户消息命中「记住」指令时，
//! 提取事实写入记忆插件（bm-memory facts.md 传送带）。
//!
//! **原型不是终态**：本文件只是把「记忆写入口」从无到有接通的雏形规则，
//! 最终由 Steward 的 governance.memorize 取代——核心零改动、规则在治理层
//! （对齐 bm-memory lib.rs 头注：策略在治理层，核心挂点不动）。雏形覆盖
//! 不到的场景（负向指令「别记住」、一条消息多事实、事实补全/纠错、LLM
//! 摘要提炼、遗忘指令等）都留到 Steward 轮在治理层实现，本文件届时整体
//! 替换，不动核心。
//!
//! 触发词与截断策略（雏形规则）：
//! - 触发词：「记住」或「remember that」（英文不分大小写）。**英文触发词
//!   必须是 "remember that"**：裸 "remember" 是英语常用词（"Do you
//!   remember..."），任意位置命中会把问句后半截误当事实写入（真实踩坑：
//!   记忆冒烟时 "Do you remember any facts?" 被记成事实）；"remember that"
//!   是明确的陈述句引导。只取消息中**第一处**触发词，其后的文本即事实候选；
//! - 清洗：去掉事实候选前导的空白与分隔符（中英冒号/逗号/句号/感叹号/
//!   问号/分号/引号/连字符等）；
//! - 截断：事实长度上限 [`MAX_FACT_CHARS`] = 200 字符（按 Unicode 字符计，
//!   中文按 1 计），超出截断取前 200 字符——记忆注入是有界字符原则
//!   （对齐 MemoryFilePlugin 的 max_facts 注入上限），雏形不做语义分段；
//! - 结果为空（无触发词 / 触发词后无内容）→ 不写入。

use std::sync::{Arc, Mutex};

use bm_memory::MemoryFilePlugin;

/// 事实长度上限：200 字符（Unicode 字符计，中文按 1 计）。
/// 约合 facts.md 一条中长事实；超出硬截断取前 200 字符（见文件头注释，
/// 雏形不做语义分段，最终由 Steward 决定提炼/分段策略）。
pub const MAX_FACT_CHARS: usize = 200;

/// 从用户消息提取「记住」指令的事实（纯函数，供单测与未来治理层复用）。
///
/// 规则见文件头注释。触发词按字节位置从左到右扫描，最早命中即停
/// （只取第一处关键词）；英文触发词大小写不敏感。
pub fn extract_remember_fact(message: &str) -> Option<String> {
    // 不 lowercase 全串：Unicode 大小写折叠会改变字节长度，索引会错位；
    // 改为逐字符边界比较前缀（触发词为固定字节串，前缀字节相等即命中）。
    let mut hit: Option<(usize, usize)> = None;
    'scan: for (i, _) in message.char_indices() {
        for kw in ["记住", "remember that"] {
            let tail = &message[i..];
            if tail.len() >= kw.len()
                && tail.as_bytes()[..kw.len()].eq_ignore_ascii_case(kw.as_bytes())
            {
                hit = Some((i, kw.len()));
                break 'scan;
            }
        }
    }
    let (pos, kw_len) = hit?;
    let rest = &message[pos + kw_len..];
    // 去掉前导分隔符：空白/中英冒号/逗号/句号/感叹号/问号/分号/引号/连字符
    let fact = rest.trim_start_matches(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                ':' | '：'
                    | ','
                    | '，'
                    | '.'
                    | '。'
                    | '!'
                    | '！'
                    | '?'
                    | '？'
                    | ';'
                    | '；'
                    | '"'
                    | '\''
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '-'
                    | '—'
                    | '*'
            )
    });
    // 截断：上限 MAX_FACT_CHARS，超出取前 200 字符
    let fact = fact.chars().take(MAX_FACT_CHARS).collect::<String>();
    let fact = fact.trim().to_string();
    if fact.is_empty() { None } else { Some(fact) }
}

/// 雏形 memorize：消息命中「记住」指令 → 锁记忆句柄 → [`MemoryFilePlugin::remember`]。
///
/// 返回记住的事实字符数（未命中/锁失败返回 None）。事实全文不进日志——
/// 只记字符数（用户内容不打日志纪律）。
pub fn memorize(memory: &Arc<Mutex<MemoryFilePlugin>>, message: &str) -> Option<usize> {
    let fact = extract_remember_fact(message)?;
    let chars = fact.chars().count();
    let Ok(mut m) = memory.lock() else {
        // 锁中毒（持有者 panic 过）：记忆是增强不是正确性依赖（fail-safe
        // 对齐压缩），只告警不阻断主链路
        tracing::warn!(event = "bm.memory_lock_failed", chars = chars);
        return None;
    };
    m.remember(fact);
    tracing::info!(event = "bm.memory_remembered", chars = chars);
    Some(chars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bm_engine::StreamHooks;
    use bm_loop::points::{LoopHooks, RequestCtx};

    /// 唯一临时目录（进程 id + uuid）：测试并行跑互不踩文件。
    fn unique_temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bm-server-governance-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn req_ctx() -> RequestCtx {
        RequestCtx {
            turn: 1,
            step: 1,
            prompt_hash: None,
        }
    }

    #[test]
    fn extracts_after_chinese_trigger() {
        assert_eq!(
            extract_remember_fact("记住：用户喜欢深色模式"),
            Some("用户喜欢深色模式".to_string()),
            "「记住：」之后即事实"
        );
        assert_eq!(
            extract_remember_fact("请记住 项目代号是 BoenMind"),
            Some("项目代号是 BoenMind".to_string()),
            "「记住」在句中也能命中"
        );
    }

    #[test]
    fn extracts_after_english_trigger_case_insensitive() {
        assert_eq!(
            extract_remember_fact("remember that the user prefers tea"),
            Some("the user prefers tea".to_string())
        );
        assert_eq!(
            extract_remember_fact("Remember THAT: dark mode"),
            Some("dark mode".to_string()),
            "英文大小写不敏感（Remember THAT）"
        );
        assert_eq!(
            extract_remember_fact("REMEMBER THAT, use tabs"),
            Some("use tabs".to_string())
        );
    }

    /// 回归：裸 "remember" 是英语常用词，问句（"Do you remember..."）不得误触发
    /// —— 英文触发词必须带 that（记忆冒烟真实踩坑）。
    #[test]
    fn bare_english_remember_does_not_trigger() {
        assert_eq!(
            extract_remember_fact("Do you remember any personal facts about the user?"),
            None,
            "问句中的裸 remember 不得把后半截当事实"
        );
        assert_eq!(
            extract_remember_fact("I remember you like coffee"),
            None,
            "陈述句中的裸 remember 也不触发"
        );
        assert_eq!(
            extract_remember_fact("Remember to buy milk"),
            None,
            "remember to（待办语义）不触发"
        );
    }

    #[test]
    fn no_trigger_returns_none() {
        assert_eq!(
            extract_remember_fact("今天天气怎么样"),
            None,
            "无触发词返回 None"
        );
        assert_eq!(extract_remember_fact(""), None, "空串返回 None");
    }

    #[test]
    fn trigger_without_content_returns_none() {
        assert_eq!(extract_remember_fact("记住"), None, "只有关键词无内容");
        assert_eq!(extract_remember_fact("记住："), None, "关键词后全是分隔符");
        assert_eq!(
            extract_remember_fact("please remember"),
            None,
            "英文触发词无内容"
        );
    }

    #[test]
    fn only_first_trigger_is_used() {
        let fact = extract_remember_fact("记住：第一段 remember：第二段").unwrap();
        assert!(fact.starts_with("第一段"), "只取第一处关键词，得到 {fact}");
    }

    #[test]
    fn trims_leading_punctuation_and_quotes() {
        assert_eq!(
            extract_remember_fact("记住：，\t“直接说重点"),
            Some("直接说重点".to_string()),
            "前导冒号/逗号/空白/引号都去掉"
        );
        assert_eq!(
            extract_remember_fact("remember that, ,apple"),
            Some("apple".to_string())
        );
    }

    #[test]
    fn overlong_fact_is_truncated_to_limit() {
        let long = "长".repeat(300);
        let fact = extract_remember_fact(&format!("记住：{long}")).unwrap();
        assert_eq!(fact.chars().count(), MAX_FACT_CHARS, "超长截断到上限");
        assert_eq!(fact, "长".repeat(MAX_FACT_CHARS), "截断保留前 200 字符");
    }

    #[test]
    fn memorize_writes_fact_and_reports_chars() {
        let dir = unique_temp_dir();
        let path = dir.join("facts.md");
        let memory = Arc::new(std::sync::Mutex::new(MemoryFilePlugin::open(
            path.clone(),
            20,
        )));

        let fact = "用户喜欢茶";
        let chars = memorize(&memory, &format!("记住：{fact}")).unwrap();
        assert_eq!(chars, fact.chars().count(), "返回事实字符数");
        {
            let m = memory.lock().unwrap();
            assert!(m.facts().any(|f| f == fact), "事实已入内存传送带");
        }
        // 持久化往返：重新 open（模拟新会话）读回
        let reopened = MemoryFilePlugin::open(path.clone(), 20);
        assert!(
            reopened.facts().any(|f| f == fact),
            "事实已落盘，跨会话可读回"
        );

        // 无触发词：不写入
        assert!(memorize(&memory, "今天天气如何").is_none(), "无触发词不写");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn injection_chain_appends_facts_block_to_system() {
        let dir = unique_temp_dir();
        let path = dir.join("facts.md");
        std::fs::write(&path, "事实A：用户喜欢深色模式\n").unwrap();

        let memory = Arc::new(std::sync::Mutex::new(MemoryFilePlugin::open(path, 20)));
        let mut hooks =
            StreamHooks::new(Arc::new(std::sync::Mutex::new(String::new())), Some(memory));

        let mut payload = serde_json::json!({
            "messages": [
                {"role": "system", "content": "你是助手"},
                {"role": "user", "content": "hi"},
            ]
        });
        hooks.on_request(&req_ctx(), &mut payload);

        let sys = payload["messages"][0]["content"].as_str().unwrap();
        assert!(sys.starts_with("你是助手"), "原 system 内容保留");
        assert!(sys.contains("[长期记忆]"), "注入块含 [长期记忆] 头");
        assert!(
            sys.contains("事实A：用户喜欢深色模式"),
            "system 含 facts.md 事实"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn injection_chain_skips_empty_facts() {
        let dir = unique_temp_dir();
        let path = dir.join("facts.md");
        std::fs::write(&path, "").unwrap();

        let memory = Arc::new(std::sync::Mutex::new(MemoryFilePlugin::open(path, 20)));
        let mut hooks =
            StreamHooks::new(Arc::new(std::sync::Mutex::new(String::new())), Some(memory));

        let mut payload = serde_json::json!({
            "messages": [
                {"role": "system", "content": "你是助手"},
                {"role": "user", "content": "hi"},
            ]
        });
        hooks.on_request(&req_ctx(), &mut payload);

        let sys = payload["messages"][0]["content"].as_str().unwrap();
        assert_eq!(sys, "你是助手", "空 facts.md 不应注入");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
