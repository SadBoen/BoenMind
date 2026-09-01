# -*- coding: utf-8 -*-
"""openai_http.rs 工具透传补丁:tools 透传 + tool_calls 解析/聚合。"""
import io

p = 'crates/bm-providers/src/openai_http.rs'
s = io.open(p, encoding='utf-8', newline='').read()
LF = chr(10)

def rep(old, new, tag):
    global s
    if new in s:
        print('skip', tag)
        return
    s = s.replace(old, new, 1)

# 1) WireRequest
rep('''#[derive(serde::Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    messages: Vec<WireMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
}''', '''#[derive(serde::Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    messages: Vec<WireMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
    /// W4 对话工具闭环:直通工具(OpenAI function 格式)透传。
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
}''', 'WireRequest')

# 2) WireMsg tool_calls
rep('''#[derive(serde::Deserialize)]
struct WireMsg {
    content: Option<String>,
}''', '''#[derive(serde::Deserialize)]
struct WireMsg {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<WireToolCall>>,
}

#[derive(serde::Deserialize)]
struct WireToolCall {
    #[serde(default)]
    id: Option<String>,
    function: Option<WireToolFn>,
}

#[derive(serde::Deserialize)]
struct WireToolFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}''', 'WireMsg')

# 3) 流式 delta 分片
rep('''#[derive(serde::Deserialize)]
struct WireStreamDelta {
    content: Option<String>,
}''', '''#[derive(serde::Deserialize)]
struct WireStreamDelta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<WireStreamToolCall>>,
}

#[derive(serde::Deserialize)]
struct WireStreamToolCall {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    function: Option<WireStreamToolFn>,
}

#[derive(serde::Deserialize)]
struct WireStreamToolFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}''', 'stream delta')

# 4) completed_stream 签名+finish 收敛
rep('''fn completed_stream(
    content: String,
    finish_raw: &str,
    usage: Option<WireUsage>,
    interrupted: bool,
    model: &str,
) -> InvokeResponse {
    // finish_reason 三值以上按合同二值收敛(与非流式同口径)。
    let finish_reason = if finish_raw == "length" {
        FinishReason::Length
    } else {
        FinishReason::Stop
    };
    InvokeResponse::Completed {
        content,
        finish_reason,''', '''fn completed_stream(
    content: String,
    finish_raw: &str,
    usage: Option<WireUsage>,
    interrupted: bool,
    model: &str,
    tool_calls: Vec<ToolCallPayload>,
) -> InvokeResponse {
    // finish_reason 三值收敛;tool_calls 随 Completed 携带(W4)。
    let finish_reason = match finish_raw {
        "length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        _ => FinishReason::Stop,
    };
    InvokeResponse::Completed {
        content,
        tool_calls,
        finish_reason,''', 'completed_stream sig')

# 5) invoke 非流式 body
rep('''        let body = WireRequest {
            model: &model,
            messages: req
                .messages
                .iter()
                .map(|m| WireMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                    },
                    content: &m.content,
                })
                .collect(),
            temperature: req.params.temperature,
            max_tokens: req.params.max_tokens,
            stream: false,
        };''', '''        let has_tools = !req.tools.is_empty();
        let body = WireRequest {
            model: &model,
            messages: req
                .messages
                .iter()
                .map(|m| WireMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "user",
                    },
                    content: &m.content,
                })
                .collect(),
            temperature: req.params.temperature,
            max_tokens: req.params.max_tokens,
            stream: false,
            tools: if has_tools {
                Some(serde_json::Value::Array(req.tools.clone()))
            } else {
                None
            },
            tool_choice: if has_tools { Some("auto") } else { None },
        };''', 'invoke body')

# 6) invoke 非流式 finish/tool_calls/Completed
rep('''        // finish_reason 三值以上(如 tool_calls)按合同二值收敛:M7 非流式、
        // tools 恒空,内容照常返回(合同枚举不破,流式留 M8)。
        let finish = wire
            .choices
            .first()
            .and_then(|c| c.finish_reason.as_deref())
            .unwrap_or("stop");
        let finish_reason = match finish {
            "length" => FinishReason::Length,
            _ => FinishReason::Stop,
        };
        let content = wire
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();''', '''        // finish_reason 三值收敛;tool_calls 响应回喂对话循环(W4)。
        let finish = wire
            .choices
            .first()
            .and_then(|c| c.finish_reason.as_deref())
            .unwrap_or("stop");
        let finish_reason = match finish {
            "length" => FinishReason::Length,
            "tool_calls" => FinishReason::ToolCalls,
            _ => FinishReason::Stop,
        };
        let tool_calls: Vec<ToolCallPayload> = wire
            .choices
            .first()
            .and_then(|c| c.message.tool_calls.as_ref())
            .map(|tcs| {
                tcs.iter()
                    .enumerate()
                    .map(|(i, tc)| ToolCallPayload {
                        id: tc.id.clone().unwrap_or_else(|| format!("call_{}", i)),
                        name: tc
                            .function
                            .as_ref()
                            .and_then(|f| f.name.clone())
                            .unwrap_or_default(),
                        arguments: tc
                            .function
                            .as_ref()
                            .and_then(|f| f.arguments.clone())
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let content = wire
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();''', 'invoke finish')

rep('''        InvokeResponse::Completed {
            content,
            finish_reason,
            usage: usage.unwrap_or(Usage {
                tokens_in: 0,
                tokens_out: 0,
            }),
            model_id: model,
            // latency 由调用方(turn 循环)按真实钟测量;此处给 0 占位,
            // 与 MockConnector 的「声明值」口径一致(基线 9.7)。
            latency_ms: 0,
            stream_interrupted: false,
        }
    }''', '''        InvokeResponse::Completed {
            content,
            tool_calls,
            finish_reason,
            usage: usage.unwrap_or(Usage {
                tokens_in: 0,
                tokens_out: 0,
            }),
            model_id: model,
            // latency 由调用方(turn 循环)按真实钟测量;此处给 0 占位,
            // 与 MockConnector 的「声明值」口径一致(基线 9.7)。
            latency_ms: 0,
            stream_interrupted: false,
        }
    }''', 'invoke Completed')

# 7) stream body
rep('''        let body = WireRequest {
            model: &model,
            messages: req
                .messages
                .iter()
                .map(|m| WireMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                    },
                    content: &m.content,
                })
                .collect(),
            temperature: req.params.temperature,
            max_tokens: req.params.max_tokens,
            stream: true,
        };''', '''        let has_tools = !req.tools.is_empty();
        let body = WireRequest {
            model: &model,
            messages: req
                .messages
                .iter()
                .map(|m| WireMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "user",
                    },
                    content: &m.content,
                })
                .collect(),
            temperature: req.params.temperature,
            max_tokens: req.params.max_tokens,
            stream: true,
            tools: if has_tools {
                Some(serde_json::Value::Array(req.tools.clone()))
            } else {
                None
            },
            tool_choice: if has_tools { Some("auto") } else { None },
        };''', 'stream body')

# 8) stream 聚合状态
rep('''        let mut buf: Vec<u8> = Vec::new();
        let mut content = String::new();
        let mut finish = "stop".to_string();
        let mut usage: Option<WireUsage> = None;''', '''        let mut buf: Vec<u8> = Vec::new();
        let mut content = String::new();
        let mut finish = "stop".to_string();
        let mut usage: Option<WireUsage> = None;
        // W4:流式 tool_calls 分片聚合(按 index 拼 id/name/arguments)。
        let mut tc_parts: std::collections::BTreeMap<usize, (String, String, String)> =
            std::collections::BTreeMap::new();''', 'stream state')

# 9) cancel 分支
rep('''                _ = cancel.cancelled() => {
                    if content.is_empty() {
                        return failed(ErrorCode::Cancelled, false, attempt);
                    }
                    return completed_stream(content, &finish, usage.take(), true, &model);
                }''', '''                _ = cancel.cancelled() => {
                    if content.is_empty() && tc_parts.is_empty() {
                        return failed(ErrorCode::Cancelled, false, attempt);
                    }
                    let tcs = tc_parts
                        .values()
                        .map(|(id, _n, ar)| ToolCallPayload {
                            id: id.clone(),
                            name: String::new(),
                            arguments: ar.clone(),
                        })
                        .collect();
                    return completed_stream(content, &finish, usage.take(), true, &model, tcs);
                }''', 'cancel arm')

# 10) 传输故障分支
rep('''                    Err(e) => {
                        // 中途传输故障:已收内容可用即用(如实标记中断)。
                        if content.is_empty() {
                            return transport_failed(&e, attempt);
                        }
                        return completed_stream(content, &finish, usage.take(), true, &model);
                    }''', '''                    Err(e) => {
                        // 中途传输故障:已收内容可用即用(如实标记中断)。
                        if content.is_empty() && tc_parts.is_empty() {
                            return transport_failed(&e, attempt);
                        }
                        let tcs = tc_parts
                            .values()
                            .map(|(id, _n, ar)| ToolCallPayload {
                                id: id.clone(),
                                name: String::new(),
                                arguments: ar.clone(),
                            })
                            .collect();
                        return completed_stream(content, &finish, usage.take(), true, &model, tcs);
                    }''', 'transport arm')

# 11) DONE 分支
rep('''                if data == "[DONE]" {
                    return completed_stream(content, &finish, usage.take(), false, &model);
                }''', '''                if data == "[DONE]" {
                    let tcs: Vec<ToolCallPayload> = tc_parts
                        .values()
                        .map(|(id, _n, ar)| ToolCallPayload {
                            id: id.clone(),
                            name: n.clone(),
                            arguments: ar.clone(),
                        })
                        .collect();
                    return completed_stream(content, &finish, usage.take(), false, &model, tcs);
                }''', 'done arm')

# 12) delta 内容分发
rep('''                    if let Some(d) = &c.delta
                        && let Some(t) = &d.content
                        && !t.is_empty()
                    {
                        content.push_str(t);
                        (on_delta)(t);
                    }''', '''                    if let Some(d) = &c.delta {
                        if let Some(t) = &d.content
                            && !t.is_empty()
                        {
                            content.push_str(t);
                            (on_delta)(t);
                        }
                        if let Some(tcs) = &d.tool_calls {
                            for tc in tcs {
                                let idx = tc.index.unwrap_or(0);
                                let slot = tc_parts.entry(idx).or_insert_with(|| {
                                    (
                                        tc.id.clone().unwrap_or_default(),
                                        String::new(),
                                        String::new(),
                                    )
                                });
                                if let Some(id) = &tc.id {
                                    slot.0 = id.clone();
                                }
                                if let Some(f) = &tc.function {
                                    if let Some(nm) = &f.name {
                                        slot.1.push_str(nm);
                                    }
                                    if let Some(ar) = &f.arguments {
                                        slot.2.push_str(ar);
                                    }
                                }
                            }
                        }
                    }''', 'delta dispatch')

# 13) 自然结束
rep('''        completed_stream(content, &finish, usage.take(), false, &model)
    }
}''', '''        let tcs: Vec<ToolCallPayload> = tc_parts
            .values()
            .map(|(id, _n, ar)| ToolCallPayload {
                id: id.clone(),
                name: String::new(),
                arguments: ar.clone(),
            })
            .collect();
        completed_stream(content, &finish, usage.take(), false, &model, tcs)
    }
}''', 'natural end')

io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('openai_http patched ok')
