"""Jina (Search Foundation API) — web search + page extraction (plugin form).

Subclasses :class:`agent.web_search_provider.WebSearchProvider`, combining
two Jina products under one key:

- **Search** (``s.jina.ai``): LLM-friendly markdown of search results.
- **Reader** (``r.jina.ai``): converts any URL into clean markdown, which
  services ``web_extract``.

Free tier: every new key gets **10M free tokens** across all Jina APIs
(Reader/Search/Embeddings/Reranker share the same quota); free rate limit
is 100 RPM shared account-wide. Note: Jina search quality trails a native
Google SERP on hard queries, and the free tier is known to hit 429s.

Config keys this provider responds to::

    web:
      search_backend: "jina"       # explicit per-capability
      extract_backend: "jina"      # explicit per-capability
      backend: "jina"              # shared fallback

Auth env var::

    JINA_API_KEY=...               # https://jina.ai
"""

from __future__ import annotations

import logging
import re
from typing import Any, Dict, List

from .base import WebSearchProvider

from .keystore import provider_env
from .keys import HttpStatusError, split_keys, with_key_rotation

logger = logging.getLogger(__name__)

_SEARCH_ENDPOINT = "https://s.jina.ai"
_READER_ENDPOINT = "https://r.jina.ai"

# Match [title](url) markdown links.
_LINK_RE = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")
# Reader metadata lines, e.g. "Title: <page title>".
_TITLE_LINE_RE = re.compile(r"^Title:\s*(.+)$", re.IGNORECASE | re.MULTILINE)


def _parse_search_markdown(text: str, limit: int) -> List[Dict[str, Any]]:
    """Best-effort parser for Jina Search's markdown output.

    Handles two shapes seen in the wild:
      1. ``### Title`` followed by a bare URL line, then description lines.
      2. ``[Title](url)`` link lines, with following prose as description.
    Skips non-result preamble (headings, Jina boilerplate) — position is
    assigned in encounter order, which matches the returned ranking.
    """
    results: List[Dict[str, Any]] = []
    lines = [ln.strip() for ln in text.splitlines()]

    def _is_result_start(ln: str) -> bool:
        return bool(_LINK_RE.search(ln)) or ln.startswith("###")

    i = 0
    while i < len(lines) and len(results) < limit:
        line = lines[i]
        if not line:
            i += 1
            continue

        m = _LINK_RE.search(line)
        if m:
            title = m.group(1).strip() or line
            url = m.group(2).strip()
        elif line.startswith("###"):
            title = line.lstrip("# ").strip()
            # Next non-empty line is usually the bare URL.
            j = i + 1
            while j < len(lines) and not lines[j]:
                j += 1
            url = lines[j] if j < len(lines) and not _is_result_start(lines[j]) else ""
            i = j
        else:
            i += 1
            continue

        # Collect following prose until the next result marker.
        desc_parts: List[str] = []
        j = i + 1
        while j < len(lines) and lines[j] and not _is_result_start(lines[j]):
            desc_parts.append(lines[j])
            j += 1
        i = j

        if not url:
            continue
        results.append(
            {
                "title": title,
                "url": url,
                "description": " ".join(desc_parts)[:500],
                "position": len(results) + 1,
            }
        )

    return results


def _extract_page_title(markdown: str, url: str) -> str:
    """Pull a title from Reader output (``Title:`` line, else first heading)."""
    m = _TITLE_LINE_RE.search(markdown)
    if m:
        return m.group(1).strip()
    for ln in markdown.splitlines():
        if ln.startswith("# "):
            return ln.lstrip("# ").strip()
    return url


class JinaWebSearchProvider(WebSearchProvider):
    """Search + extract provider built on the Jina Search Foundation API."""

    @property
    def name(self) -> str:
        return "jina"

    @property
    def display_name(self) -> str:
        return "Jina (Search + Reader)"

    def is_available(self) -> bool:
        """Return True when ``JINA_API_KEY`` is set to a non-empty value."""
        return bool(provider_env("JINA_API_KEY"))

    def supports_search(self) -> bool:
        return True

    def supports_extract(self) -> bool:
        return True

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        """Search via ``s.jina.ai``; returns markdown parsed into the
        standard ``{"success": True, "data": {"web": [...]}}`` envelope."""
        import httpx

        # 支持逗号分隔多 Key:401/403/429 自动轮换下一个候选
        # (override 文件优先于 env,设置页改 key 无需重启)
        candidates = split_keys(provider_env("JINA_API_KEY"))
        if not candidates:
            return {"success": False, "error": "JINA_API_KEY is not set"}

        def _attempt(key: str):
            try:
                resp = httpx.get(
                    _SEARCH_ENDPOINT,
                    params={"q": query},
                    headers={
                        "Authorization": f"Bearer {key}",
                        "Accept": "text/markdown",
                    },
                    timeout=30,
                )
                resp.raise_for_status()
                return resp
            except httpx.HTTPStatusError as exc:
                raise HttpStatusError(
                    exc.response.status_code,
                    f"Jina Search returned HTTP {exc.response.status_code}",
                ) from exc

        try:
            resp = with_key_rotation(candidates, _attempt)
        except HttpStatusError as exc:
            logger.warning("Jina Search HTTP error: %s", exc)
            return {"success": False, "error": str(exc)}
        except httpx.RequestError as exc:
            logger.warning("Jina Search request error: %s", exc)
            return {"success": False, "error": f"Could not reach Jina Search: {exc}"}

        web_results = _parse_search_markdown(resp.text, limit)
        logger.info(
            "Jina Search '%s': %d results (limit %d)",
            query,
            len(web_results),
            limit,
        )
        return {"success": True, "data": {"web": web_results}}

    def extract(self, urls: List[str], **kwargs: Any) -> Any:
        """Extract page content via ``r.jina.ai`` Reader (URL → markdown).

        Returns a list of per-URL result dicts; a failing URL carries an
        ``error`` field instead of failing the whole batch. When the key is
        missing entirely, returns ``{"success": False, "error": ...}``.
        """
        import httpx

        # 支持逗号分隔多 Key:401/403/429 自动轮换下一个候选
        # (override 文件优先于 env,设置页改 key 无需重启)
        candidates = split_keys(provider_env("JINA_API_KEY"))
        if not candidates:
            return {"success": False, "error": "JINA_API_KEY is not set"}

        results: List[Dict[str, Any]] = []
        for url in urls:
            target = f"{_READER_ENDPOINT}/{url}"

            def _attempt(key: str, _target: str = target):
                try:
                    resp = httpx.get(
                        _target,
                        headers={
                            "Authorization": f"Bearer {key}",
                            "Accept": "text/markdown",
                        },
                        timeout=30,
                    )
                    resp.raise_for_status()
                    return resp
                except httpx.HTTPStatusError as exc:
                    raise HttpStatusError(
                        exc.response.status_code,
                        f"Jina Reader returned HTTP {exc.response.status_code}",
                    ) from exc

            try:
                resp = with_key_rotation(candidates, _attempt)
                md = resp.text
                results.append(
                    {
                        "url": url,
                        "title": _extract_page_title(md, url),
                        "content": md,
                        "raw_content": md,
                        "metadata": {},
                    }
                )
            except HttpStatusError as exc:
                logger.warning("Jina Reader HTTP error for %s: %s", url, exc)
                results.append({"url": url, "error": str(exc)})
            except httpx.RequestError as exc:
                logger.warning("Jina Reader request error for %s: %s", url, exc)
                results.append({"url": url, "error": f"Could not reach Jina Reader: {exc}"})

        return results

    def get_setup_schema(self) -> Dict[str, Any]:
        return {
            "name": "Jina (Search + Reader)",
            "badge": "free",
            "tag": "s.jina.ai search + r.jina.ai Reader extraction. 10M free tokens, 100 RPM free tier.",
            "env_vars": [
                {
                    "key": "JINA_API_KEY",
                    "prompt": "Jina API key",
                    "url": "https://jina.ai",
                },
            ],
        }
