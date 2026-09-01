"""MCP stdio 协议层冒烟:initialize → tools/list → tools/call 真搜索。"""
import asyncio
import json

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


async def main() -> None:
    params = StdioServerParameters(
        command="uv",
        args=["run", "--with", "mcp>=1.2.0,<2", "--with", "httpx", "--with", "ddgs",
              "python", "server.py"],
        cwd=r"D:\96_CoderWorld\boenmind-mcp-servers\web-multisearch",
    )
    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            tools = await session.list_tools()
            print("tools:", [t.name for t in tools.tools])
            res = await session.call_tool("web_search_lite", {"query": "assistant-ui react", "limit": 3})
            text = res.content[0].text
            data = json.loads(text)
            print("lite success:", data.get("success"))
            for item in (data.get("data") or {}).get("web") or []:
                print(" -", item.get("position"), item.get("title", "")[:60], "|", item.get("url", "")[:60])


asyncio.run(main())
