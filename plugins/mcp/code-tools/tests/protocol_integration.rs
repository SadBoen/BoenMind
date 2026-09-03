//! 进程级集成测试:spawn 真实 exe,stdio 全双工走完 MCP 握手与工具闭环。

use serde_json::{json, Value};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// 返回配置临时目录占位(随 Session 同生命周期)——子进程异步读配置,
/// 若目录在函数返回时销毁,配置文件会在被读取前消失(已踩实)。
async fn spawn_with_roots(root: &std::path::Path) -> (Session, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("cfg tmp");
    let cfg_path = dir.path().join("cfg.json");
    std::fs::write(
        &cfg_path,
        json!({"allowed_roots": [root.display().to_string()]}).to_string(),
    )
    .expect("写配置");
    let mut child = Command::new(env!("CARGO_BIN_EXE_code-tools"))
        .arg("--config")
        .arg(&cfg_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn code-tools");
    let stdin = child.stdin.take().expect("stdin");
    let stdout = BufReader::new(child.stdout.take().expect("stdout"));
    (
        Session {
            child,
            stdin,
            stdout,
        },
        dir,
    )
}

async fn rpc(session: &mut Session, req: &Value) -> Value {
    session
        .stdin
        .write_all(format!("{}\n", req).as_bytes())
        .await
        .expect("写请求");
    session.stdin.flush().await.expect("flush");
    let mut line = String::new();
    let n = session.stdout.read_line(&mut line).await.expect("读应答");
    assert!(n > 0, "进程提前退出");
    serde_json::from_str(line.trim()).expect("应答 JSON")
}

fn structured(resp: &Value) -> Value {
    resp["result"]["structuredContent"].clone()
}

#[tokio::test]
async fn full_handshake_and_tool_loop() {
    let work = tempfile::tempdir().expect("work tmp");
    std::fs::write(
        work.path().join("fixture.txt"),
        "hello world\nsecond line\n",
    )
    .expect("fixture");

    let (mut s, _cfg_dir) = spawn_with_roots(work.path()).await;

    // 握手
    let init = rpc(
        &mut s,
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )
    .await;
    assert_eq!(init["result"]["serverInfo"]["name"], "code-tools");

    // 工具清单:4 个,审批注解正确
    let list = rpc(
        &mut s,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    )
    .await;
    let tools = list["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 4);
    assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
    assert_eq!(tools[3]["annotations"]["destructiveHint"], true);

    // write → read → edit → search 闭环
    let w = structured(
        &rpc(
            &mut s,
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
                    "params":{"name":"write","arguments":{"path":"nested/out.txt","content":"alpha beta\n"}}}),
        )
        .await,
    );
    assert_eq!(w["ok"], true, "{w}");

    let r = structured(
        &rpc(
            &mut s,
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
                    "params":{"name":"read","arguments":{"path":"fixture.txt"}}}),
        )
        .await,
    );
    assert_eq!(r["total_lines"], 2);
    assert!(r["content"].as_str().expect("c").contains("hello world"));

    let e = structured(
        &rpc(
            &mut s,
            &json!({"jsonrpc":"2.0","id":5,"method":"tools/call",
                    "params":{"name":"edit","arguments":{"path":"fixture.txt","old_string":"world","new_string":"code-tools"}}}),
        )
        .await,
    );
    assert_eq!(e["replacements"], 1, "{e}");

    let sc = structured(
        &rpc(
            &mut s,
            &json!({"jsonrpc":"2.0","id":6,"method":"tools/call",
                    "params":{"name":"search","arguments":{"query":"code-tools","fixed":true}}}),
        )
        .await,
    );
    assert_eq!(sc["ok"], true, "{sc}");
    assert_eq!(sc["total_matches"], 1);

    // 沙箱:白名单外路径被拒
    let escape = structured(
        &rpc(
            &mut s,
            &json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
                    "params":{"name":"read","arguments":{"path":"../../outside.txt"}}}),
        )
        .await,
    );
    assert_eq!(escape["ok"], false, "越界必须被拒:{escape}");

    let _ = s.child.kill().await;
}
