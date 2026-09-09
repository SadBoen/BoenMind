//! P-06 稳态 RSS 采样(issue #33;perf-baseline 表格 P-06 行的接续)。
//! 搁置理由已消解:基线注记「独立进程采样自 M3 守护形态起才有意义」——
//! boenmind-server 守护进程即 M3 形态,本测试以外部 OS 采样器观测其
//! 常驻内存,进程内嵌偏差归零。
//!
//! 运行:`cargo test --release -p bm-runtime --test p06_rss_sampling -- --ignored --nocapture`
//! (默认 #[ignore],与 perf_persist 家族同口径;采样走 OS 工具,Windows =
//! tasklist,Unix = /proc/<pid>/status VmRSS)

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// 子进程 PID 的常驻内存(KB)。Windows = tasklist CSV 尾列;Unix = VmRSS。
#[cfg(windows)]
fn rss_kb(pid: u32) -> Option<u64> {
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().find(|l| l.contains(&pid.to_string()))?;
    // 尾列 = 内存使用(引号内含千位分隔逗号,如 "45,678 K"):取最后一个
    // CSV 字段再提数字,避免把 PID 等列一并拼进来
    let field = line.rsplit("\",\"").next()?;
    let digits: String = field.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(unix)]
fn rss_kb(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn median(mut samples: Vec<u64>) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    samples.sort_unstable();
    Some(samples[samples.len() / 2])
}

#[tokio::test]
#[ignore = "P-06 稳态 RSS 实测:外部采样 100 回合,perf 家族默认忽略(--ignored 运行)"]
async fn p06_steady_state_rss_sampling() {
    let dir = tempfile::tempdir().expect("临时数据目录");
    // 找一个空闲端口:绑了就放,竞态窗口可忽略(测试机本地)
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("探端口");
    let port = listener.local_addr().expect("地址").port();
    drop(listener);

    let exe = env!("CARGO_BIN_EXE_boenmind-server");
    let mut child: Child = Command::new(exe)
        .arg("--data-dir")
        .arg(dir.path())
        .arg("--bind")
        .arg(format!("127.0.0.1:{port}"))
        .stderr(Stdio::null())
        .spawn()
        .expect("拉起 boenmind-server");
    let pid = child.id();

    // 就绪等待(/health;最多 30s)
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(r) = client.get(format!("{base}/health")).send().await
            && r.status().is_success()
        {
            break;
        }
        assert!(Instant::now() < deadline, "server 30s 未就绪");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // 100 回合(每 10 回合采样一次 + 首尾),mock 模型无外网
    let mut samples: Vec<u64> = Vec::new();
    if let Some(kb) = rss_kb(pid) {
        samples.push(kb);
    }
    let mut session = String::new();
    for i in 1..=100u32 {
        let mut req = client
            .post(format!("{base}/v1/chat/completions"))
            .json(&serde_json::json!({
                "model": "mock-model",
                "messages": [{"role": "user", "content": format!("回合 {i}:固定语料")}],
                "stream": false,
            }));
        if !session.is_empty() {
            req = req.header("X-Bm-Session", &session);
        }
        let resp = req.send().await.expect("回合调用");
        assert_eq!(resp.status().as_u16(), 200, "回合 {i} 失败");
        if let Some(sid) = resp
            .headers()
            .get("x-bm-session")
            .and_then(|v| v.to_str().ok())
        {
            session = sid.to_string();
        }
        if i % 10 == 0
            && let Some(kb) = rss_kb(pid)
        {
            samples.push(kb);
        }
    }
    if let Some(kb) = rss_kb(pid) {
        samples.push(kb);
    }
    assert!(samples.len() >= 8, "采样点不足:{}", samples.len());

    // 稳态中位数 = 后半样本的中位数(剔除启动爬坡)
    let steady = median(samples[samples.len() / 2..].to_vec()).expect("稳态样本");

    // 环境元数据(perf-baseline §2:回填必须附带)
    println!(
        "P-06 稳态 RSS 中位数 = {} KB (≈ {:.1} MB)",
        steady,
        steady as f64 / 1024.0
    );
    println!("样本序列(KB) = {:?}", samples);
    println!(
        "环境: os={} profile=test(build) 模型=mock 回合数=100 采样点={}",
        std::env::consts::OS,
        samples.len()
    );
    assert!(steady > 10 * 1024, "稳态 RSS {}KB 低于合理下限", steady);
    assert!(
        steady < 32 * 1024 * 1024,
        "稳态 RSS {steady}KB 超出 32GB 合理上限:采样解析疑似污染"
    );

    // 收尾:杀子进程(数据目录为临时件,随测试目录清理)
    let _ = child.kill();
    let _ = child.wait();
}
