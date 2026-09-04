use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn test_self_describe_cli() {
    let bin_path = env!("CARGO_BIN_EXE_context-inspector");
    let output = Command::new(bin_path)
        .arg("--self-describe")
        .output()
        .expect("运行 context-inspector --self-describe");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let val: Value = serde_json::from_str(stdout.trim()).expect("解析 self-describe json");

    assert_eq!(val["name"], "context_inspector");
    assert!(val["title"].is_string());
    assert!(val["suggested_entry"]["args"].is_array());
}

#[test]
fn test_mcp_protocol_roundtrip() {
    let tmp = tempfile::tempdir().expect("创建临时数据目录");
    let tmp_path = tmp.path();

    // 播种一份假 context-log.jsonl
    let ctx_log = tmp_path.join("context-log.jsonl");
    let sample_rec = json!({
        "seq": 1,
        "ts": "2026-09-05T06:00:00Z",
        "session_id": "sess_test",
        "agent_id": "default",
        "operation_id": "op_test",
        "turn_index": 1,
        "step": 1,
        "attempt": 1,
        "model_id": "mimo-v2.5",
        "streaming": true,
        "messages": [
            {
                "role": "system",
                "content": "你是智能助理\n\n[附加技能 · 写作]\n善于写诗\n\n[工作目录] 本对话工作目录:D:/ws"
            },
            {
                "role": "user",
                "content": "帮我写首诗"
            }
        ],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "fs.read",
                    "description": "只读文件",
                    "parameters": {}
                }
            }
        ],
        "status": "ok",
        "tokens_in": 150,
        "tokens_out": 40,
        "tokens_reasoning": 20,
        "tokens_cached": 80,
        "ttft_ms": 320,
        "evicted_turns": 0,
        "latency_ms": 1200
    });
    std::fs::write(
        &ctx_log,
        format!("{}\n", serde_json::to_string(&sample_rec).unwrap()),
    )
    .unwrap();

    // 播种一份模型窗口配置 config/model.json
    let cfg_dir = tmp_path.join("config");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let model_json = cfg_dir.join("model.json");
    let sample_cfg = json!({
        "modelId": "mimo-v2.5",
        "contextWindows": {
            "mimo-v2.5": 128000
        }
    });
    std::fs::write(&model_json, serde_json::to_string(&sample_cfg).unwrap()).unwrap();

    let bin_path = env!("CARGO_BIN_EXE_context-inspector");
    let mut child = Command::new(bin_path)
        .arg("--data-dir")
        .arg(tmp_path.to_str().unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("启动 context-inspector 进程");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout_reader = BufReader::new(child.stdout.take().expect("stdout"));

    // 1. 测试 initialize 握手
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    writeln!(stdin, "{}", serde_json::to_string(&init_req).unwrap()).unwrap();
    let mut line = String::new();
    stdout_reader.read_line(&mut line).unwrap();
    let init_resp: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(init_resp["id"], 1);
    assert_eq!(
        init_resp["result"]["serverInfo"]["name"],
        "context_inspector"
    );

    // 2. 测试 tools/list
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    writeln!(stdin, "{}", serde_json::to_string(&list_req).unwrap()).unwrap();
    line.clear();
    stdout_reader.read_line(&mut line).unwrap();
    let list_resp: Value = serde_json::from_str(&line).unwrap();
    let tools = list_resp["result"]["tools"]
        .as_array()
        .expect("tools array");
    assert_eq!(tools.len(), 4);
    assert!(tools
        .iter()
        .any(|t| t["name"] == "context_inspect_snapshot"));
    assert!(tools.iter().any(|t| t["name"] == "context_diagnose_spikes"));
    assert!(tools
        .iter()
        .any(|t| t["name"] == "context_track_file_effects"));
    assert!(tools.iter().any(|t| t["name"] == "context_search_history"));

    // 3. 测试 tools/call: context_inspect_snapshot
    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "context_inspect_snapshot",
            "arguments": {
                "session_id": "sess_test"
            }
        }
    });
    writeln!(stdin, "{}", serde_json::to_string(&call_req).unwrap()).unwrap();
    line.clear();
    stdout_reader.read_line(&mut line).unwrap();
    let call_resp: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(call_resp["id"], 3);
    let structured = &call_resp["result"]["structuredContent"];
    assert_eq!(structured["found"], true);
    assert_eq!(structured["session_id"], "sess_test");
    assert_eq!(structured["metrics"]["max_window_registered"], 128000);
    assert_eq!(structured["metrics"]["remaining_headroom"], 127810);
    assert_eq!(structured["recipe"]["skills"][0]["name"], "写作");

    drop(stdin);
    let _ = child.wait();
}
