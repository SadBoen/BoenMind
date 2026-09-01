"""Key 覆盖链 — 设置页写入的 key 优先于环境变量,且无需重启进程即生效。

链路:override 文件(设置页写)→ ``get_provider_env``(os.environ → ~/.hermes/.env)。
override 文件由 sidecar(设置页后端)写入,0600;搜索进程只读,
按 mtime 缓存——文件一变,下一次搜索立即用新 key,不需要重启。

文件位置:~/.hermes/plugins/.env-overrides/web-multisearch.json
结构:{"SERPER_API_KEY": "k1,k2", ...}(值为空字符串 = 删除覆盖,回退 env)
"""

from __future__ import annotations

import json
import logging
import os
import threading
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

logger = logging.getLogger(__name__)

#: 本插件管理的 key 源:(展示名, 环境变量名)
KEY_SOURCES: List[Tuple[str, str]] = [
    ("serper", "SERPER_API_KEY"),
    ("jina", "JINA_API_KEY"),
    ("tavily", "TAVILY_API_KEY"),
    ("exa", "EXA_API_KEY"),
    ("brave", "BRAVE_SEARCH_API_KEY"),
    ("langsearch", "LANGSEARCH_API_KEY"),
    ("linkup", "LINKUP_API_KEY"),
    ("you", "YOU_API_KEY"),
    ("websearchapi", "WEBSEARCHAPI_API_KEY"),
]

_ENV_NAMES = [env for _, env in KEY_SOURCES]

_OVERRIDE_PATH = Path(
    os.environ.get("WMS_OVERRIDES_PATH")
    or (Path.home() / ".hermes/plugins/.env-overrides/web-multisearch.json")
)

# (mtime, data) 缓存;None = 尚未加载或文件不存在
_cache_lock = threading.Lock()
_cache: List[Any] = [None]  # [Optional[Tuple[float, Dict[str, str]]]]


def _read_overrides() -> Dict[str, str]:
    """读 override 文件(mtime 缓存);不存在/坏损 → 空覆盖。"""
    try:
        mtime = _OVERRIDE_PATH.stat().st_mtime
    except OSError:
        with _cache_lock:
            _cache[0] = None
        return {}
    with _cache_lock:
        cached = _cache[0]
        if cached is not None and cached[0] == mtime:
            return cached[1]
    try:
        data = json.loads(_OVERRIDE_PATH.read_text(encoding="utf-8"))
        overrides = {
            k: str(v) for k, v in data.items()
            if k in _ENV_NAMES and isinstance(v, str) and v.strip()
        }
    except Exception as exc:  # noqa: BLE001 — 坏损文件不阻断搜索,回退 env
        logger.warning("web-multisearch: bad overrides file %s: %s", _OVERRIDE_PATH, exc)
        overrides = {}
    with _cache_lock:
        _cache[0] = (mtime, overrides)
    return overrides


def provider_env(name: str) -> str:
    """provider 统一入口:override 文件优先,否则回退 Hermes 配置层 env。"""
    ov = _read_overrides().get(name, "")
    if ov.strip():
        return ov.strip()
    try:
        from agent.web_search_provider import get_provider_env

        return get_provider_env(name)
    except Exception:  # noqa: BLE001 — 脱离 hermes 环境(如 sidecar/测试)时兜底
        return os.environ.get(name, "")


# ---------------------------------------------------------------------------
# sidecar(设置页后端)用的写侧
# ---------------------------------------------------------------------------


def get_state() -> Dict[str, Any]:
    """设置页读取:每个 key 源的覆盖状态 + 是否有 env 兜底(回显用,不回显值)。"""
    raw = _OVERRIDE_PATH.read_text(encoding="utf-8") if _OVERRIDE_PATH.exists() else "{}"
    try:
        stored = json.loads(raw)
    except Exception:  # noqa: BLE001
        stored = {}

    def _mask(val: str) -> Optional[Dict[str, Any]]:
        val = (val or "").strip()
        if not val:
            return None
        tail = val.split(",")[-1].strip()  # 多 key 只回显最后一把的尾 4 位
        return {"set": True, "tail": tail[-4:] if len(tail) >= 4 else "***", "multi": "," in val}

    sources = []
    for label, env in KEY_SOURCES:
        override = _mask(str(stored.get(env, "") or ""))
        env_val = ""
        try:
            from agent.web_search_provider import get_provider_env

            env_val = get_provider_env(env)
        except Exception:  # noqa: BLE001
            env_val = os.environ.get(env, "")
        sources.append({
            "name": label,
            "env": env,
            "override": override,          # 覆盖层(None=未设置)
            "env_fallback": bool((env_val or "").strip()),  # env 里是否有兜底可用
            "active": bool(override or (env_val or "").strip()),
        })
    return {"sources": sources, "path": str(_OVERRIDE_PATH)}


def set_key(env: str, value: Optional[str]) -> Dict[str, Any]:
    """写/删一个 key 覆盖。value=None/空串 = 删除覆盖(回退 env)。返回新状态。"""
    if env not in _ENV_NAMES:
        return {"ok": False, "error": f"unknown env: {env}"}
    _OVERRIDE_PATH.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    stored: Dict[str, Any] = {}
    if _OVERRIDE_PATH.exists():
        try:
            stored = json.loads(_OVERRIDE_PATH.read_text(encoding="utf-8"))
        except Exception:  # noqa: BLE001 — 坏损则重建
            stored = {}
    if value and str(value).strip():
        stored[env] = str(value).strip()
    else:
        stored.pop(env, None)
    tmp = _OVERRIDE_PATH.with_suffix(".tmp")
    tmp.write_text(json.dumps(stored, ensure_ascii=False, indent=2), encoding="utf-8")
    os.chmod(tmp, 0o600)
    os.replace(tmp, _OVERRIDE_PATH)
    with _cache_lock:
        _cache[0] = None  # 失效缓存,下次读取即新值
    return {"ok": True, **get_state()}
