"""凭据解析与逗号分隔多 Key 轮换 — 自 dsh-plugin-multi-search 移植。

Key 值可以写多个,用逗号分隔(``key-a,key-b``);搜索遇到 401/403/429
时自动轮换到下一个候选,全部耗尽才报错。错误摘要只提候选个数,
绝不回显 key 本体。
"""

from __future__ import annotations

from typing import Callable, List, Sequence, TypeVar

T = TypeVar("T")

#: 触发换下一个 key 的 HTTP 状态码
ROTATABLE_STATUSES = frozenset({401, 403, 429})


class HttpStatusError(Exception):
    """带 HTTP 状态码的异常;轮换逻辑只看 ``.status``。"""

    def __init__(self, status: int, message: str) -> None:
        super().__init__(message)
        self.status = status


def split_keys(raw: str | None) -> List[str]:
    """把逗号分隔的 key 串拆成去空白、去空的候选列表(保序)。"""
    return [k.strip() for k in (raw or "").split(",") if k.strip()]


def with_key_rotation(candidates: Sequence[str], run: Callable[[str], T]) -> T:
    """按候选顺序执行 ``run(key)``,401/403/429 轮换下一个,其余错误立即抛出。

    全部耗尽时重抛最后一个错误(消息由调用方包装,不回显 key)。
    """
    if len(candidates) <= 1:
        return run(candidates[0] if candidates else "")
    last: Exception | None = None
    for key in candidates:
        try:
            return run(key)
        except Exception as exc:  # noqa: BLE001 — 轮换语义:按状态决定换/抛
            last = exc
            status = getattr(exc, "status", None)
            if status not in ROTATABLE_STATUSES:
                raise
    raise last  # type: ignore[misc] — candidates 非空时 last 必非 None
