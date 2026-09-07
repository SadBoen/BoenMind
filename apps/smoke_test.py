# P1-38(2026-09-07 架构评审):apps 三个 stdio MCP server 的协议冒烟门禁。
# 此前零测试零 CI,发布包直接打包未测脚本。本测试拉起每个 server 走真实
# stdio 管道:initialize 握手 → tools/list → tools/call 未知工具(应用层
# isError)→ 未知 method(-32601)→ 坏 JSON(-32700)。
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def rpc(proc: subprocess.Popen, obj: dict) -> dict:
    proc.stdin.write(json.dumps(obj) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    assert line, f"{proc.args}: 服务端无响应"
    return json.loads(line)


def smoke(script: str, extra_args: list[str]) -> None:
    proc = subprocess.Popen(
        [sys.executable, script, *extra_args],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        encoding="utf-8",
    )
    try:
        init = rpc(
            proc,
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        )
        assert init["result"]["protocolVersion"] == "2024-11-05", init

        tools = rpc(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        listed = tools["result"]["tools"]
        assert isinstance(listed, list) and listed, tools

        # MCP 规范口径:未知工具 = 应用层错误(isError:true),非协议错误
        unk = rpc(
            proc,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "no_such_tool", "arguments": {}},
            },
        )
        assert unk["result"].get("isError") is True, unk

        # 未知 method = -32601 method not found
        nf = rpc(proc, {"jsonrpc": "2.0", "id": 4, "method": "no/such"})
        assert nf.get("error", {}).get("code") == -32601, nf

        # 坏 JSON = -32700 parse error(P1-39:此前静默吞掉)
        proc.stdin.write("{not-json\n")
        proc.stdin.flush()
        pe = json.loads(proc.stdout.readline())
        assert pe.get("error", {}).get("code") == -32700, pe
    finally:
        proc.kill()
        proc.wait()


def main() -> None:
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        # wiki 强制要求 --dir;music 可缺省(传临时目录更干净);market 无参数
        plan = [
            ("wiki_server.py", ["--dir", tmp]),
            ("market_server.py", []),
            ("music_server.py", ["--dir", tmp]),
        ]
        for name, args in plan:
            smoke(os.path.join(HERE, name), args)
            print(f"[smoke] {name} OK")
    print("apps smoke: all green")


if __name__ == "__main__":
    main()
