"""四个 keyed 搜索源 — 自 dsh-plugin-multi-search 移植(langsearch/linkup/you/websearchapi)。

与 serper/jina 同款约定:keystore override 链取 key、逗号分隔多 Key、
401/403/429 自动轮换。全部 search-only(不支持 extract)。
"""

from __future__ import annotations

import logging
from typing import Any, Dict, List

from .base import WebSearchProvider

from .keystore import provider_env
from .keys import HttpStatusError, split_keys, with_key_rotation

logger = logging.getLogger(__name__)


class _KeyedSearchProvider(WebSearchProvider):
    """通用 keyed 单端点搜索源骨架:子类只实现 _request/_parse。"""

    source_name = "keyed"
    display_label = "Keyed source"
    default_budget = 1000  # 每月预算缺省(设置页进度条参照,可改)

    @property
    def name(self) -> str:
        return self.source_name

    @property
    def display_name(self) -> str:
        return self.display_label

    def is_available(self) -> bool:
        return bool(provider_env(self.env_name))

    def supports_search(self) -> bool:
        return True

    def supports_extract(self) -> bool:
        return False

    def monthly_budget(self) -> int:
        return self.default_budget

    def _request(self, httpx, key: str, query: str, limit: int):
        """发起一次 HTTP 请求,返回 response(轮换语义:HTTPStatusError 带状态码抛出)。"""
        raise NotImplementedError

    def _parse(self, data: Any, limit: int) -> "List[tuple]":
        """解析响应为 [(title, url, description)] 行列表。"""
        raise NotImplementedError

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        import httpx

        candidates = split_keys(provider_env(self.env_name))
        if not candidates:
            return {"success": False, "error": f"{self.env_name} is not set"}
        limit = max(1, min(int(limit), 50))

        def _attempt(key: str):
            try:
                return self._request(httpx, key, query, limit)
            except httpx.HTTPStatusError as exc:
                raise HttpStatusError(
                    exc.response.status_code,
                    f"{self.source_name} returned HTTP {exc.response.status_code}",
                ) from exc

        try:
            resp = with_key_rotation(candidates, _attempt)
            items = self._parse(resp.json(), limit)
        except HttpStatusError as exc:
            logger.warning("%s HTTP error: %s", self.source_name, exc)
            return {"success": False, "error": str(exc)}
        except httpx.RequestError as exc:
            logger.warning("%s request error: %s", self.source_name, exc)
            return {"success": False, "error": f"Could not reach {self.source_name}: {exc}"}
        except ValueError as exc:
            return {"success": False, "error": f"{self.source_name} response is not JSON: {exc}"}

        web = [
            {"title": str(t), "url": str(u), "description": str(d or ""), "position": i + 1}
            for i, (t, u, d) in enumerate(items[:limit])
        ]
        logger.info("%s '%s': %d results (limit %d)", self.source_name, query, len(web), limit)
        return {"success": True, "data": {"web": web}}

    def get_setup_schema(self) -> Dict[str, Any]:
        return {
            "name": self.display_label,
            "badge": "keyed",
            "tag": f"{self.display_label} — multi-key rotation supported.",
            "env_vars": [{"key": self.env_name, "prompt": f"{self.env_name}", "url": ""}],
        }


class LangSearchWebSearchProvider(_KeyedSearchProvider):
    """LangSearch — 免费额度,原生 freshness 过滤(dsh 移植)。"""

    source_name = "langsearch"
    display_label = "LangSearch"
    env_name = "LANGSEARCH_API_KEY"
    default_budget = 1000

    def _request(self, httpx, key, query, limit):
        return httpx.post(
            "https://api.langsearch.com/v1/web-search",
            json={"query": query, "count": limit, "summary": False, "freshness": "noLimit"},
            headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json",
                     "accept": "application/json"},
            timeout=15,
        )

    def _parse(self, data, limit):
        pages = ((data or {}).get("data") or {}).get("webPages") or {}
        value = pages.get("value")
        if not isinstance(value, list):
            raise ValueError("unexpected LangSearch response")
        return [
            (v.get("name") or v.get("url") or "", v.get("url") or "",
             v.get("snippet") or v.get("summary") or "")
            for v in value if v.get("url")
        ]


class LinkupWebSearchProvider(_KeyedSearchProvider):
    """Linkup — sourcedAnswer 搜索(推广赠金,dsh 移植)。"""

    source_name = "linkup"
    display_label = "Linkup"
    env_name = "LINKUP_API_KEY"
    default_budget = 500

    def _request(self, httpx, key, query, limit):
        return httpx.post(
            "https://api.linkup.so/v1/search",
            json={"q": query, "depth": "standard", "outputType": "sourcedAnswer"},
            headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json",
                     "accept": "application/json"},
            timeout=15,
        )

    def _parse(self, data, limit):
        payload = data or {}
        raw = list(payload.get("results") or []) + list(payload.get("sources") or [])
        if payload.get("results") is None and payload.get("sources") is None:
            raise ValueError("unexpected LinkUp response")
        return [
            (it.get("name") or it.get("url") or "", it.get("url") or "",
             it.get("content") or it.get("snippet") or "")
            for it in raw if it.get("url")
        ]


class YouWebSearchProvider(_KeyedSearchProvider):
    """You.com Data API(100 请求/天,dsh 移植)。"""

    source_name = "you"
    display_label = "You.com"
    env_name = "YOU_API_KEY"
    default_budget = 3000

    def _request(self, httpx, key, query, limit):
        return httpx.get(
            "https://ydc-index.io/v1/search",
            params={"query": query, "count": limit},
            headers={"x-api-key": key, "accept": "application/json"},
            timeout=15,
        )

    def _parse(self, data, limit):
        payload = data or {}
        results = payload.get("results")
        group = results if isinstance(results, list) else (results or {}).get("web")
        items = group or (payload.get("hits") or {}).get("results") or (payload.get("web") or {}).get("results")
        if not isinstance(items, list):
            raise ValueError("unexpected You.com response")
        return [
            (it.get("title") or it.get("url") or "", it.get("url") or "",
             it.get("description") or it.get("snippet") or "")
            for it in items if it.get("url")
        ]


class WebSearchApiProvider(_KeyedSearchProvider):
    """WebSearchAPI.ai(2000 credits/月,dsh 移植)。"""

    source_name = "websearchapi"
    display_label = "WebSearchAPI.ai"
    env_name = "WEBSEARCHAPI_API_KEY"
    default_budget = 2000

    def _request(self, httpx, key, query, limit):
        return httpx.post(
            "https://api.websearchapi.ai/ai-search",
            json={"query": query, "maxResults": limit, "includeContent": False,
                  "country": "us", "language": "en"},
            headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json",
                     "accept": "application/json"},
            timeout=15,
        )

    def _parse(self, data, limit):
        organic = (data or {}).get("organic")
        if not isinstance(organic, list):
            raise ValueError("unexpected WebSearchAPI response")
        return [
            (it.get("title") or it.get("url") or "", it.get("url") or "",
             it.get("description") or "")
            for it in organic if it.get("url")
        ]
