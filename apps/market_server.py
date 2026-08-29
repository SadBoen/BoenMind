# -*- coding: utf-8 -*-
"""Market App——确定性领域 App(M8.2;ADR-0011)。

stdio MCP server(python 标准库):内嵌 fixture 行情(价格以「分」记账,
整数运算,杜绝浮点抖动)——同查询恒同答案,重放逐字节同结果(可评估)。

用法:python market_server.py
"""
import json
import sys

MARKET_DATA_VERSION = "2026.08.0"

# fixture 行情(确定性:版本钉死,价格=整数分)
FIXTURE = {
    "ACME": {"price_cents": 4217, "currency": "USD", "day_high": 4302, "day_low": 4155},
    "GLOBEX": {"price_cents": 1988, "currency": "USD", "day_high": 2011, "day_low": 1950},
    "INITECH": {"price_cents": 7734, "currency": "USD", "day_high": 7800, "day_low": 7601},
}

portfolio = {}  # symbol -> qty(整数;进程内可逆账本)

TOOLS = [
    {
        "name": "quote.get",
        "description": "查询行情快照(确定性 fixture)",
        "inputSchema": {
            "type": "object",
            "properties": {"symbol": {"type": "string"}},
            "required": ["symbol"],
        },
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "portfolio.add",
        "description": "记一笔持仓(进程内账本,可逆)",
        "inputSchema": {
            "type": "object",
            "properties": {
                "symbol": {"type": "string"},
                "qty": {"type": "integer", "minimum": 1},
            },
            "required": ["symbol", "qty"],
        },
        "annotations": {},
    },
    {
        "name": "portfolio.value",
        "description": "组合市值(纯计算,整数分)",
        "inputSchema": {"type": "object"},
        "annotations": {"readOnlyHint": True},
    },
]


def tool_call(name, a):
    if name == "quote.get":
        sym = a.get("symbol", "")
        if sym not in FIXTURE:
            return {"isError": True, "content": [{"type": "text", "text": "unknown symbol"}]}
        row = FIXTURE[sym]
        out = dict(row)
        out["symbol"] = sym
        out["market_data_version"] = MARKET_DATA_VERSION
        return {
            "content": [{"type": "text", "text": json.dumps(out, sort_keys=True)}],
            "structuredContent": out,
        }
    if name == "portfolio.add":
        sym = a.get("symbol", "")
        qty = a.get("qty", 0)
        if sym not in FIXTURE or not isinstance(qty, int) or qty < 1:
            return {"isError": True, "content": [{"type": "text", "text": "bad input"}]}
        portfolio[sym] = portfolio.get(sym, 0) + qty
        return {
            "content": [
                {
                    "type": "text",
                    "text": json.dumps(
                        {"symbol": sym, "qty": portfolio[sym]}, sort_keys=True
                    ),
                }
            ],
            "structuredContent": {"symbol": sym, "qty": portfolio[sym]},
        }
    if name == "portfolio.value":
        total = sum(q * FIXTURE[s]["price_cents"] for s, q in sorted(portfolio.items()))
        out = {
            "value_cents": total,
            "positions": {s: portfolio[s] for s in sorted(portfolio)},
            "market_data_version": MARKET_DATA_VERSION,
        }
        return {
            "content": [{"type": "text", "text": json.dumps(out, sort_keys=True)}],
            "structuredContent": out,
        }
    return {"isError": True, "content": [{"type": "text", "text": "unknown tool"}]}


def main():
    while True:
        line = sys.stdin.readline()
        if not line:
            break
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if "id" not in msg:
            continue
        rid = msg["id"]
        method = msg.get("method")
        if method == "initialize":
            result = {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {
                    "name": "market",
                    "version": "0.1.0",
                    "marketDataVersion": MARKET_DATA_VERSION,
                },
            }
        elif method == "tools/list":
            result = {"tools": TOOLS}
        elif method == "tools/call":
            a = msg.get("params", {}).get("arguments", {})
            result = tool_call(msg.get("params", {}).get("name", ""), a)
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


if __name__ == "__main__":
    main()
