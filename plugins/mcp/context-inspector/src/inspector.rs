use crate::config::Config;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

pub fn est_tokens(s: &str) -> u64 {
    (s.chars().count().saturating_add(2) / 3) as u64
}

/// 读取 model.json 中的 contextWindows 登记表
pub fn read_context_windows(cfg: &Config) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    let p = cfg.model_config_path();
    if let Ok(content) = std::fs::read_to_string(&p) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            if let Some(obj) = v.get("contextWindows").and_then(Value::as_object) {
                for (k, val) in obj {
                    if let Some(n) = val.as_u64() {
                        map.insert(k.clone(), n);
                    }
                }
            }
        }
    }
    map
}

/// 读取 context-log.jsonl 中的所有行 (逐行容错解析)
pub fn read_all_records(cfg: &Config) -> Vec<Value> {
    let mut list = Vec::new();
    let p = cfg.context_log_path();
    if let Ok(file) = File::open(&p) {
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                    list.push(v);
                }
            }
        }
    }
    list
}

/// 工具 1: 快照深度拆解与透视
pub fn inspect_snapshot(cfg: &Config, session_id: Option<&str>, seq: Option<u64>) -> Value {
    let records = read_all_records(cfg);
    // 过滤出模型快照行 (无 kind 字段)
    let snapshots: Vec<&Value> = records
        .iter()
        .filter(|r| r.get("kind").is_none())
        .filter(|r| {
            if let Some(sid) = session_id {
                r.get("session_id").and_then(Value::as_str) == Some(sid)
            } else {
                true
            }
        })
        .collect();

    let target = match seq {
        Some(s) => snapshots
            .iter()
            .find(|r| r.get("seq").and_then(Value::as_u64) == Some(s))
            .copied(),
        None => snapshots.last().copied(),
    };

    let Some(snap) = target else {
        return json!({
            "found": false,
            "note": "未找到符合条件的大模型调用快照"
        });
    };

    let windows = read_context_windows(cfg);
    let model_id = snap.get("model_id").and_then(Value::as_str).unwrap_or("");
    let max_window = windows.get(model_id).copied();

    let tokens_in = snap.get("tokens_in").and_then(Value::as_u64).unwrap_or(0);
    let tokens_out = snap.get("tokens_out").and_then(Value::as_u64).unwrap_or(0);
    let total_tokens = tokens_in.saturating_add(tokens_out);

    let (remaining_headroom, headroom_pct) = if let Some(mw) = max_window {
        let rem = mw.saturating_sub(total_tokens);
        let pct = (total_tokens as f64 / mw as f64 * 100.0).min(100.0).round();
        (Some(rem), Some(pct))
    } else {
        (None, None)
    };

    let latency_ms = snap.get("latency_ms").and_then(Value::as_u64);
    let speed = if let Some(lat) = latency_ms {
        if lat > 0 && tokens_out > 0 {
            Some(format!(
                "{:.1}",
                (tokens_out as f64) / (lat as f64 / 1000.0)
            ))
        } else {
            None
        }
    } else {
        None
    };

    // 解析 messages 组成
    let empty_vec = Vec::new();
    let messages = snap
        .get("messages")
        .and_then(Value::as_array)
        .unwrap_or(&empty_vec);
    let mut persona_text = String::new();
    let mut skills = Vec::new();
    let mut workspace_text = None;
    let mut history_turns = Vec::new();
    let mut current_input = String::new();
    let mut reasoning_snippet = None;

    for m in messages {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("");
        let content = m.get("content").and_then(Value::as_str).unwrap_or("");

        if role == "system" {
            let mut raw = content.to_string();
            if let Some(idx) = raw.find("[工作目录]") {
                workspace_text = Some(raw[idx..].trim().to_string());
                raw = raw[..idx].trim().to_string();
            }

            if let Some(first_skill) = raw.find("[附加技能 · ") {
                persona_text = raw[..first_skill].trim().to_string();
                let skills_section = &raw[first_skill..];
                // 按 "[附加技能 · " 切分
                for part in skills_section.split("[附加技能 · ") {
                    let trimmed = part.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Some(close_bracket) = trimmed.find(']') {
                        let name = trimmed[..close_bracket].trim();
                        let instruction = trimmed[close_bracket + 1..].trim();
                        skills.push(json!({
                            "name": name,
                            "instruction": instruction,
                            "estimated_tokens": est_tokens(instruction)
                        }));
                    }
                }
            } else {
                persona_text = raw;
            }
        } else if (role == "user" || role == "assistant")
            && content.contains("<think>")
            && content.contains("</think>")
        {
            if let (Some(s), Some(e)) = (content.find("<think>"), content.find("</think>")) {
                reasoning_snippet = Some(content[s + 7..e].trim().to_string());
            }
        }
    }

    let non_sys: Vec<&Value> = messages
        .iter()
        .filter(|m| {
            let r = m.get("role").and_then(Value::as_str).unwrap_or("");
            r == "user" || r == "assistant"
        })
        .collect();

    if let Some(last) = non_sys.last() {
        if last.get("role").and_then(Value::as_str) == Some("user") {
            current_input = last
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let prev = &non_sys[..non_sys.len() - 1];
            let mut t_idx = 1;
            let mut i = 0;
            while i < prev.len() {
                let u = if prev[i].get("role").and_then(Value::as_str) == Some("user") {
                    prev[i].get("content").and_then(Value::as_str).unwrap_or("")
                } else {
                    ""
                };
                let a = if i + 1 < prev.len()
                    && prev[i + 1].get("role").and_then(Value::as_str) == Some("assistant")
                {
                    prev[i + 1]
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                } else {
                    ""
                };
                if !u.is_empty() || !a.is_empty() {
                    history_turns.push(json!({
                        "turn_index": t_idx,
                        "user": u,
                        "assistant": a,
                        "tokens": est_tokens(u).saturating_add(est_tokens(a))
                    }));
                    t_idx += 1;
                }
                i += 2;
            }
        }
    }

    // 解析 tools
    let tools = snap
        .get("tools")
        .and_then(Value::as_array)
        .unwrap_or(&empty_vec);
    let tool_list: Vec<Value> = tools
        .iter()
        .map(|t| {
            let fn_obj = t.get("function").cloned().unwrap_or(json!({}));
            let name = fn_obj
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let desc = fn_obj
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let params = fn_obj.get("parameters").cloned().unwrap_or(json!({}));
            let param_tokens = est_tokens(&params.to_string());
            json!({
                "name": name,
                "description": desc,
                "needs_approval": desc.contains("需要用户审批"),
                "param_tokens": param_tokens
            })
        })
        .collect();

    json!({
        "found": true,
        "seq": snap.get("seq"),
        "ts": snap.get("ts"),
        "session_id": snap.get("session_id"),
        "turn_index": snap.get("turn_index"),
        "step": snap.get("step"),
        "model_id": model_id,
        "status": snap.get("status"),
        "streaming": snap.get("streaming"),
        "metrics": {
            "tokens_in": tokens_in,
            "tokens_out": tokens_out,
            "tokens_reasoning": snap.get("tokens_reasoning"),
            "tokens_cached": snap.get("tokens_cached"),
            "ttft_ms": snap.get("ttft_ms"),
            "latency_ms": latency_ms,
            "speed_tokens_per_sec": speed,
            "max_window_registered": max_window,
            "remaining_headroom": remaining_headroom,
            "headroom_pct": headroom_pct,
            "evicted_turns": snap.get("evicted_turns").unwrap_or(&json!(0))
        },
        "recipe": {
            "persona": {
                "text": persona_text,
                "estimated_tokens": est_tokens(&persona_text)
            },
            "skills": skills,
            "workspace": workspace_text,
            "tools": tool_list,
            "history_turns": history_turns,
            "current_input": {
                "text": current_input,
                "estimated_tokens": est_tokens(&current_input)
            },
            "reasoning_snippet": reasoning_snippet
        }
    })
}

/// 工具 2: 多轮 Token 暴增与异常激增诊断
pub fn diagnose_spikes(
    cfg: &Config,
    session_id: &str,
    threshold_diff: Option<u64>,
    threshold_ratio: Option<f64>,
) -> Value {
    let diff_limit = threshold_diff.unwrap_or(2500);
    let ratio_limit = threshold_ratio.unwrap_or(2.0);

    let records = read_all_records(cfg);
    let snapshots: Vec<&Value> = records
        .iter()
        .filter(|r| r.get("kind").is_none())
        .filter(|r| r.get("session_id").and_then(Value::as_str) == Some(session_id))
        .collect();

    let mut timeline = Vec::new();
    let mut spikes_count = 0;

    for (idx, snap) in snapshots.iter().enumerate() {
        let cur_in = snap.get("tokens_in").and_then(Value::as_u64).unwrap_or(0);
        let cur_out = snap.get("tokens_out").and_then(Value::as_u64).unwrap_or(0);
        let prev_in = if idx > 0 {
            snapshots[idx - 1]
                .get("tokens_in")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        } else {
            0
        };

        let diff = if idx > 0 {
            cur_in.saturating_sub(prev_in)
        } else {
            0
        };
        let is_spike = idx > 0
            && (diff >= diff_limit
                || (prev_in > 0 && (cur_in as f64 / prev_in as f64) >= ratio_limit));
        if is_spike {
            spikes_count += 1;
        }

        timeline.push(json!({
            "seq": snap.get("seq"),
            "turn_index": snap.get("turn_index"),
            "step": snap.get("step"),
            "tokens_in": cur_in,
            "tokens_out": cur_out,
            "diff_from_prev": diff,
            "is_spike": is_spike,
            "note": if is_spike { "检测到 Token 异常激增，可能调用了携带大量长文本的外部工具或读入了超大文件" } else { "正常增长" }
        }));
    }

    json!({
        "session_id": session_id,
        "total_snapshots": snapshots.len(),
        "spikes_found": spikes_count,
        "timeline": timeline
    })
}

/// 工具 3: 工程文件副作用追踪
pub fn track_file_effects(cfg: &Config, session_id: &str) -> Value {
    let records = read_all_records(cfg);
    let mut files_map: HashMap<String, Value> = HashMap::new();

    for r in &records {
        if r.get("session_id").and_then(Value::as_str) != Some(session_id) {
            continue;
        }
        if r.get("kind").and_then(Value::as_str) == Some("tool_call") {
            if let Some(data) = r.get("data").and_then(Value::as_object) {
                let tool = data.get("tool").and_then(Value::as_str).unwrap_or("");
                let args = data.get("arguments").cloned().unwrap_or(json!({}));

                let path = args
                    .get("path")
                    .or_else(|| args.get("file"))
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or_else(|| {
                        args.get("command")
                            .and_then(Value::as_str)
                            .and_then(|cmd| cmd.split_whitespace().nth(1))
                            .map(|s| s.to_string())
                    });

                if let Some(p) = path {
                    if p.contains('/') || p.contains('\\') || p.contains('.') {
                        let action = if tool.contains("write") {
                            "write"
                        } else if tool.contains("edit") {
                            "edit"
                        } else if tool.contains("exec") {
                            "exec"
                        } else {
                            "read"
                        };

                        files_map.insert(
                            p.clone(),
                            json!({
                                "path": p,
                                "action": action,
                                "tool_name": tool,
                                "last_seen_seq": r.get("seq"),
                                "turn_index": r.get("turn_index"),
                                "detail": args
                            }),
                        );
                    }
                }
            }
        }
    }

    let mut list: Vec<Value> = files_map.into_values().collect();
    list.sort_by_key(|v| v.get("last_seen_seq").and_then(Value::as_u64).unwrap_or(0));

    json!({
        "session_id": session_id,
        "affected_files_count": list.len(),
        "files": list
    })
}

/// 工具 4: 跨会话历史上下文检索
pub fn search_history(cfg: &Config, query: &str, limit: Option<usize>) -> Value {
    let lim = limit.unwrap_or(20).clamp(1, 100);
    let records = read_all_records(cfg);
    let needle = query.to_lowercase();

    let mut hits = Vec::new();
    for r in records.iter().rev() {
        let serialized = r.to_string().to_lowercase();
        if serialized.contains(&needle) {
            hits.push(r.clone());
            if hits.len() >= lim {
                break;
            }
        }
    }

    json!({
        "query": query,
        "total_hits": hits.len(),
        "hits": hits
    })
}
