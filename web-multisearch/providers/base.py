"""WebSearchProvider 基类 — 自包含迁移重建(原 agent.web_search_provider.ABC)。

接口自 hermes-plugin-web-multisearch 的 provider 实现反推:
- ``name`` / ``display_name``:标识与展示名;
- ``is_available()``:凭据/配置是否就绪(聚合器据此过滤源);
- ``supports_search()`` / ``supports_extract()``:能力位;
- ``search(query, limit)``:返回 ``{"success": bool, "data": {"web": [
  {"title", "url", "description", ...}]}}``,失败返回 ``{"success": False,
  "error": str}``,约定**不抛异常**;
- ``extract(urls)``:正文抓取(可选,聚合器仅用 jina reader 兜底)。
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Any, Dict, List


class WebSearchProvider(ABC):
    """搜索 provider 抽象基类。"""

    @property
    @abstractmethod
    def name(self) -> str:
        """源标识(小写,用于聚合标注与配置)。"""

    @property
    def display_name(self) -> str:
        return self.name

    def is_available(self) -> bool:
        return True

    def supports_search(self) -> bool:
        return True

    def supports_extract(self) -> bool:
        return False

    @abstractmethod
    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        """执行搜索;返回标准结构,约定不抛异常。"""

    def extract(self, urls: List[str], **kwargs: Any) -> Any:
        raise NotImplementedError(f"{self.name}: extract not supported")
