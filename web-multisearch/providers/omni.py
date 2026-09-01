"""Web-Multisearch aggregator — 全网搜:并行调用所有已配置搜索源,RRF 融合排序。

自 hermes-plugin-web-multisearch 的 providers/omni.py 迁移(BoenMind MCP 版)。
自包含化改造:
- 基类改 providers/base.py(原 agent.web_search_provider);
- 内置源改 providers/builtin.py 自包含实现(原 import plugins.web.*);
- extract 简化为 jina Reader 兜底(firecrawl 不迁,v0);
- 用量打点(usage)不迁,搜索主链路不受影响。

保留的核心资产(原样):
- RRF 融合排序(Σ1/(60+rank))、CJK 同题镜像合并(bigram+Jaccard≥0.9);
- 逗号分隔多 Key,401/403/429 自动轮换;
- meta 源耗时遥测;description 前缀 [来源] 标注。
"""

from __future__ import annotations

import json
import logging
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from typing import Any, Dict, List, Optional, Tuple

from .base import WebSearchProvider
from .builtin import (
    BraveFreeWebSearchProvider,
    DDGSWebSearchProvider,
    ExaWebSearchProvider,
    SearxngWebSearchProvider,
    TavilyWebSearchProvider,
)
from .extra import (
    LangSearchWebSearchProvider,
    LinkupWebSearchProvider,
    WebSearchApiProvider,
    YouWebSearchProvider,
)
from .fusion import merge_mirrors, rrf_fuse
from .jina import JinaWebSearchProvider
from .marginalia import MarginaliaWebSearchProvider
from .serper import SerperWebSearchProvider

logger = logging.getLogger(__name__)

# 整体等待上限:各源内部已有 12-30s 超时,这里兜底,超时源直接丢弃
_GLOBAL_TIMEOUT = 25.0


# ---------------------------------------------------------------------------
# 源装配
# ---------------------------------------------------------------------------


def _build_builtin_sources() -> Dict[str, WebSearchProvider]:
    """自包含内置源(原 plugins.web.* 的迁移重建版)。"""
    return {
        "searxng": SearxngWebSearchProvider(),
        "ddgs": DDGSWebSearchProvider(),
        "tavily": TavilyWebSearchProvider(),
        "exa": ExaWebSearchProvider(),
        "brave": BraveFreeWebSearchProvider(),
    }


def _build_user_sources() -> Dict[str, WebSearchProvider]:
    """本插件自带的搜索源(原样迁移)。"""
    return {
        "serper": SerperWebSearchProvider(),
        "jina": JinaWebSearchProvider(),
        "marginalia": MarginaliaWebSearchProvider(),
        "langsearch": LangSearchWebSearchProvider(),
        "linkup": LinkupWebSearchProvider(),
        "you": YouWebSearchProvider(),
        "websearchapi": WebSearchApiProvider(),
    }


def _all_sources() -> Dict[str, WebSearchProvider]:
    sources: Dict[str, WebSearchProvider] = {}
    try:
        sources.update(_build_builtin_sources())
    except Exception as exc:  # noqa: BLE001 — 内置模块加载失败不应拖垮整个插件
        logger.warning("web-multisearch: builtin source import failed: %s", exc)
    try:
        sources.update(_build_user_sources())
    except Exception as exc:  # noqa: BLE001
        logger.warning("web-multisearch: user source import failed: %s", exc)
    return sources


# ---------------------------------------------------------------------------
# 输出组装
# ---------------------------------------------------------------------------


def _annotate(items: List[Dict[str, Any]], limit: int) -> List[Dict[str, Any]]:
    """输出标准结构,description 前缀标注来源集合,如 ``[ddgs|jina|searxng]``。"""
    out: List[Dict[str, Any]] = []
    for i, m in enumerate(items[:limit]):
        srcs = "|".join(sorted(m["sources"]))
        prefix = f"[{srcs}] "
        title = m.get("title", "")
        desc = m.get("description", "")
        if desc:
            desc = prefix + desc
        elif title:
            title = prefix + title
        else:
            title = prefix.rstrip()
        out.append({
            "title": title,
            "url": m["url"],
            "description": desc,
            "position": i + 1,
        })
    return out


# ---------------------------------------------------------------------------
# 聚合器
# ---------------------------------------------------------------------------


class _AggregatorBase(WebSearchProvider):
    """聚合器基类:source_names 决定聚合哪些源。"""

    source_names: Tuple[str, ...] = ()
    display_label = "Web-Multisearch aggregator"

    def __init__(self) -> None:
        self._sources: Optional[Dict[str, WebSearchProvider]] = None

    @property
    def display_name(self) -> str:
        return self.display_label

    def _load_sources(self) -> Dict[str, WebSearchProvider]:
        if self._sources is None:
            all_src = _all_sources()
            self._sources = {
                n: p for n, p in all_src.items() if n in self.source_names
            }
        return self._sources

    def is_available(self) -> bool:
        return any(
            p.is_available() for p in self._load_sources().values()
        )

    def supports_search(self) -> bool:
        return True

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        sources = {
            n: p for n, p in self._load_sources().items() if p.is_available()
        }
        if not sources:
            return {
                "success": False,
                "error": (
                    f"{self.name}: no source available "
                    f"(configured sources: {', '.join(self.source_names)}; "
                    "check API keys and SearXNG URL)"
                ),
            }

        # 每源取 limit+2 条:RRF 融合去重后截回 limit
        per_source_limit = limit + 2
        per_source: List[Tuple[str, List[Dict[str, Any]]]] = []
        timings: Dict[str, int] = {}
        failed: List[Tuple[str, str]] = []

        ex = ThreadPoolExecutor(max_workers=min(len(sources), 8))
        try:
            futs = {
                ex.submit(self._timed_search, p, query, per_source_limit): n
                for n, p in sources.items()
            }
            try:
                for fut in as_completed(futs, timeout=_GLOBAL_TIMEOUT):
                    n = futs[fut]
                    try:
                        ms, res = fut.result()
                        timings[n] = ms
                        web = ((res or {}).get("data") or {}).get("web") or []
                        if web:
                            per_source.append((n, list(web)))
                        else:
                            failed.append((n, (res or {}).get("error") or "no results"))
                    except Exception as exc:  # noqa: BLE001 — 单源失败不影响整体
                        failed.append((n, str(exc)))
                        logger.warning("web-multisearch: source %s failed: %s", n, exc)
            except TimeoutError:
                logger.warning(
                    "web-multisearch: global timeout %.0fs hit, dropping slow sources",
                    _GLOBAL_TIMEOUT,
                )
                for n, fut in futs.items():
                    if n not in timings:
                        failed.append((n, "global timeout"))
                    fut.cancel()
        finally:
            ex.shutdown(wait=False, cancel_futures=True)

        if not per_source:
            return {
                "success": False,
                "error": f"{self.name}: all sources failed: {failed}",
            }

        # RRF 融合 → 同题镜像合并 → 截断输出
        fused = rrf_fuse(per_source)
        fused = merge_mirrors(fused)
        web = _annotate(fused, limit)
        logger.info(
            "web-multisearch '%s': %d sources ok, %d failed, %d unique -> %d out",
            query, len(per_source), len(failed), len(fused), len(web),
        )
        return {
            "success": True,
            "data": {"web": web},
            "meta": {
                "mode": self.name,
                "sources_ok": sorted(n for n, _ in per_source),
                "sources_failed": [n for n, _ in failed],
                "timings_ms": {n: timings[n] for n in sorted(timings)},
                "unique": len(fused),
            },
        }

    @staticmethod
    def _timed_search(provider: WebSearchProvider, query: str, limit: int) -> Tuple[int, Dict[str, Any]]:
        """跑单源搜索并计时;(耗时 ms, 结果)。"""
        started = time.monotonic()
        try:
            res = provider.search(query, limit) or {}
        except Exception as exc:  # noqa: BLE001 — 异常也算该源一次失败调用
            res = {"success": False, "error": str(exc)}
        ms = int((time.monotonic() - started) * 1000)
        return ms, res

    # -- extract:jina Reader 兜底(firecrawl 不迁,v0)----------------------

    def supports_extract(self) -> bool:
        return True

    def extract(self, urls: List[str], **kwargs: Any) -> Any:
        jina = self._load_sources().get("jina")
        if jina is None or not jina.is_available():
            return {"success": False, "error": "web-multisearch: no extract source available (set JINA_API_KEY)"}
        try:
            j_res = jina.extract(urls)
            if isinstance(j_res, list):
                return j_res
            return {"success": False, "error": "web-multisearch: jina extract returned non-list"}
        except Exception as exc:  # noqa: BLE001
            return {"success": False, "error": f"web-multisearch extract failed: {exc}"}

    def get_setup_schema(self) -> Dict[str, Any]:
        return {
            "name": self.display_label,
            "badge": "aggregator",
            "tag": (
                f"Sources: {', '.join(self.source_names)}. "
                "Parallel search, RRF fusion, mirror dedup, cross-source annotation."
            ),
            "env_vars": [],
        }


class LiteWebSearchProvider(_AggregatorBase):
    """日常组合:searxng + ddgs + jina + marginalia(全免费源)。"""

    @property
    def name(self) -> str:
        return "web-multisearch-lite"

    source_names = ("searxng", "ddgs", "jina", "marginalia")
    display_label = "Web-Multisearch Lite (searxng + ddgs + jina + marginalia)"


class OmniWebSearchProvider(_AggregatorBase):
    """全网搜:免费四源 + serper + tavily + exa + brave + langsearch/linkup/you/websearchapi。"""

    @property
    def name(self) -> str:
        return "web-multisearch"

    source_names = ("searxng", "ddgs", "jina", "marginalia", "serper", "tavily",
                    "exa", "brave", "langsearch", "linkup", "you", "websearchapi")
    display_label = "Web-Multisearch 全源 (12 sources)"


def format_result(result: Dict[str, Any]) -> str:
    """把聚合结果序列化为 web_search 同款 JSON 字符串(供 MCP 工具用)。"""
    return json.dumps(result, ensure_ascii=False, indent=2)
