# issue #56:官方插件的 MCP/JSON-RPC 协议一致性冒烟。
#
# 历史缺口:apps/ 三个 Python server 已有 apps/smoke_test.py(P1-38)做协议门禁,
# 但 2 个 Rust 插件(web-multisearch / context-inspector)曾各自手写 stdio
# JSON-RPC 循环,零跨实现一致性测试。ADR-0034 后两插件协议面收口到
# boenmind-plugin-sdk,本冒烟作为端到端门禁保留(真 stdio 管道,覆盖 SDK 集成)。
#
# 本测试对每个插件走真实 stdio 管道,断言与 apps/ 同族的协议不变量:
#   initialize 握手(protocolVersion=2024-11-05 + serverInfo.name 自报)
#   → tools/list 非空 → 未知工具(统一规范口径 -32602)→ 未知 method(-32601)
#   → 坏 JSON(-32700,JSON-RPC 2.0 强制)。
# ADR-0034:未知工具口径已统一为 -32602(协议错误,对齐 MCP 2024-11-05 规范示例);
# app 侧原 isError:true 一并改齐,两族断言不再分歧。
import argparse
import json
import os
import subprocess
import sys

PROTOCOL_VERSION = "2024-11-05"
HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)


def rpc(proc: subprocess.Popen, obj: dict) -> dict:
    proc.stdin.write(json.dumps(obj) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    assert line, f"{proc.args}: 服务端无响应"
    return json.loads(line)


def find_bin(plugin: str) -> str:
    base = os.path.join(REPO, "plugins", "mcp", plugin, "target", "release", plugin)
    for cand in (base + ".exe", base):
        if os.path.exists(cand):
            return cand
    raise SystemExit(f"未找到插件二进制 {base}(先 cargo build --release)")


def smoke(plugin: str, server_name: str) -> None:
    bin_path = find_bin(plugin)
    proc = subprocess.Popen(
        [bin_path],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        encoding="utf-8",
    )
    try:
        init = rpc(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
        assert init["result"]["protocolVersion"] == PROTOCOL_VERSION, init
        assert init["result"]["serverInfo"]["name"] == server_name, init

        tools = rpc(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        listed = tools["result"]["tools"]
        assert isinstance(listed, list) and listed, tools

        # 未知工具:统一规范口径 = -32602 Invalid params(MCP 2024-11-05 规范示例)
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

        # 坏 JSON = -32700 Parse error(JSON-RPC 2.0 强制)
        proc.stdin.write("{not-json\n")
        proc.stdin.flush()
        pe = json.loads(proc.stdout.readline())
        assert pe.get("error", {}).get("code") == -32700, pe
    finally:
        proc.kill()
        proc.wait()


# (cargo bin 名, serverInfo.name 自报值;自报值 = 自描述声明名,下划线字符集)
PLUGINS = {
    "context-inspector": "context_inspector",
    "web-multisearch": "web_multisearch",
}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--plugin",
        choices=sorted(PLUGINS),
        help="只测一个插件;缺省测全部",
    )
    args = ap.parse_args()
    selected = [args.plugin] if args.plugin else sorted(PLUGINS)
    for plugin in selected:
        smoke(plugin, PLUGINS[plugin])
        print(f"[plugin-smoke] {plugin} OK")
    print("plugin smoke: all green")
    return 0


if __name__ == "__main__":
    sys.exit(main())
