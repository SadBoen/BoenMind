"""Serper — Google SERP web search (plugin form).

Subclasses :class:`agent.web_search_provider.WebSearchProvider` (the
plugin-facing ABC), mirroring the bundled ``plugins/web/brave_free``
layout. Serper returns real-time **Google** SERP data (organic results;
news/maps/shopping/images are separate endpoints) — it is search-only,
so pair it with Firecrawl/Tavily/Exa for ``web_extract``.

Config keys this provider responds to::

    web:
      search_backend: "serper"     # explicit per-capability
      backend: "serper"            # shared fallback

Auth env var::

    SERPER_API_KEY=...             # https://serper.dev

Notes:
- Free trial: 2,500 queries on signup, **one-time** (not monthly).
- Paid credits are prepaid packs (Starter $50 / 50k queries) valid for
  6 months — unused balance expires.
- Endpoint: POST https://google.serper.dev/search with ``X-API-KEY`` header.
"""

from __future__ import annotations

import logging
from typing import Any, Dict

from .base import WebSearchProvider

from .keystore import provider_env
from .keys import HttpStatusError, split_keys, with_key_rotation

logger = logging.getLogger(__name__)

_SERPER_ENDPOINT = "https://google.serper.dev/search"


class SerperWebSearchProvider(WebSearchProvider):
    """Search-only provider against Serper's Google SERP API."""

    @property
    def name(self) -> str:
        return "serper"

    @property
    def display_name(self) -> str:
        return "Serper (Google SERP)"

    def is_available(self) -> bool:
        """Return True when ``SERPER_API_KEY`` is set to a non-empty value."""
        return bool(provider_env("SERPER_API_KEY"))

    def supports_search(self) -> bool:
        return True

    def supports_extract(self) -> bool:
        return False

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        """Execute a search against the Serper API.

        Returns ``{"success": True, "data": {"web": [{"title", "url", "description", "position"}]}}``
        on success, or ``{"success": False, "error": str}`` on failure.
        """
        import httpx

        # 支持逗号分隔多 Key:401/403/429 自动轮换下一个候选
        # (override 文件优先于 env,设置页改 key 无需重启)
        candidates = split_keys(provider_env("SERPER_API_KEY"))
        if not candidates:
            return {"success": False, "error": "SERPER_API_KEY is not set"}

        # Serper's `num` is capped at 100 per request.
        num = max(1, min(int(limit), 100))

        def _attempt(key: str):
            try:
                resp = httpx.post(
                    _SERPER_ENDPOINT,
                    json={"q": query, "num": num},
                    headers={
                        "X-API-KEY": key,
                        "Content-Type": "application/json",
                    },
                    timeout=15,
                )
                resp.raise_for_status()
                return resp
            except httpx.HTTPStatusError as exc:
                raise HttpStatusError(
                    exc.response.status_code,
                    f"Serper returned HTTP {exc.response.status_code}",
                ) from exc

        try:
            resp = with_key_rotation(candidates, _attempt)
        except HttpStatusError as exc:
            logger.warning("Serper HTTP error: %s", exc)
            return {"success": False, "error": str(exc)}
        except httpx.RequestError as exc:
            logger.warning("Serper request error: %s", exc)
            return {"success": False, "error": f"Could not reach Serper: {exc}"}

        try:
            data = resp.json()
        except Exception as exc:  # noqa: BLE001
            logger.warning("Serper response parse error: %s", exc)
            return {"success": False, "error": "Could not parse Serper response as JSON"}

        raw_results = data.get("organic") or []
        truncated = raw_results[:limit]

        web_results = [
            {
                "title": str(r.get("title", "")),
                "url": str(r.get("link", "")),
                "description": str(r.get("snippet", "")),
                "position": i + 1,
            }
            for i, r in enumerate(truncated)
        ]

        logger.info(
            "Serper '%s': %d results (from %d raw, limit %d)",
            query,
            len(web_results),
            len(raw_results),
            limit,
        )

        return {"success": True, "data": {"web": web_results}}

    def get_setup_schema(self) -> Dict[str, Any]:
        return {
            "name": "Serper (Google SERP)",
            "badge": "trial",
            "tag": "Real Google SERP results. 2,500 one-time free queries; paid credits valid 6 months.",
            "env_vars": [
                {
                    "key": "SERPER_API_KEY",
                    "prompt": "Serper API key",
                    "url": "https://serper.dev",
                },
            ],
        }
