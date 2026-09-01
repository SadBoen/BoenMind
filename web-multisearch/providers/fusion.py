"""排序融合与去重 — 自 dsh-plugin-multi-search 移植。

内容:
- ``normalize_url``       : URL 规范化(去跟踪参数/www/尾斜杠,自 hermes 版保留)
- ``rrf_fuse``            : Reciprocal Rank Fusion,按 Σ1/(k+rank) 跨源融合排序
- ``merge_mirrors``       : 同题镜像合并(CJK 感知 token 化,Jaccard ≥ 0.9)

与旧版(命中源数 + 最靠前位置)排序的差异:RRF 同时考虑"被几个源命中"
和"在每个源里排多前",多源命中且各源排名靠前的结果真正浮顶,而不是
简单的计数排序。
"""

from __future__ import annotations

import re
from typing import Any, Dict, List, Set, Tuple
from urllib.parse import parse_qsl, urlencode, urlparse, urlunparse

# 常见跟踪参数:规范化 URL 时丢弃(同一页面不再被算成多条)
TRACKING_PARAMS = {
    "utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content",
    "fbclid", "gclid", "mc_cid", "mc_eid", "yclid", "igshid", "ref", "ref_src",
    "spm", "from", "source", "wfr", "_ga",
}

_CJK_WORD_RE = re.compile(r"[a-z0-9]+")
_RRF_K = 60
_MIRROR_THRESHOLD = 0.9


def normalize_url(url: str) -> str:
    """规范化 URL 用于去重:小写 host、去 www、去尾斜杠、去 hash、丢跟踪参数。"""
    try:
        raw = (url or "").strip()
        p = urlparse(raw)
        if not p.scheme or not p.netloc:
            return raw
        host = p.netloc.lower()
        if host.startswith("www."):
            host = host[4:]
        path = p.path.rstrip("/") or "/"
        keep = [
            (k, v) for k, v in parse_qsl(p.query, keep_blank_values=True)
            if k.lower() not in TRACKING_PARAMS
        ]
        keep.sort()
        query = urlencode(keep) if keep else ""
        return urlunparse((p.scheme, host, path, "", query, ""))
    except Exception:  # noqa: BLE001 — 任何解析失败都退回原值
        return (url or "").strip()


def rrf_fuse(per_source: List[Tuple[str, List[Dict[str, Any]]]], k: int = _RRF_K) -> List[Dict[str, Any]]:
    """Reciprocal Rank Fusion:把各源的有序结果列表融合成一个排序。

    URL 相同的条目合并:贡献源并入 ``sources`` 集合,保留更完整的
    title/description;得分 = Σ 1/(k + rank)。排序:得分降序,平手时
    命中源数降序(交叉验证强度)。

    :param per_source: ``[(源名, [结果 dict, ...]), ...]``,结果需含 ``url``。
    :param k: RRF 平滑常数(dsh 版同款 60)。
    """
    groups: Dict[str, Dict[str, Any]] = {}
    for src, results in per_source:
        rank = 0
        for r in results:
            url = (r.get("url") or "").strip()
            if not url:
                continue
            key = normalize_url(url)
            if not key:
                continue
            rank += 1
            g = groups.get(key)
            if g is None:
                g = {
                    "url": url,
                    "title": r.get("title") or "",
                    "description": r.get("description") or "",
                    "sources": set(),
                    "score": 0.0,
                    "hits": 0,
                }
                groups[key] = g
            g["hits"] += 1
            g["score"] += 1.0 / (k + rank)
            g["sources"].add(src)
            if len(r.get("title") or "") > len(g["title"]):
                g["title"] = r.get("title") or ""
            if len((r.get("description") or "")) > len(g["description"]):
                g["description"] = r.get("description") or ""
    return sorted(groups.values(), key=lambda g: (-g["score"], -g["hits"]))


def _is_cjk(ch: str) -> bool:
    if not ch:
        return False
    code = ord(ch)
    return 0x4E00 <= code <= 0x9FFF or 0x3400 <= code <= 0x4DBF


def tokenize_title(title: str) -> Set[str]:
    """标题 token 化:CJK 连续段切成相邻二字 bigram,拉丁/数字切成小写词。"""
    tokens: Set[str] = set()
    cleaned = (title or "").lower()
    chars = list(cleaned)
    for i, ch in enumerate(chars):
        nxt = chars[i + 1] if i + 1 < len(chars) else ""
        if _is_cjk(ch) and _is_cjk(nxt):
            tokens.add(ch + nxt)
    for word in _CJK_WORD_RE.findall(cleaned):
        tokens.add(word)
    return tokens


def _jaccard(a: Set[str], b: Set[str]) -> float:
    if not a and not b:
        return 0.0
    inter = len(a & b)
    return inter / (len(a) + len(b) - inter)


def merge_mirrors(items: List[Dict[str, Any]], threshold: float = _MIRROR_THRESHOLD) -> List[Dict[str, Any]]:
    """合并同题镜像:转载站点常用近似标题转发同一内容。

    仅当两个标题都至少有 2 个 token 时才参与比较(单 token 标题多为
    "Home"/"404" 这类泛化词,误合率高);合并时来源集合并集、补齐空缺的
    title/description。
    """
    kept: List[Dict[str, Any]] = []
    token_sets: List[Set[str]] = []
    for it in items:
        toks = tokenize_title(it.get("title") or "")
        mirror_idx = -1
        if len(toks) >= 2:
            for i, ts in enumerate(token_sets):
                if len(ts) >= 2 and _jaccard(ts, toks) >= threshold:
                    mirror_idx = i
                    break
        if mirror_idx >= 0:
            orig = kept[mirror_idx]
            orig["sources"] |= it.get("sources", set())
            if not orig.get("title"):
                orig["title"] = it.get("title", "")
            if not orig.get("description"):
                orig["description"] = it.get("description", "")
            continue
        kept.append(it)
        token_sets.append(toks)
    return kept
