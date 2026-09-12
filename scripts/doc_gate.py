#!/usr/bin/env python3
"""doc-gate:文档治理门(ADR 状态机 + 取代双向一致 + 归档一致 + 白名单 + 治理自检)。

为什么要这道门:此前的四套文档治理(ADR-0015 / 0026 / 0027 及 2026-09-09 的
Issues 改制)都是"写在纸上的规则",没有一条被机器执行——于是每套都在数日内
被下一套推翻,治理本身成了最大的一份历史叙事。可机器验证的部分必须固化为 CI
硬门,规则才不再依赖自觉。

检查项(任一失败即 exit 1):
  A. ADR front-matter 合法(必填 status/date;status 在词表内;date 形如 YYYY-MM-DD)。
  B. status=superseded 必带 superseded_by;其余状态不得带。
  C. 取代关系双向一致(A.superseded_by=B ⇔ B.supersedes∋A,且 B 存在)。
  D. 归档一致:superseded/withdrawn 必须在 adr/archive/;proposed/accepted 不得在归档区。
  E. 白名单:所有"仓库可提交面"内的 .md 必须命中 docs/doc-whitelist.txt;空条目告警。
  F. 治理自检:ADR-0040 在位且 accepted;ci.yml 仍含 doc-gate job(门不可被静默摘除)。
  G. 文档预算:总量与单文件字节上限;超限须显式上调常量(增长=显式决策)。
  H. 索引新鲜度:adr/README.md 的索引表与 ADR front-matter 一致(由 gen_adr_index.py 生成,禁止手改漂移)。

提示(不阻断,仅计数):ADR 正文中的叙事标记,供后续按 ADR-0027 逐批清理。
"""

from __future__ import annotations

import fnmatch
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ADR_DIR = ROOT / "adr"
ARCHIVE_DIR = ADR_DIR / "archive"
WHITELIST = ROOT / "docs" / "doc-whitelist.txt"
CI_FILE = ROOT / ".github" / "workflows" / "ci.yml"
GOVERNANCE_ADR = "ADR-0040"

# 文档预算(ADR-0040):总量与单文件上限。超限即失败,须显式上调常量并在提交说明
# 里给出理由——把"文档增长"从无意识堆积变成一次显式决策。
TOTAL_BUDGET_BYTES = 430_000
MAX_FILE_BYTES = 100_000

STATUS_VOCAB = {"proposed", "accepted", "superseded", "withdrawn"}
TERMINAL = {"superseded", "withdrawn"}

DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
ADR_ID_RE = re.compile(r"^(ADR-\d{4})")
FM_KEYS = ("status", "date", "supersedes", "superseded_by")

# 叙事标记(仅计数提示;命中不代表失败)
NARRATIVE_MARKERS = ("共识", "未决分歧", "辩论", "Zen consensus", "交付记录")

errors: list[str] = []
warns: list[str] = []


def err(msg: str) -> None:
    errors.append(msg)


def warn(msg: str) -> None:
    warns.append(msg)


def rel(p: Path) -> str:
    return p.relative_to(ROOT).as_posix()


def parse_front_matter(text: str) -> dict[str, object] | None:
    """解析文件头部的简单 front-matter;非 front-matter 起头返回 None。"""
    if not text.startswith("---\n"):
        return None
    end = text.find("\n---", 3)
    if end == -1:
        return None
    block = text[4:end]
    fm: dict[str, object] = {}
    for raw in block.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if ":" not in line:
            return None
        key, _, value = line.partition(":")
        key, value = key.strip(), value.strip()
        if value.startswith("[") and value.endswith("]"):
            items = [v.strip() for v in value[1:-1].split(",")]
            fm[key] = [v for v in items if v]
        else:
            fm[key] = value
    return fm


def adr_id_of(path: Path) -> str | None:
    m = ADR_ID_RE.match(path.name)
    return m.group(1) if m else None


def collect_adrs() -> dict[str, Path]:
    """收集全部 ADR(含归档区),返回 id -> 路径。重号即失败。"""
    found: dict[str, Path] = {}
    for path in sorted(ADR_DIR.rglob("ADR-*.md")):
        aid = adr_id_of(path)
        if aid is None:
            err(f"{rel(path)}: 文件名不含合法 ADR 编号(ADR-NNNN)")
            continue
        if aid in found:
            err(f"ADR 编号重号:{aid}({rel(found[aid])} 与 {rel(path)})")
        found[aid] = path
    return found


def check_front_matter(adrs: dict[str, Path]) -> dict[str, dict[str, object]]:
    """检查 A/B/D,返回 id -> front-matter。"""
    metas: dict[str, dict[str, object]] = {}
    for aid, path in adrs.items():
        text = path.read_text(encoding="utf-8")
        fm = parse_front_matter(text)
        if fm is None:
            err(f"{rel(path)}: 缺 front-matter(status/date 等机器字段)")
            continue
        metas[aid] = fm

        for key in FM_KEYS:
            if key not in fm:
                err(f"{rel(path)}: front-matter 缺字段 `{key}`")

        status = fm.get("status")
        if status not in STATUS_VOCAB:
            err(f"{rel(path)}: status=`{status}` 不在词表 {sorted(STATUS_VOCAB)}")

        date = fm.get("date")
        if not isinstance(date, str) or not DATE_RE.match(date):
            err(f"{rel(path)}: date=`{date}` 非 YYYY-MM-DD")

        sup_by = fm.get("superseded_by")
        if not isinstance(sup_by, list):
            err(f"{rel(path)}: superseded_by 必须是列表(空用 [])")
            sup_by = []
        if status == "superseded" and not sup_by:
            err(f"{rel(path)}: status=superseded 必须带 superseded_by")
        if status != "superseded" and sup_by:
            err(f"{rel(path)}: 非 superseded 状态不得带 superseded_by")

        in_archive = ARCHIVE_DIR in path.parents
        if status in TERMINAL and not in_archive:
            err(f"{rel(path)}: 终态({status})必须归档到 adr/archive/")
        if status not in TERMINAL and in_archive:
            err(f"{rel(path)}: 非终态({status})不得放在 adr/archive/")

        for marker in NARRATIVE_MARKERS:
            hits = text.count(marker)
            if hits:
                warn(f"叙事标记 `{marker}`×{hits}: {rel(path)}")
    return metas


def check_bidirectional(metas: dict[str, dict[str, object]]) -> None:
    """检查 C:取代关系双向一致。"""
    for aid, fm in metas.items():
        for target in fm.get("supersedes") or []:
            if target not in metas:
                err(f"{aid}.supersedes 指向不存在的 {target}")
                continue
            back = metas[target].get("superseded_by") or []
            if aid not in back:
                err(f"{aid}.supersedes={target} 但 {target}.superseded_by 未回指 {aid}")
        for target in fm.get("superseded_by") or []:
            if target not in metas:
                err(f"{aid}.superseded_by 指向不存在的 {target}")
                continue
            fwd = metas[target].get("supersedes") or []
            if aid not in fwd:
                err(f"{aid}.superseded_by={target} 但 {target}.supersedes 未回指 {aid}")


def repo_md_files() -> list[str]:
    """仓库"可提交面"内的 .md(受追踪 + 未忽略的未追踪;不含被忽略的 .work/.tools/.zcode)。"""
    out = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "-co", "--exclude-standard", "--", "*.md"],
        capture_output=True,
        text=True,
        check=False,
    )
    if out.returncode != 0:
        err("git ls-files 执行失败,无法枚举 .md(白名单检查跳过)")
        return []
    return sorted(p for p in out.stdout.splitlines() if (ROOT / p).exists())


def load_whitelist() -> list[str]:
    if not WHITELIST.exists():
        err(f"缺白名单文件 {rel(WHITELIST)}")
        return []
    entries = []
    for raw in WHITELIST.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line and not line.startswith("#"):
            entries.append(line)
    return entries


def match(entry: str, path: str) -> bool:
    if entry.endswith("/**"):
        return path.startswith(entry[:-3].rstrip("/") + "/")
    return fnmatch.fnmatch(path, entry)


def check_whitelist() -> None:
    """检查 E:可提交面 .md 必须命中白名单;空悬条目告警。"""
    entries = load_whitelist()
    if not entries:
        return
    files = repo_md_files()
    for path in files:
        if not any(match(e, path) for e in entries):
            err(f"白名单外的 .md:{path}(新增文档须先登 docs/doc-whitelist.txt)")
    for entry in entries:
        if not any(match(entry, p) for p in files):
            warn(f"白名单条目未命中任何文件(可删):{entry}")


def check_governance_selfcheck(metas: dict[str, dict[str, object]]) -> None:
    """检查 F:治理方案不可被静默摘除。"""
    if GOVERNANCE_ADR not in metas:
        err(f"缺治理 ADR({GOVERNANCE_ADR}),治理门失去自身锚点")
    else:
        status = metas[GOVERNANCE_ADR].get("status")
        if status != "accepted":
            err(f"{GOVERNANCE_ADR} 状态为 {status},治理方案本身必须是 accepted")
    if not CI_FILE.exists():
        err("缺 .github/workflows/ci.yml")
    elif re.search(r"^\s{2}doc-gate:", CI_FILE.read_text(encoding="utf-8"), re.M) is None:
        err("ci.yml 不含 doc-gate job——治理门被摘除将无法被发现")


def check_budget() -> None:
    """检查 G:文档总量与单文件预算(防止历史堆积再次无意识发生)。"""
    total = 0
    for path in repo_md_files():
        size = (ROOT / path).stat().st_size
        total += size
        if size > MAX_FILE_BYTES:
            err(f"单文档超上限({size} > {MAX_FILE_BYTES} 字节):{path}")
    if total > TOTAL_BUDGET_BYTES:
        err(
            f"文档总量 {total} 字节超预算 {TOTAL_BUDGET_BYTES}——"
            "须显式上调 TOTAL_BUDGET_BYTES 并在提交说明中给出理由"
        )
    else:
        print(f"  [预算] 文档总量 {total} / {TOTAL_BUDGET_BYTES} 字节")


def check_index_freshness() -> None:
    """检查 H:ADR 索引表必须与 front-matter 一致(生成物,禁止手改漂移)。"""
    gen = ROOT / "scripts" / "gen_adr_index.py"
    if not gen.exists():
        err("缺 scripts/gen_adr_index.py(ADR 索引生成器)")
        return
    out = subprocess.run(
        [sys.executable, str(gen), "--check"],
        capture_output=True,
        text=True,
        check=False,
    )
    if out.returncode != 0:
        err(f"ADR 索引过期:{out.stderr.strip() or out.stdout.strip()}(运行 gen_adr_index.py)")


def check_doc_anchors() -> None:
    """检查 I(issue #79):ADR 声明的「守护测试/测试」锚点必须在代码中真实存在。

    背景:specs-first 文化下,文档是唯一真源,一条「已有守护测试 X」若不实,
    会被后续每个会话当裁决事实继承(2026-09-12 实锤 3 例)。本门只校验**明确
    声明式锚点**——`module::test_fn`(含 `::` 的 code 内联记号),不碰自然语言
    里的「已完成」等词(那会误报,如「该调用已完成」是引用被删文案)。

    判据:锚点末段函数名在 runtime/ 或 plugins/ 的 .rs 中出现 `fn <name>` 即通过。
    """
    code_roots = [p for p in (ROOT / "runtime", ROOT / "plugins") if p.is_dir()]
    if not code_roots:
        print("  [锚点] 未发现 runtime/ 或 plugins/ 代码源,跳过锚点校验")
        return
    code = []
    for cr in code_roots:
        for p in cr.rglob("*.rs"):
            if "/target/" in p.as_posix() or "\\target\\" in p.as_posix():
                continue
            try:
                code.append(p.read_text(encoding="utf-8", errors="replace"))
            except OSError:
                continue
    code_blob = "\n".join(code)

    # 模块名集合:文件名 stem + `mod X;` 声明——末段是模块的锚点(`a::b::skill_host`)
    # 是模块引用而非测试,跳过以免误报(issue #79 告诫:勿粗暴关键词拦截)。
    mod_names: set[str] = set()
    for cr in code_roots:
        for p in cr.rglob("*.rs"):
            if "\\target\\" in p.as_posix() or "/target/" in p.as_posix():
                continue
            mod_names.add(p.stem)
    for m in re.finditer(r"\bmod\s+([a-z_][a-z0-9_]*)\s*;", code_blob):
        mod_names.add(m.group(1))

    # 锚点记号:`a::b::test_name`(至少两段 ::);只校验非模块末段。
    anchor_re = re.compile(r"`([a-z_][a-z0-9_]*(?:::[a-z_][a-z0-9_]*){2,})`")
    adrs = collect_adrs()
    total = 0
    for aid, path in sorted(adrs.items()):
        text = path.read_text(encoding="utf-8", errors="replace")
        for m in anchor_re.finditer(text):
            anchor = m.group(1)
            fn = anchor.split("::")[-1]
            if fn in mod_names:
                continue  # 模块引用,非测试锚点
            total += 1
            if re.search(r"\bfn\s+" + re.escape(fn) + r"\b", code_blob) is None:
                err(f"{rel(path)}: 锚点 `{anchor}` 的函数 {fn} 在代码中不存在(过期引用)")
    print(f"  [锚点] ADR 声明式测试锚点 {total} 个,{sum(1 for x in errors if '锚点' in x)} 个失效")


def main() -> int:
    adrs = collect_adrs()
    metas = check_front_matter(adrs)
    check_bidirectional(metas)
    check_whitelist()
    check_budget()
    check_index_freshness()
    check_governance_selfcheck(metas)
    check_doc_anchors()

    print(f"doc-gate:ADR {len(adrs)} 个;错误 {len(errors)};提示 {len(warns)}")
    for w in warns:
        print(f"  [提示] {w}")
    for e in errors:
        print(f"  [错误] {e}")
    if errors:
        print("doc-gate FAILED", file=sys.stderr)
        return 1
    print("doc-gate OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
