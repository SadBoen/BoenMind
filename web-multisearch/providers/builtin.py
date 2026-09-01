"""内置搜索源重建 — 自包含(不再 import Hermes 的 plugins.web.*)。

行为对齐原 Hermes 内置 provider 的返回结构:
``{"success": bool, "data": {"web": [{"title", "url", "description"}]}}``。
HTTP 全部走 httpx(与自带 providers 同依赖);key 从环境读取
(keystore.provider_env 链:override 文件 > env,由 server.py 注入)。

firecrawl 不迁(v0):它只服务 extract,jina reader 兜底已覆盖。
"""

from __future__ import annotations

import os
from typing import Any, Dict

from .base import WebSearchProvider
from .keystore import provider_env


def _cfg(name: str) -> str:
    """配置读取:override/env 链统一入口(WMS_ 前缀变量也走 env)。"""
    return os.environ.get(name, "")


class SearxngWebSearchProvider(WebSearchProvider):
    """SearXNG 自托管实例(JSON API)。WMS_SEARXNG_URL 非空即启用。"""

    @property
    def name(self) -> str:
        return "searxng"

    @property
    def display_name(self) -> str:
        return "SearXNG (self-hosted)"

    def is_available(self) -> bool:
        return bool(_cfg("WMS_SEARXNG_URL"))

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        import httpx

        base = _cfg("WMS_SEARXNG_URL").rstrip("/")
        try:
            resp = httpx.get(
                f"{base}/search",
                params={"q": query, "format": "json"},
                timeout=12,
            )
            resp.raise_for_status()
            results = resp.json().get("results") or []
        except Exception as exc:  # noqa: BLE001 — provider 约定不抛
            return {"success": False, "error": f"searxng: {exc}"}
        web = [
            {
                "title": r.get("title") or "",
                "url": r.get("url") or "",
                "description": r.get("content") or "",
            }
            for r in results[:limit]
            if r.get("url")
        ]
        return {"success": True, "data": {"web": web}}


class DDGSWebSearchProvider(WebSearchProvider):
    """DuckDuckGo 搜索(ddgs 库,免 key)。"""

    @property
    def name(self) -> str:
        return "ddgs"

    @property
    def display_name(self) -> str:
        return "DuckDuckGo (ddgs)"

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        try:
            from ddgs import DDGS
        except ImportError:
            # 兼容旧包名 duckduckgo-search
            try:
                from duckduckgo_search import DDGS  # type: ignore
            except ImportError as exc:
                return {"success": False, "error": f"ddgs: {exc}"}
        try:
            rows = DDGS().text(query, max_results=limit)
        except Exception as exc:  # noqa: BLE001
            return {"success": False, "error": f"ddgs: {exc}"}
        web = [
            {
                "title": r.get("title") or "",
                "url": r.get("href") or r.get("url") or "",
                "description": r.get("body") or "",
            }
            for r in rows
            if r.get("href") or r.get("url")
        ]
        return {"success": True, "data": {"web": web}}


class TavilyWebSearchProvider(WebSearchProvider):
    """Tavily 搜索 API(TAVILY_API_KEY)。"""

    @property
    def name(self) -> str:
        return "tavily"

    @property
    def display_name(self) -> str:
        return "Tavily"

    def is_available(self) -> bool:
        return bool(provider_env("TAVILY_API_KEY"))

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        import httpx

        key = provider_env("TAVILY_API_KEY")
        try:
            resp = httpx.post(
                "https://api.tavily.com/search",
                json={"api_key": key, "query": query, "max_results": limit},
                timeout=20,
            )
            resp.raise_for_status()
            results = resp.json().get("results") or []
        except Exception as exc:  # noqa: BLE001
            return {"success": False, "error": f"tavily: {exc}"}
        web = [
            {
                "title": r.get("title") or "",
                "url": r.get("url") or "",
                "description": r.get("content") or "",
            }
            for r in results[:limit]
        ]
        return {"success": True, "data": {"web": web}}


class ExaWebSearchProvider(WebSearchProvider):
    """Exa 搜索 API(EXA_API_KEY)。"""

    @property
    def name(self) -> str:
        return "exa"

    @property
    def display_name(self) -> str:
        return "Exa"

    def is_available(self) -> bool:
        return bool(provider_env("EXA_API_KEY"))

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        import httpx

        key = provider_env("EXA_API_KEY")
        try:
            resp = httpx.post(
                "https://api.exa.ai/search",
                headers={"x-api-key": key},
                json={"query": query, "numResults": limit},
                timeout=20,
            )
            resp.raise_for_status()
            results = resp.json().get("results") or []
        except Exception as exc:  # noqa: BLE001
            return {"success": False, "error": f"exa: {exc}"}
        web = [
            {
                "title": r.get("title") or "",
                "url": r.get("url") or "",
                "description": (r.get("text") or "")[:300],
            }
            for r in results
        ]
        return {"success": True, "data": {"web": web}}


class BraveFreeWebSearchProvider(WebSearchProvider):
    """Brave Search API(BRAVE_SEARCH_API_KEY)。"""

    @property
    def name(self) -> str:
        return "brave"

    @property
    def display_name(self) -> str:
        return "Brave Search"

    def is_available(self) -> bool:
        return bool(provider_env("BRAVE_SEARCH_API_KEY"))

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        import httpx

        key = provider_env("BRAVE_SEARCH_API_KEY")
        try:
            resp = httpx.get(
                "https://api.search.brave.com/res/v1/web/search",
                params={"q": query, "count": limit},
                headers={
                    "X-Subscription-Token": key,
                    "Accept": "application/json",
                },
                timeout=20,
            )
            resp.raise_for_status()
            results = (resp.json().get("web") or {}).get("results") or []
        except Exception as exc:  # noqa: BLE001
            return {"success": False, "error": f"brave: {exc}"}
        web = [
            {
                "title": r.get("title") or "",
                "url": r.get("url") or "",
                "description": r.get("description") or "",
            }
            for r in results[:limit]
        ]
        return {"success": True, "data": {"web": web}}
