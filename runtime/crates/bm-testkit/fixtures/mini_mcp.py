# -*- coding: utf-8 -*-
"""最小 MCP stdio server 测试夹具(newline-delimited JSON-RPC 2.0)。

零外部依赖、零密钥:仅实现 initialize / notifications/initialized /
tools/list / tools/call(ping 工具)。供 m7_mcp_tests t104 使用。

注意:管道读必须用 readline() 循环——`for line in sys.stdin` 在
Windows 管道上受内部缓冲影响,会等到缓冲满才返回(挂起陷阱)。
"""
import json
import os
import sys


def main():
    # MINI_MCP_DIE_AFTER=N:应答完第 N 个请求后退出(模拟子进程崩溃,
    # 供 stdio 重生语义测试;重生代继承同一环境,再走一轮)
    die_after = int(os.environ.get("MINI_MCP_DIE_AFTER", "0") or 0)
    answered = 0
    while True:
        line = sys.stdin.readline()
        if not line:  # EOF
            break
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if "id" not in msg:
            continue  # 通知:无需响应
        rid = msg["id"]
        method = msg.get("method")
        if method == "initialize":
            result = {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "mini", "version": "0.0.1"},
            }
        elif method == "tools/list":
            result = {
                "tools": [
                    {
                        "name": "ping",
                        "inputSchema": {"type": "object"},
                        "annotations": {"readOnlyHint": True},
                    }
                ]
            }
        elif method == "tools/call":
            result = {
                "content": [{"type": "text", "text": "pong"}],
                "isError": False,
            }
        else:
            print(
                json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": rid,
                        "error": {"code": -32601, "message": "method not found"},
                    }
                ),
                flush=True,
            )
            continue
        print(json.dumps({"jsonrpc": "2.0", "id": rid, "result": result}), flush=True)
        answered += 1
        if die_after and answered >= die_after:
            sys.exit(0)


if __name__ == "__main__":
    main()
