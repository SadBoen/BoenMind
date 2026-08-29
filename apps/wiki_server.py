# -*- coding: utf-8 -*-
"""Wiki App——首批真实 App(M8.1;ADR-0011)。

stdio MCP server(python 标准库):文件域真实持久,page.write 是真实
世界副作用(磁盘文件变更),出执行收据(内容摘要 + 字节数)。

用法:python wiki_server.py --dir <wiki 目录>
安全:name 白名单字符集且禁止路径穿越——App 只能触自己的数据域
(M7.6 隔离在 App 侧的落地)。
"""
import argparse
import hashlib
import json
import os
import re
import sys

NAME_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}")

TOOLS = [
    {
        "name": "page.read",
        "description": "读取 wiki 页面(只读)",
        "inputSchema": {
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"],
        },
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "page.write",
        "description": "写入 wiki 页面(真实写盘;返回内容摘要收据)",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "content": {"type": "string"},
            },
            "required": ["name", "content"],
        },
        "annotations": {"destructiveHint": True},
    },
    {
        "name": "page.list",
        "description": "列出全部页面名(只读)",
        "inputSchema": {"type": "object"},
        "annotations": {"readOnlyHint": True},
    },
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    opts = ap.parse_args()
    wiki_dir = os.path.abspath(opts.dir)
    os.makedirs(wiki_dir, exist_ok=True)

    def safe_path(name):
        if not name or ".." in name or not NAME_RE.fullmatch(name):
            return None
        return os.path.join(wiki_dir, name + ".md")

    def tool_call(name, a):
        if name == "page.read":
            p = safe_path(a.get("name", ""))
            if p is None:
                return {"isError": True, "content": [{"type": "text", "text": "bad name"}]}
            if not os.path.exists(p):
                return {"isError": True, "content": [{"type": "text", "text": "not found"}]}
            with open(p, "rb") as f:
                data = f.read()
            return {
                "content": [
                    {
                        "type": "text",
                        "text": data.decode("utf-8", errors="replace"),
                    }
                ],
                "structuredContent": {"bytes": len(data)},
            }
        if name == "page.write":
            p = safe_path(a.get("name", ""))
            if p is None:
                return {"isError": True, "content": [{"type": "text", "text": "bad name"}]}
            content = a.get("content", "")
            data = content.encode("utf-8")
            with open(p, "wb") as f:
                f.write(data)
            receipt = {
                "sha256": hashlib.sha256(data).hexdigest(),
                "bytes": len(data),
                "written": True,
            }
            return {
                "content": [{"type": "text", "text": json.dumps(receipt, sort_keys=True)}],
                "structuredContent": receipt,
            }
        if name == "page.list":
            names = sorted(
                f[:-3] for f in os.listdir(wiki_dir) if f.endswith(".md")
            )
            return {
                "content": [{"type": "text", "text": json.dumps(names)}],
                "structuredContent": {"pages": names},
            }
        return {"isError": True, "content": [{"type": "text", "text": "unknown tool"}]}

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
                "serverInfo": {"name": "wiki", "version": "0.1.0"},
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
