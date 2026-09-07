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
        let slot = turns
            .entry(session_id.clone())
            .or_default()
            .entry((op, turn_index))
            .or_default();
        match v.get("kind").and_then(|k| k.as_str()).unwrap_or("") {
            "user_message" => slot.0 = Some(content),
            "assistant_final" => {
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
        while entry.len() > limits.history_max_turns {
            entry.remove(0);
        }
        let mut total: usize = entry.iter().map(|(u, a)| u.len() + a.len()).sum();
        while total > limits.history_max_chars && entry.len() > 1 {
            total -= entry[0].0.len() + entry[0].1.len();
            entry.remove(0);
        }
        w.session_turn_totals
            .insert(session_id.clone(), totals.remove(&session_id).unwrap_or(0));
        w.session_chats.insert(session_id, entry);
    }
}
pub(crate) fn push_capped(
    entry: &mut Vec<(String, String)>,
    user: String,
    assistant: String,
    limits: &crate::limits::Limits,
) {
    entry.push((user, assistant));
    while entry.len() > limits.history_max_turns {
        entry.remove(0);
    }
    let mut total: usize = entry.iter().map(|(u, a)| u.len() + a.len()).sum();
    while total > limits.history_max_chars && entry.len() > 1 {
        total -= entry[0].0.len() + entry[0].1.len();
        entry.remove(0);
    }
}
#[allow(dead_code)] // 测试助手在 lib 构建下天然未用(同模块既有 allow 惯例)
mod w5_history_tests {
    #[allow(unused_imports)] // lib 构建下 cfg(test) 剥离产生假性未用
    use super::*;
    use crate::limits::Limits;

    fn push(entry: &mut Vec<(String, String)>, u: String, a: String) {
        push_capped(entry, u, a, &Limits::default());
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
}
