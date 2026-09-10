//! 自 turn.rs 机械移入(内容零改动)。
use super::*;

// W10(ADR-0024):台账双上限(默认 20 轮/24000 字符)走 limits 热生效,
// 生产路径读 w.config.limits;测试经 Limits::default() 取同值。

/// 成功回合终稿入账(工具轮中间态不入;只在 TurnEvent::Completed 路径调)。
/// 存活守卫:close 不取消在途回合(INV-6),迟到的落定不得把已清退的
/// 台账条目复活成孤儿——会话不存在或已 Closed 时丢弃。
pub(crate) fn remember_turn(w: &mut World, session_id: BmId, user: String, assistant: String) {
    let live = w
        .sessions
        .get(&session_id)
        .map(|s| s.state != SessionState::Closed)
        .unwrap_or(false);
    if !live {
        return;
    }
    *w.session_turn_totals.entry(session_id.clone()).or_insert(0) += 1;
    let limits = w.config.limits.get();
    push_capped(
        w.session_chats.entry(session_id).or_default(),
        user,
        assistant,
        &limits,
    );
}
/// 会话目录标题(2026-09-08 三端一致批):首条用户消息首行截断 40 字符
/// (字符安全);空消息回落占位。服务端为唯一权威口径。
pub(crate) fn session_title_from(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("").trim();
    let mut title: String = first_line.chars().take(40).collect();
    if first_line.chars().count() > 40 {
        title.push('…');
    }
    if title.is_empty() {
        title = "新对话".into();
    }
    title
}

pub(crate) fn rebuild_session_chats(w: &mut World) {
    let Some(data_dir) = w.config.data_dir.clone() else {
        return;
    };
    let Ok(f) = std::fs::File::open(data_dir.join("context-log.jsonl")) else {
        return;
    };
    use std::io::BufRead;
    // session → (operation_id, turn_index) → (user, assistant)
    type TurnSlot = (Option<String>, Option<String>);
    let mut turns: std::collections::HashMap<
        BmId,
        std::collections::HashMap<(String, u32), TurnSlot>,
    > = std::collections::HashMap::new();
    let mut totals: std::collections::HashMap<BmId, u64> = std::collections::HashMap::new();
    // 会话目录回填源(2026-09-08 三端一致批):(首条 user_message 截断标题,
    // 最后一条相关行 ts);与台账重建搭同一次扫描,不二次读盘。
    let mut meta: std::collections::HashMap<BmId, (Option<String>, Option<String>)> =
        std::collections::HashMap::new();
    for line in std::io::BufReader::new(f).lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue; // 坏行跳过
        };
        let Some(session_id) = v
            .get("session_id")
            .and_then(|s| s.as_str())
            .and_then(|s| BmId::parse(s).ok())
        else {
            continue;
        };
        let op = v
            .get("operation_id")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let turn_index = v.get("turn_index").and_then(|t| t.as_u64()).unwrap_or(0) as u32;
        let content = v["data"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if content.is_empty() {
            continue;
        }
        let ts = v.get("ts").and_then(|t| t.as_str()).map(str::to_string);
        let slot = turns
            .entry(session_id.clone())
            .or_default()
            .entry((op, turn_index))
            .or_default();
        match v.get("kind").and_then(|k| k.as_str()).unwrap_or("") {
            "user_message" => {
                let m = meta.entry(session_id.clone()).or_default();
                if m.0.is_none() {
                    m.0 = Some(session_title_from(&content));
                }
                if ts.is_some() {
                    m.1 = ts;
                }
                slot.0 = Some(content);
            }
            "assistant_final" => {
                let m = meta.entry(session_id.clone()).or_default();
                if ts.is_some() {
                    m.1 = ts;
                }
                slot.1 = Some(content);
                *totals.entry(session_id).or_insert(0) += 1;
            }
            _ => {}
        }
    }
    for (session_id, mut pairs) in turns {
        // live 守卫:会话不存在/已 Closed 不重建(与 remember_turn 同款)
        let live = w
            .sessions
            .get(&session_id)
            .map(|s| s.state != SessionState::Closed)
            .unwrap_or(false);
        if !live {
            continue;
        }
        let mut entry: Vec<(String, String)> = Vec::new();
        for (_k, (user, assistant)) in pairs.drain() {
            if let Some(a) = assistant {
                entry.push((
                    user.unwrap_or_else(|| "(早期记录未含该轮用户消息)".into()),
                    a,
                ));
            }
        }
        if entry.is_empty() {
            continue;
        }
        // 双上限与 push_capped 同口径(台账形状与运行期写入完全一致)
        let limits = w.config.limits.get();
        trim_capped(&mut entry, &limits);
        w.session_turn_totals
            .insert(session_id.clone(), totals.remove(&session_id).unwrap_or(0));
        w.session_chats.insert(session_id, entry);
    }
    // 会话目录回填(2026-09-08 三端一致批):存量行 title/updated_at 为 NULL
    // 时写平(内存 None ⇔ 行 NULL,dirty 即需落库);失败只告警——目录列非
    // 规范判定位,下次启动可重入补齐。已删/墓碑会话不在 sessions 映射,自然跳过。
    let store = w.store.clone();
    for (session_id, (title, last_ts)) in &meta {
        let Some(s) = w.sessions.get_mut(session_id) else {
            continue;
        };
        let mut dirty = false;
        if s.title.is_none() {
            s.title = title.clone();
            dirty = true;
        }
        if s.updated_at.is_none() {
            s.updated_at = last_ts.clone();
            dirty = true;
        }
        if dirty
            && let Some(store) = store.as_ref()
            && let Err(e) = store.backfill_session_meta(
                session_id.as_str(),
                s.title.as_deref(),
                s.updated_at.as_deref(),
            )
        {
            tracing::warn!(
                error = %e,
                session = %session_id.as_str(),
                "会话目录回填落库失败(下次启动重试)"
            );
        }
    }
}
/// 双上限裁剪(ADR-0028:上限 0 = 不限制,缺省默认即 0)。运行期写入与
/// 重启重建共用同一口径。
fn trim_capped(entry: &mut Vec<(String, String)>, limits: &crate::limits::Limits) {
    if limits.history_max_turns > 0 {
        while entry.len() > limits.history_max_turns {
            entry.remove(0);
        }
    }
    if limits.history_max_chars > 0 {
        let mut total: usize = entry.iter().map(|(u, a)| u.len() + a.len()).sum();
        while total > limits.history_max_chars && entry.len() > 1 {
            total -= entry[0].0.len() + entry[0].1.len();
            entry.remove(0);
        }
    }
}
pub(crate) fn push_capped(
    entry: &mut Vec<(String, String)>,
    user: String,
    assistant: String,
    limits: &crate::limits::Limits,
) {
    entry.push((user, assistant));
    trim_capped(entry, limits);
}
#[cfg(test)] // 门控剥除:测试模块不进生产 lib(同步全仓 mod tests 惯例)
mod w5_history_tests {
    #[allow(unused_imports)] // lib 构建下 cfg(test) 剥离产生假性未用
    use super::*;
    use crate::limits::Limits;

    fn push(entry: &mut Vec<(String, String)>, u: String, a: String) {
        // ADR-0028 起默认 0=不限;裁剪行为测试显式构造旧双上限(20 轮/24K)。
        let limits = Limits {
            history_max_turns: HISTORY_MAX_TURNS,
            history_max_chars: HISTORY_MAX_CHARS,
            ..Limits::default()
        };
        push_capped(entry, u, a, &limits);
    }
    const HISTORY_MAX_TURNS: usize = 20;
    const HISTORY_MAX_CHARS: usize = 24_000;

    #[test]
    fn turn_count_cap_drops_oldest() {
        let mut entry = Vec::new();
        for i in 0..(HISTORY_MAX_TURNS + 5) {
            push(&mut entry, format!("u{i}"), format!("a{i}"));
        }
        assert_eq!(entry.len(), HISTORY_MAX_TURNS);
        assert_eq!(entry[0], ("u5".to_string(), "a5".to_string()));
        assert_eq!(
            entry.last().unwrap().0,
            format!("u{}", HISTORY_MAX_TURNS + 4)
        );
    }

    #[test]
    fn char_cap_drops_oldest_but_keeps_latest() {
        let mut entry = Vec::new();
        let big = "x".repeat(10_000);
        for i in 0..4 {
            push(&mut entry, format!("{big}{i}"), big.clone());
        }
        // 每条 2 万字符,总量 8 万 > 24000:一路丢到只剩最新一条
        assert_eq!(entry.len(), 1);
        assert_eq!(entry[0].0, format!("{big}3"));
    }

    #[test]
    fn char_cap_never_drops_single_latest() {
        let mut entry = Vec::new();
        let big = "y".repeat(HISTORY_MAX_CHARS + 100);
        push(&mut entry, big.clone(), big);
        assert_eq!(entry.len(), 1, "最新一条不因字符上限被丢");
    }

    // ADR-0028:双上限 0 = 不限制(全量回喂),默认即 0。
    #[test]
    fn zero_limits_mean_unlimited() {
        let limits = Limits::default();
        assert_eq!(limits.history_max_turns, 0);
        assert_eq!(limits.history_max_chars, 0);
        let mut entry = Vec::new();
        for i in 0..30 {
            push_capped(&mut entry, format!("u{i}"), format!("a{i}"), &limits);
        }
        assert_eq!(entry.len(), 30, "0 上限不得裁掉任何轮次");
        assert_eq!(entry[0].0, "u0", "最早一轮必须仍在");
    }

    #[test]
    fn evicted_turns_arithmetic() {
        // 新会话前 3 轮全存活:无遗忘
        assert_eq!(evicted_turns(3, 3), 0);
        // 25 轮历史进 20 轮上限:遗忘最早 5 轮
        assert_eq!(evicted_turns(25, 20), 5);
        // 防御:计数滞后(复活/回放场景)不得借位下溢
        assert_eq!(evicted_turns(2, 5), 0);
        assert_eq!(evicted_turns(0, 0), 0);
    }

    // ---- 会话目录标题(2026-09-08 三端一致批)----

    #[test]
    fn session_title_takes_first_line_truncated() {
        assert_eq!(
            session_title_from("帮我总结这份文档\n第二行不该出现"),
            "帮我总结这份文档"
        );
        let long = "长".repeat(60);
        let t = session_title_from(&long);
        assert_eq!(t.chars().count(), 41, "40 字截断 + 省略号");
        assert!(t.ends_with('…'));
        assert_eq!(session_title_from("   "), "新对话", "空白消息回落占位");
    }
}
