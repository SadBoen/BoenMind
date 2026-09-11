# MCP/JSON-RPC 协议冒烟门禁(单源;2026-09-12 合并原 apps/smoke_test.py 与
# plugins/smoke_test.py——两份 rpc() 与五步断言序列此前逐字重复,协议口径
# 改一处即两族同改)。
#
# 覆盖(真 stdio 管道,全协议往返):
#   initialize 握手(protocolVersion=2024-11-05;插件另断 serverInfo.name 自报)
#   → tools/list 非空 → 未知工具(-32602,ADR-0034 统一规范口径,对齐 MCP
#   2024-11-05 规范示例)→ 未知 method(-32601)→ 坏 JSON(-32700,JSON-RPC
#   2.0 强制;P1-39:此前 app 侧静默吞掉)。
#
# 用法:
#   python scripts/mcp_smoke.py                 # apps 三 server 全测(P1-38 门禁)
#   python scripts/mcp_smoke.py --app music     # 只测一个 app
#   python scripts/mcp_smoke.py --plugin web-multisearch   # 单个官方插件
import argparse
import json
import os
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PROTOCOL_VERSION = "2024-11-05"


def rpc(proc: subprocess.Popen, obj: dict) -> dict:
    proc.stdin.write(json.dumps(obj) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    assert line, f"{proc.args}: 服务端无响应"
    return json.loads(line)


def assert_protocol(proc: subprocess.Popen, expect_server_name: str | None = None) -> None:
    """五步协议不变量(两族同一口径)。expect_server_name 非空时加断
    serverInfo.name 自报(官方 Rust 插件用;app 侧历史无此断,如实保留)。"""
    init = rpc(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
    assert init["result"]["protocolVersion"] == PROTOCOL_VERSION, init
    if expect_server_name is not None:
        assert init["result"]["serverInfo"]["name"] == expect_server_name, init

    tools = rpc(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    listed = tools["result"]["tools"]
    assert isinstance(listed, list) and listed, tools

    # ADR-0034:MCP 规范口径:未知工具 = 协议错误 -32602(非应用层 isError)
    unk = rpc(
        proc,
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "no_such_tool", "arguments": {}},
        },
    )
    assert unk.get("error", {}).get("code") == -32602, unk

    # 未知 method = -32601 method not found
    nf = rpc(proc, {"jsonrpc": "2.0", "id": 4, "method": "no/such"})
    assert nf.get("error", {}).get("code") == -32601, nf

    # 坏 JSON = -32700 parse error(P1-39:此前静默吞掉)
    proc.stdin.write("{not-json\n")
    proc.stdin.flush()
    pe = json.loads(proc.stdout.readline())
    assert pe.get("error", {}).get("code") == -32700, pe


def spawn(cmd: list[str]) -> subprocess.Popen:
    return subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        encoding="utf-8",
    )


# (脚本名, serverInfo.name 自报值;app 侧暂不断自报名)
APP_SERVERS = {
    "wiki": ("wiki_server.py", None),
    "market": ("market_server.py", None),
    "music": ("music_server.py", None),
}

# (cargo bin 名, serverInfo.name 自报值;自报值 = 自描述声明名,下划线字符集)
PLUGINS = {
    "context-inspector": "context_inspector",
    "web-multisearch": "web_multisearch",
}


def smoke_app(name: str, tmp: str) -> None:
    script, _ = APP_SERVERS[name]
    proc = spawn(
        [sys.executable, os.path.join(REPO, "apps", script), *(["--dir", tmp] if name != "market" else [])]
    )
    try:
        assert_protocol(proc)
    finally:
        proc.kill()
        proc.wait()
    print(f"[smoke] {script} OK")


def find_bin(plugin: str) -> str:
    base = os.path.join(REPO, "plugins", "mcp", plugin, "target", "release", plugin)
    for cand in (base + ".exe", base):
        if os.path.exists(cand):
            return cand
    raise SystemExit(f"未找到插件二进制 {base}(先 cargo build --release)")


def smoke_plugin(plugin: str) -> None:
    proc = spawn([find_bin(plugin)])
    try:
        assert_protocol(proc, PLUGINS[plugin])
    finally:
        proc.kill()
        proc.wait()
    print(f"[plugin-smoke] {plugin} OK")


def main() -> int:
    ap = argparse.ArgumentParser(description="MCP/JSON-RPC 协议冒烟(apps + 官方插件单源)")
    ap.add_argument("--app", choices=sorted(APP_SERVERS), action="append", help="只测指定 app(可重复)")
    ap.add_argument("--plugin", choices=sorted(PLUGINS), action="append", help="测指定官方插件(可重复)")
    args = ap.parse_args()

    if args.plugin:
        for plugin in args.plugin:
            smoke_plugin(plugin)
    else:
        # wiki/music 强制/宜传 --dir;market 无参数。临时目录复用原 apps 门禁口径
        import tempfile

        selected = args.app or ["wiki", "market", "music"]
        with tempfile.TemporaryDirectory() as tmp:
            for name in selected:
                smoke_app(name, tmp)
    print("mcp smoke: all green")
    return 0


if __name__ == "__main__":
    sys.exit(main())
