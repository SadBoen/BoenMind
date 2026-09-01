"""Marginalia Search — 独立索引搜索引擎(免 Key,自 dsh-plugin-multi-search 移植)。

非商业化独立索引,与 Google/Bing 重叠度低,给聚合器补一类"独立网"
结果。公共共享 key(``api-key: public``),免费但限速——单源失败不影响
聚合整体。
"""

from __future__ import annotations

import logging
from typing import Any, Dict

from .base import WebSearchProvider

logger = logging.getLogger(__name__)

_ENDPOINT = "https://api2.marginalia-search.com/search"


class MarginaliaWebSearchProvider(WebSearchProvider):
    """Search-only provider against the Marginalia public API."""

    @property
    def name(self) -> str:
        return "marginalia"

    @property
    def display_name(self) -> str:
        return "Marginalia (独立索引,免 Key)"

    def is_available(self) -> bool:
        return True

    def supports_search(self) -> bool:
        return True

    def supports_extract(self) -> bool:
        return False

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        import httpx

        try:
            resp = httpx.get(
                _ENDPOINT,
                params={"query": query, "count": max(1, min(int(limit), 50))},
                headers={"api-key": "public", "accept": "application/json"},
                timeout=12,
            )
            resp.raise_for_status()
            data = resp.json()
        except httpx.HTTPStatusError as exc:
            logger.warning("Marginalia HTTP error: %s", exc)
            return {"success": False, "error": f"Marginalia returned HTTP {exc.response.status_code}"}
        except httpx.RequestError as exc:
            logger.warning("Marginalia request error: %s", exc)
            return {"success": False, "error": f"Could not reach Marginalia: {exc}"}
        except ValueError as exc:
            return {"success": False, "error": f"Marginalia response is not JSON: {exc}"}

        raw = data.get("results")
        if not isinstance(raw, list):
            return {"success": False, "error": "Unexpected Marginalia response shape"}

        web_results = []
        for item in raw:
            url = str((item or {}).get("url") or "").strip()
            if not url:
                continue
            web_results.append({
                "title": str(item.get("title") or url),
                "url": url,
                "description": str(item.get("description") or ""),
                "position": len(web_results) + 1,
            })
            if len(web_results) >= limit:
                break

        logger.info("Marginalia '%s': %d results (limit %d)", query, len(web_results), limit)
        return {"success": True, "data": {"web": web_results}}

    def get_setup_schema(self) -> Dict[str, Any]:
        return {
            "name": "Marginalia (independent index)",
            "badge": "free",
            "tag": "Non-commercial independent index; public shared key, free but rate-limited.",
            "env_vars": [],
        }
