#!/usr/bin/env python
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "mcp>=1.2.0,<2",
#   "httpx>=0.27",
#   "ddgs>=9.0",
# ]
# ///
"""boenmind-mcp-multisearch — Web-Multisearch 聚合搜索的 MCP server。

迁移自 Hermes 插件 hermes-plugin-web-multisearch(自包含,不依赖 Hermes 运行时)。

工具:
- ``web_search_lite`` — 日常组合:searxng + ddgs + jina + marginalia(全免费源)
- ``web_search_all``  — 全源并行聚合(RRF 融合排序+CJK 镜像合并去重+多 Key 轮换)

自声明式配置:manifest.json 与本文件同目录,声明全部配置项
(BoenMind 设置页据此渲染表单);配置经 --config <json> 传入,
启动时注入环境/override 文件,provider 的免重启改 key 链保持原样。

运行:uv run server.py --config /path/to/config.json
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from mcp.server.fastmcp import FastMCP

from providers.omni import LiteWebSearchProvider, OmniWebSearchProvider, format_result

# 配置项 key → provider 读取的环境变量名(keystore.KEY_SOURCES 的镜像)
_ENV_MAP = {
    "serper_api_key": "SERPER_API_KEY",
    "jina_api_key": "JINA_API_KEY",
    "tavily_api_key": "TAVILY_API_KEY",
    "exa_api_key": "EXA_API_KEY",
    "brave_api_key": "BRAVE_SEARCH_API_KEY",
    "langsearch_api_key": "LANGSEARCH_API_KEY",
    "linkup_api_key": "LINKUP_API_KEY",
    "you_api_key": "YOU_API_KEY",
    "websearchapi_api_key": "WEBSEARCHAPI_API_KEY",
}

_LITE = LiteWebSearchProvider()
_ALL = OmniWebSearchProvider()
DEFAULT_LIMIT = 5


def load_config(path: str | None) -> dict:
    """读配置文件并注入运行环境:key 走 override 文件链(免重启换 key)。"""
    if not path:
        return {}
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    # 覆盖文件路径可由配置指定(默认 ~/.hermes/plugins/.env-overrides/...)
    overrides_path = data.get("overrides_path")
    if overrides_path:
        os.environ["WMS_OVERRIDES_PATH"] = overrides_path
    # key 注入 override 文件(keystore.set_key:0600 + mtime 缓存失效)
    # 延迟导入:providers.keystore 需要 WMS_OVERRIDES_PATH 已就位
    from providers.keystore import set_key

    for cfg_key, env_name in _ENV_MAP.items():
        value = str(data.get(cfg_key) or "").strip()
        if value:
            set_key(env_name, value)
    # 非密钥配置直接进环境(searxng 地址等)
    if str(data.get("searxng_url") or "").strip():
        os.environ["WMS_SEARXNG_URL"] = str(data["searxng_url"]).strip()
    return data


def _limit(args_limit: int | None, config: dict) -> int:
    try:
        raw = args_limit if args_limit is not None else int(config.get("default_limit", DEFAULT_LIMIT))
    except (TypeError, ValueError):
        raw = DEFAULT_LIMIT
    return min(max(int(raw), 1), 100)


mcp = FastMCP("web-multisearch")


@mcp.tool()
def web_search_lite(query: str, limit: int | None = None) -> str:
    """日常聚合搜索:searxng + ddgs + jina + marginalia(全免费源)并行,
    RRF 融合排序+同题镜像合并去重,description 带 [来源] 标注。
    一般搜索优先用这个,快且免费。"""
    if not query.strip():
        return json.dumps({"success": False, "error": "query 参数不能为空"}, ensure_ascii=False)
    try:
        result = _LITE.search(query.strip(), _limit(limit, CONFIG))
        return format_result(result)
    except Exception as exc:  # noqa: BLE001 — 工具约定:错误也返回 JSON,不抛
        return json.dumps({"success": False, "error": f"web_search_lite failed: {exc}"}, ensure_ascii=False)


@mcp.tool()
def web_search_all(query: str, limit: int | None = None) -> str:
    """全网搜:并行调用所有已配置搜索源(最多 12 家),RRF 融合排序+镜像合并,
    meta 带各源耗时遥测。用户要求「全网搜」、需要最大覆盖或交叉验证时使用。"""
    if not query.strip():
        return json.dumps({"success": False, "error": "query 参数不能为空"}, ensure_ascii=False)
    try:
        result = _ALL.search(query.strip(), _limit(limit, CONFIG))
        return format_result(result)
    except Exception as exc:  # noqa: BLE001
        return json.dumps({"success": False, "error": f"web_search_all failed: {exc}"}, ensure_ascii=False)


CONFIG: dict = {}

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="web-multisearch MCP server")
    parser.add_argument("--config", help="配置文件路径(JSON,自 manifest.json 的 schema)")
    args = parser.parse_args()
    CONFIG = load_config(args.config)
    mcp.run()
