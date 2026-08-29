# -*- coding: utf-8 -*-
"""M7-T4 server 二进制:--mcp-config 装配 MCP server(安装 = 显式配置)。"""
import io

P = r'D:\96_CoderWorld\BoenMind\runtime\crates\bm-runtime\src\bin\boenmind-server.rs'
s = io.open(P, encoding='utf-8').read()

pairs = [
    ("""            "--web-dir" => web_dir = Some(PathBuf::from(args.next().expect("--web-dir 需要值"))),
            "--help" | "-h" => {
                println!("boenmind-server [--data-dir <path>] [--bind <addr>] [--web-dir <path>]");
                return Ok(());
            }""",
     """            "--web-dir" => web_dir = Some(PathBuf::from(args.next().expect("--web-dir 需要值"))),
            "--mcp-config" => {
                let v = args.next().expect("--mcp-config 需要值");
                mcp_config = Some(PathBuf::from(v));
            }
            "--help" | "-h" => {
                println!(
                    "boenmind-server [--data-dir <path>] [--bind <addr>] [--web-dir <path>] [--mcp-config <path>]"
                );
                return Ok(());
            }"""),
    ("""    let mut web_dir: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);""",
     """    let mut web_dir: Option<PathBuf> = None;
    let mut mcp_config: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);"""),
]

for old, new in pairs:
    assert s.count(old) == 1, f"anchor: {old[:60]!r} count={s.count(old)}"
    s = s.replace(old, new)

# RuntimeConfig 装配处:扩 capabilities + async_executor(当前为 None)
old = """    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        version: format!("{}-server", env!("CARGO_PKG_VERSION")),"""
new = """    // M7.2/M7.7:--mcp-config 显式安装清单(= 用户批准)→ 握手发现 →
    // 动态注册 + 异步执行器装配;env 明文只进子进程(INV-5)
    let mut capabilities = bm_providers::builtin::builtin_capability_set();
    let mut mcp_executor: Option<Arc<dyn bm_core::ports::AsyncCapabilityExecutor>> = None;
    if let Some(cfg) = &mcp_config {
        let hub = bm_providers::mcp::McpHub::new();
        let setups = bm_providers::mcp::load_mcp_setups(cfg, secrets.as_ref())?;
        for setup in setups {
            let transport = bm_providers::mcp::StdioMcpTransport::spawn(
                &setup.command,
                &setup.args,
                &setup.env_resolved,
            )?;
            let manifests = hub
                .connect(&setup.name, transport, setup.tool_timeout_ms)
                .await?;
            println!(
                "MCP server {} 已接入:{} 个工具",
                setup.name,
                manifests.len()
            );
            capabilities.extend(bm_providers::mcp::McpHub::capability_entries(manifests));
        }
        mcp_executor = Some(hub);
    }

    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities,
        version: format!("{}-server", env!("CARGO_PKG_VERSION")),"""
assert s.count(old) == 1, s.count(old)
s = s.replace(old, new)

old2 = """        capabilities,"""
# 上面已把 capabilities 字段换成聚合值;此处再把 async_executor: None 换成执行器
pairs2 = [
    ("""        async_executor: None,
        version: format!("{}-server", env!("CARGO_PKG_VERSION")),""",
     """        async_executor: mcp_executor,
        version: format!("{}-server", env!("CARGO_PKG_VERSION")),"""),
]
for old3, new3 in pairs2:
    assert s.count(old3) == 1, f"anchor2: {old3[:60]!r} count={s.count(old3)}"
    s = s.replace(old3, new3)

io.open(P, 'w', encoding='utf-8', newline='\n').write(s)
print('server patched')
