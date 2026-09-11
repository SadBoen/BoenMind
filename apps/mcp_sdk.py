# -*- coding: utf-8 -*-
"""BoenMind 插件协议最小 SDK(Python 侧;ADR-0034,issue #56)。

统一 MCP 2024-11-05 / JSON-RPC 2.0 over stdio 的协议面:逐行帧、通知语义、
错误码与 `--self-describe` 声明。app 只需提供工具表与调用处理并调用
[`run_stdio`],协议不变量由本模块单处守门(与 Rust 侧 `boenmind-plugin-sdk` 同构)。

管道读必须用 `sys.stdin.readline()` 循环——`for line in sys.stdin` 在 Windows
管道上受内部缓冲影响会挂起(见 bm-testkit/fixtures/mini_mcp.py 的同款说明)。
"""
import json
import sys

JSONRPC_VERSION = "2.0"
MCP_PROTOCOL_VERSION = "2024-11-05"

# JSON-RPC 2.0 标准错误码
PARSE_ERROR = -32700
INVALID_REQUEST = -32600
METHOD_NOT_FOUND = -32601
INVALID_PARAMS = -32602
INTERNAL_ERROR = -32603


def _write(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def emit_self_describe(payload):
    """若命令行含 `--self-describe`,打印声明并退出(0);否则正常返回。

    须在 app 自身参数解析(如 argparse required)之前调用。
    """
    if "--self-describe" in sys.argv:
        sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
        sys.stdout.flush()
        sys.exit(0)


def _ok(rid, result):
    return {"jsonrpc": JSONRPC_VERSION, "id": rid, "result": result}


def _err(rid, code, message):
    return {
        "jsonrpc": JSONRPC_VERSION,
        "id": rid,
        "error": {"code": code, "message": message},
    }


def run_stdio(server_info, tools, call_tool, custom=None):
    """运行 stdio 协议循环直至 EOF。

    集中处理:坏 JSON → ``-32700``;通知(id 缺省/null)不回复;``initialize``/
    ``ping``/``tools/list``/``tools/call`` 分发;未知工具 → ``-32602``;未知方法
    → ``-32601``;``shutdown``/``exit`` → ``result:null`` 后退出。

    Args:
        server_info: 握手 ``serverInfo`` 字典(至少 name/version;可含附加字段),
            如 ``{"name": "wiki", "version": "0.1.0"}``。
        tools: ``tools/list`` 返回的工具定义列表;其 name 集合同时决定未知工具的
            ``-32602`` 判定。
        call_tool: ``(name, args) -> dict``,仅当 name 在 tools 内时才被调用;
            返回 MCP ``tools/call`` 的 result 负载(content/structuredContent 等)。
        custom: 可选 ``{method: handler}``;handler(params) 返回 result 值,
            用于非 MCP 标准的扩展方法。
    """
    custom = custom or {}
    known_tools = {t["name"] for t in tools}
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
            # JSON-RPC 2.0 强制:无法解析的请求回 -32700 Parse error
            _write(_err(None, PARSE_ERROR, "parse error"))
            continue

        rid = msg.get("id")
        if rid is None:  # 通知:无应答
            continue
        method = msg.get("method")
        params = msg.get("params") or {}

        if method in ("shutdown", "exit"):
            _write(_ok(rid, None))
            break

        if method == "initialize":
            result = {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": server_info,
            }
        elif method == "ping":
            result = {}
        elif method == "tools/list":
            result = {"tools": tools}
        elif method == "tools/call":
            name = params.get("name", "")
            if name not in known_tools:
                _write(_err(rid, INVALID_PARAMS, "未知工具:%s" % name))
                continue
            result = call_tool(name, params.get("arguments") or {})
        elif method in custom:
            result = custom[method](params)
        else:
            _write(_err(rid, METHOD_NOT_FOUND, "method not found"))
            continue
        _write(_ok(rid, result))
