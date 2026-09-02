//! 声明式配置端到端回归(2026-09-02,用户要求确认「仍然要有声明式的配置文件」):
//!
//! manifest.json(声明 schema,BoenMind 设置页渲染表单)→ 用户填值 →
//! webadmin 写 `config/mcp-web_multisearch.json` → exe `--config` 指向它并按
//! mtime 热读。本测试以本地 SearXNG mock 验证三件事:
//! 1. `--config` 的值真实驱动行为(searxng 源按配置启用并命中 mock);
//! 2. **改配置文件不用重启,下一次搜索立即生效**(热读);
//! 3. 配置里的 default_limit 生效。
//!
//! 断言只依赖 searxng mock(永远成功),不依赖 ddgs/marginalia 等外网源
//! 的可用性——它们的成败不影响本测试成立。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

struct Mock {
    port: u16,
}

/// 起一个假 SearXNG(JSON API 形状):回显查询词进标题,固定两条结果。
fn start_mock(marker: &'static str) -> Mock {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("mock bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            std::thread::spawn(move || {
                let mut buf = String::new();
                {
                    let mut reader = BufReader::new(match stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => return,
                    });
                    // 读到请求头结束(空行);查询词在首行
                    loop {
                        match reader.read_line(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(_) if buf.ends_with("\r\n\r\n") || buf.ends_with("\n\n") => break,
                            Ok(_) => {
                                if buf.lines().count() > 1
                                    && buf.contains("HTTP/1.1")
                                    && buf.rfind("\r\n\r\n").is_some()
                                {
                                    break;
                                }
                                // 简化:GET 请求头后即空行,读到空行为止
                                if buf.contains("\r\n\r\n") || buf.contains("\n\n") {
                                    break;
                                }
                            }
                        }
                    }
                }
                let first_line = buf.lines().next().unwrap_or_default().to_string();
                let q = first_line
                    .split("q=")
                    .nth(1)
                    .and_then(|rest| rest.split('&').next())
                    .unwrap_or("")
                    .replace("%20", " ")
                    .replace('+', " ");
                let body = serde_json::json!({
                    "results": [
                        {"title": format!("{marker} | {q}"), "url": "https://example.com/mock-1", "content": "mock 描述一"},
                        {"title": format!("{marker}-2"), "url": "https://example.com/mock-2", "content": "mock 描述二"}
                    ]
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let mut stream = stream;
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            });
        }
    });
    Mock { port }
}

fn write_config(path: &std::path::Path, searxng_url: &str, default_limit: u32) {
    std::fs::write(
        path,
        serde_json::json!({"searxng_url": searxng_url, "default_limit": default_limit}).to_string(),
    )
    .expect("写配置文件");
}

#[test]
fn declarative_config_drives_and_hot_reloads() {
    let mock_a = start_mock("MARKER-A");
    let mock_b = start_mock("MARKER-B");
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("mcp-web_multisearch.json");
    write_config(
        &config_path,
        &format!("http://127.0.0.1:{}", mock_a.port),
        5,
    );

    let mut child: Child = Command::new(env!("CARGO_BIN_EXE_web-multisearch"))
        .arg("--config")
        .arg(&config_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn web-multisearch.exe");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));

    let mut send = |req: &str| {
        stdin.write_all(req.as_bytes()).expect("写请求");
        stdin.write_all(b"\n").expect("写换行");
        stdin.flush().expect("flush");
    };
    let mut recv = || -> serde_json::Value {
        let mut line = String::new();
        stdout.read_line(&mut line).expect("读应答行");
        serde_json::from_str(line.trim()).expect("应答应为合法 JSON")
    };

    // 0) 握手
    send(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let resp = recv();
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");

    // 1) 配置 A:searxng 指向 mock A → 搜索命中 MARKER-A(配置驱动行为)
    send(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"web_search_lite","arguments":{"query":"mock query","limit":3}}}"#,
    );
    let resp = recv();
    let sc = &resp["result"]["structuredContent"];
    assert_eq!(sc["success"], true, "searxng mock 应使命中: {sc}");
    let ok: Vec<String> = sc["meta"]["sources_ok"]
        .as_array()
        .expect("sources_ok")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        ok.contains(&"searxng".to_string()),
        "sources_ok 应含 searxng: {ok:?}"
    );
    let web = sc["data"]["web"].as_array().expect("web");
    assert!(
        web.iter()
            .any(|v| v["title"].as_str().unwrap().contains("MARKER-A")),
        "结果应含 MARKER-A: {web:?}"
    );
    assert!(web.len() <= 3, "args limit 生效");

    // 2) 热改配置:searxng 指向 mock B + default_limit=1 → 不重启,下一次搜索即生效
    write_config(
        &config_path,
        &format!("http://127.0.0.1:{}", mock_b.port),
        1,
    );
    std::thread::sleep(std::time::Duration::from_millis(80)); // 确保 mtime 变化

    // 不带 args limit → 配置 default_limit=1 生效(Python 同款语义:args 优先于配置,
    // 故此处必须省略 args limit 才能验证配置项)
    send(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"web_search_lite","arguments":{"query":"second query"}}}"#,
    );
    let resp = recv();
    let sc = &resp["result"]["structuredContent"];
    assert_eq!(sc["success"], true, "热改后 searxng(mock B)仍应命中: {sc}");
    let web = sc["data"]["web"].as_array().expect("web");
    assert_eq!(web.len(), 1, "default_limit=1 应生效: {web:?}");
    assert!(
        web[0]["title"].as_str().unwrap().contains("MARKER-B"),
        "应命中 MARKER-B(证明热读新配置): {:?}",
        sc["meta"]
    );

    let _ = child.kill();
    let _ = child.wait();
}
