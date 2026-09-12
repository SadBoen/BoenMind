#!/usr/bin/env python3
"""生成 adr/README.md 的 ADR 索引表(单一来源 = 各 ADR 的 front-matter 与正文)。

「能生成的不手写,同一事实只有一处」(ADR-0040):索引若手维护必与 ADR 漂移。
本脚本从每个 ADR 的 front-matter(status/date/supersedes/superseded_by/summary)、
标题行与「条件与验收」段推导整张表,写入 adr/README.md 的标记区。

用法:
  python scripts/gen_adr_index.py           # 就地写回
  python scripts/gen_adr_index.py --check    # 只校验是否最新(CI 用;有 diff 即失败)
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ADR_DIR = ROOT / "adr"
README = ADR_DIR / "README.md"
BEGIN = "<!-- ADR-INDEX:BEGIN 由 scripts/gen_adr_index.py 生成,勿手改 -->"
END = "<!-- ADR-INDEX:END -->"

ID_RE = re.compile(r"^(ADR-\d{4})")
HEADING_RE = re.compile(r"^#\s*(ADR-\d{4})\s*[:：]?\s*(.*)$")


def parse_front_matter(text: str) -> dict[str, object]:
    fm: dict[str, object] = {}
    if not text.startswith("---\n"):
        return fm
    end = text.find("\n---", 3)
    for raw in text[4:end].splitlines():
        line = raw.strip()
        if not line or ":" not in line:
            continue
        k, _, v = line.partition(":")
        k, v = k.strip(), v.strip()
        if v.startswith("[") and v.endswith("]"):
            fm[k] = [x.strip() for x in v[1:-1].split(",") if x.strip()]
        else:
            fm[k] = v
    return fm


def title_of(text: str) -> str:
    for line in text.splitlines():
        m = HEADING_RE.match(line)
        if m:
            return m.group(2).strip() or m.group(1)
    return ""


def has_conditions(text: str) -> bool:
    """沿革状态表注:「条件与验收」段是否有实际条件(否则不加 -with-conditions)。"""
    i = text.find("## 条件与验收")
    if i == -1:
        return False
    rest = text[i + len("## 条件与验收") :]
    nxt = rest.find("\n## ")
    body = rest[: nxt if nxt != -1 else len(rest)]
    for line in body.splitlines():
        s = line.strip()
        if s.startswith("- ") and not s.startswith("- (无"):
            return True
    return False


def status_cell(fm: dict[str, object], text: str) -> str:
    st = fm.get("status")
    if st == "superseded":
        by = fm.get("superseded_by") or []
        return "superseded→" + "、".join(by) if by else "superseded"
    if st == "accepted" and has_conditions(text):
        return "accepted-with-conditions"
    return str(st)


def is_archive(path: Path) -> bool:
    return "archive" in path.parts


def build_row(path: Path) -> str:
    text = path.read_text(encoding="utf-8")
    fm = parse_front_matter(text)
    aid = ID_RE.match(path.name).group(1)
    rel = path.relative_to(ADR_DIR).as_posix()
    title = title_of(text)
    summary = str(fm.get("summary", "")).strip()
    return f"| [{aid}]({rel}) | {title} | {status_cell(fm, text)} | {summary} |"


def ordered_ads() -> list[Path]:
    files = []
    for p in ADR_DIR.glob("ADR-*.md"):
        files.append(p)
    for p in (ADR_DIR / "archive").glob("ADR-*.md"):
        files.append(p)
    return sorted(files, key=lambda p: ID_RE.match(p.name).group(1))


def render_table() -> str:
    head = ["| ADR | 标题 | 状态 | 一句话决策 |", "|---|---|---|---|"]
    rows = [build_row(p) for p in ordered_ads()]
    return "\n".join(head + rows)


def render_readme(text: str) -> str:
    if BEGIN not in text or END not in text:
        raise SystemExit("adr/README.md 缺 ADR-INDEX 标记区")
    pre = text[: text.index(BEGIN) + len(BEGIN)]
    post = text[text.index(END) :]
    return pre + "\n" + render_table() + "\n" + post


def main() -> int:
    check = "--check" in sys.argv
    current = README.read_text(encoding="utf-8")
    fresh = render_readme(current)
    if current == fresh:
        print("ADR 索引: 已是最新")
        return 0
    if check:
        print("ADR 索引过期——请运行 python scripts/gen_adr_index.py", file=sys.stderr)
        return 1
    README.write_text(fresh, encoding="utf-8", newline="\n")
    print("ADR 索引: 已重新生成")
    return 0


if __name__ == "__main__":
    sys.exit(main())
