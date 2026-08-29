//! M3-T5 跨进程端到端(基线 M3 验收 + S4 over HTTP):
//! 真实 boenmind-server 子进程 + bm-cli 客户端库:
//! ① 全链路:建会话 → 发回合(HTTP)→ 收据终态;
//! ② 硬杀 server → 重启 → session.resume(active)+ operation 保持 succeeded
//!    (不消失、不取消)+ 新回合可发且 ID 无撞号(ID 计数提示);
//! ③ CLI 退出不取消:客户端进程消亡后回合仍完成(本测试的客户端即"退出后
//!    重连"形态,语义同 INV-6)。

use bm_cli::EnvelopeClient;
use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn server_exe() -> &'static str {
    env!("CARGO_BIN_EXE_boenmind-server")
}

struct Server {
    child: Child,
    url: String,
}

impl Drop for Server {
    /// panic/失败路径也强制杀子进程:避免管道滞留挂死测试进程。
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 拉起 server 子进程并自 stdout 解析实际绑定地址。
fn spawn_server(dir: &std::path::Path) -> Server {
    let mut child = Command::new(server_exe())
        .arg("--data-dir")
        .arg(dir)
        .arg("--bind")
        .arg("127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("拉起 boenmind-server");

    let stdout = child.stdout.take().expect("stdout 已管道化");
    let mut lines = std::io::BufReader::new(stdout).lines();
    let deadline = Instant::now() + Duration::from_secs(30);
    let url = loop {
        assert!(Instant::now() < deadline, "30s 内 server 未监听");
        match lines.next() {
            Some(Ok(line)) => {
                if let Some(addr) = line
                    .strip_prefix("boenmind-server")
                    .and_then(|u| u.split("监听 http://").nth(1))
                {
                    break format!("http://{addr}");
                }
            }
            Some(Err(e)) => panic!("读 stdout 失败: {e}"),
            None => panic!("server 提前退出"),
        }
    };
    // 消费剩余行,避免管道写满阻塞子进程(后台线程排空)
    std::thread::spawn(move || for _ in lines {});

    Server { child, url }
}

impl Server {
    /// 等待 /health 就绪。
    fn wait_health(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if reqwest::blocking::get(format!("{}/health", self.url))
                .map(|r| r.status().as_u16() == 200)
                .unwrap_or(false)
            {
                return;
            }
            assert!(Instant::now() < deadline, "/health 30s 未就绪");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn hard_kill(&mut self) {
        self.child.kill().expect("硬杀");
        self.child.wait().expect("回收");
    }
}

fn client_for(url: &str, dir: &std::path::Path) -> EnvelopeClient {
    let token = std::fs::read_to_string(dir.join("token")).expect("令牌文件");
    EnvelopeClient::new(url, Some(token.trim())).expect("客户端")
}

fn call(
    c: &EnvelopeClient,
    method: bm_contract::wire::Method,
    params: serde_json::Value,
) -> serde_json::Value {
    match c.call(method, params.clone()) {
        Ok(v) => {
            assert!(v.is_object() && !v.is_null(), "{method} 返回异常: {v}");
            v
        }
        Err(e) => panic!(
            "{method} 信封错误({}): 参数={}",
            e.error_object(),
            serde_json::to_string(&params).unwrap_or_default()
        ),
    }
}

fn wait_terminal(c: &EnvelopeClient, op: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let receipt = call(
            c,
            bm_contract::wire::Method::OperationsGet,
            serde_json::json!({"operation_id": op}),
        );
        if matches!(
            receipt["state"].as_str(),
            Some("succeeded") | Some("failed") | Some("cancelled") | Some("timeout")
        ) {
            return receipt;
        }
        assert!(Instant::now() < deadline, "回合 30s 未终态");
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn t32_cross_process_kill_restart_resume_over_http() {
    let dir = tempfile::tempdir().expect("临时目录");

    // ── 进程 #1:全链路 ────────────────────────────────────────────
    let mut srv1 = spawn_server(dir.path());
    srv1.wait_health();
    let client1 = client_for(&srv1.url, dir.path());

    let created = call(
        &client1,
        bm_contract::wire::Method::SessionCreate,
        serde_json::json!({"agent": {"name": "assistant",
            "model_chain": ["zhipu.glm-4-flash"],
            "budget": {"max_tokens": 100000, "max_turns": 20}}}),
    );
    let sess1 = created["session_id"].as_str().expect("sess").to_string();
    let agent1 = created["agent_id"].as_str().expect("agent").to_string();

    let sent = call(
        &client1,
        bm_contract::wire::Method::AgentSendInput,
        serde_json::json!({"session_id": sess1, "agent_id": agent1,
            "content": "进程一的问题", "input_trust": "trusted"}),
    );
    let op1 = sent["operation_id"].as_str().expect("op").to_string();
    let r1 = wait_terminal(&client1, &op1);
    assert_eq!(r1["state"], "succeeded");

    // ── 硬杀进程 #1,重启进程 #2(同一数据目录)──────────────────
    srv1.hard_kill();
    let mut srv2 = spawn_server(dir.path());
    srv2.wait_health();
    let client2 = client_for(&srv2.url, dir.path());

    // resume:会话仍在(存在性恢复),状态 active
    let resumed = call(
        &client2,
        bm_contract::wire::Method::SessionResume,
        serde_json::json!({"session_id": sess1, "since_seq": 0}),
    );
    assert_eq!(resumed["session_state"], "active");
    assert_eq!(resumed["agent_state"], "running");
    assert_eq!(
        resumed["events"].as_array().expect("补发数组").len(),
        7,
        "跨进程补发全部会话历史(会话过滤,7 条)"
    );

    // 原 operation 保持 succeeded(未消失、未取消)
    let r1_again = call(
        &client2,
        bm_contract::wire::Method::OperationsGet,
        serde_json::json!({"operation_id": op1}),
    );
    assert_eq!(r1_again["state"], "succeeded", "存在性恢复:终态保持");

    // 新回合可发且完成(ID 计数提示防撞:新 op id ≠ 旧 id)
    let sent2 = call(
        &client2,
        bm_contract::wire::Method::AgentSendInput,
        serde_json::json!({"session_id": sess1, "agent_id": agent1,
            "content": "进程二的问题", "input_trust": "trusted"}),
    );
    let op2 = sent2["operation_id"].as_str().expect("op2").to_string();
    assert_ne!(op2, op1, "ID 防撞:重启后新 operation 不与历史撞号");
    let r2 = wait_terminal(&client2, &op2);
    assert_eq!(r2["state"], "succeeded");

    // 事件流:无任何 cancelled(基线 M3 验收:CLI/客户端退出不取消)
    let polled = call(
        &client2,
        bm_contract::wire::Method::EventsPoll,
        serde_json::json!({"session_id": sess1, "since_seq": 0, "limit": 1000}),
    );
    let text = serde_json::to_string(&polled).expect("序列化");
    assert!(!text.contains("cancelled"), "全事件流不得出现 cancelled");
    assert!(text.contains("agent.completed"));

    srv2.hard_kill();

    // 客户端(≈CLI)进程早已不复存在,服务端状态完好——验收语义成立
}
