# -*- coding: utf-8 -*-
"""apps/mcp_sdk.py 单测(ADR-0034,issue #56)。

在进程内驱动 run_stdio:替换 stdin/stdout,断言协议不变量(initialize 握手、
通知跳过、未知工具 -32602、未知方法 -32601、坏 JSON -32700、shutdown 收尾、
自定义方法)。apps/smoke_test.py 覆盖真实子进程往返,本文件覆盖 SDK 分支细节。
"""
import io
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import mcp_sdk  # noqa: E402

TOOLS = [
    {
        "name": "t.echo",
        "description": "回显",
        "inputSchema": {"type": "object"},
        "annotations": {"readOnlyHint": True},
    }
]


def _drive(lines):
    """喂入若干行,返回 run_stdio 写出的应答列表。"""
    payload = "\n".join(lines) + "\n"
    old_in, old_out = sys.stdin, sys.stdout
    sys.stdin = io.StringIO(payload)
    sys.stdout = io.StringIO()
    try:
        mcp_sdk.run_stdio(
            server_info={"name": "unit", "version": "9.9"},
            tools=TOOLS,
            call_tool=lambda name, args: {
                "content": [{"type": "text", "text": json.dumps(args)}],
                "structuredContent": args,
                "isError": False,
            },
            custom={"unit.ping": lambda params: {"pong": True}},
        )
        out = sys.stdout.getvalue()
    finally:
        sys.stdin, sys.stdout = old_in, old_out
    return [json.loads(ln) for ln in out.splitlines() if ln.strip()]


def test_initialize_handshake():
    (resp,) = _drive(['{"jsonrpc":"2.0","id":1,"method":"initialize"}'])
    assert resp["id"] == 1
    assert resp["result"]["protocolVersion"] == "2024-11-05"
    assert resp["result"]["serverInfo"] == {"name": "unit", "version": "9.9"}
    assert resp["result"]["capabilities"] == {"tools": {}}


def test_notification_gets_no_reply():
    assert _drive(['{"jsonrpc":"2.0","method":"notifications/initialized"}']) == []


def test_tools_list():
    (resp,) = _drive(['{"jsonrpc":"2.0","id":2,"method":"tools/list"}'])
    assert resp["result"]["tools"] == TOOLS


def test_tool_call_success():
    (resp,) = _drive(
        [
            '{"jsonrpc":"2.0","id":3,"method":"tools/call",'
            '"params":{"name":"t.echo","arguments":{"v":1}}}'
        ]
    )
    assert resp["result"]["structuredContent"] == {"v": 1}
    assert resp["result"]["isError"] is False


def test_unknown_tool_is_invalid_params():
    (resp,) = _drive(
        [
            '{"jsonrpc":"2.0","id":4,"method":"tools/call",'
            '"params":{"name":"nope","arguments":{}}}'
        ]
    )
    assert resp["error"]["code"] == mcp_sdk.INVALID_PARAMS == -32602


def test_unknown_method_is_method_not_found():
    (resp,) = _drive(['{"jsonrpc":"2.0","id":5,"method":"resources/list"}'])
    assert resp["error"]["code"] == mcp_sdk.METHOD_NOT_FOUND == -32601


def test_bad_json_is_parse_error():
    (resp,) = _drive(["{not-json"])
    assert resp["error"]["code"] == mcp_sdk.PARSE_ERROR == -32700
    assert resp["id"] is None


def test_custom_method_routed():
    (resp,) = _drive(['{"jsonrpc":"2.0","id":6,"method":"unit.ping"}'])
    assert resp["result"] == {"pong": True}


def test_shutdown_replies_null_and_stops():
    # shutdown 后不应再处理后续行
    out = _drive(
        [
            '{"jsonrpc":"2.0","id":7,"method":"shutdown"}',
            '{"jsonrpc":"2.0","id":8,"method":"initialize"}',
        ]
    )
    assert len(out) == 1
    assert out[0]["id"] == 7
    assert out[0]["result"] is None


def _run_all():
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for t in tests:
        t()
        print("[mcp-sdk-test] %s OK" % t.__name__)
    print("mcp_sdk tests: all green")


if __name__ == "__main__":
    _run_all()
