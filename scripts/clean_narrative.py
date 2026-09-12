#!/usr/bin/env python3
"""清理代码注释里的「纯过程痕迹」括号(收窄规则,低风险)。

用户 2026-09-13 反馈:代码注释堆积的历史信息会污染人或 AI 读代码时的上下文。
本脚本只删**括号内容恰为「日期」或「日期+一个过程词」**的片段,例如:
  `P1-24(2026-09-07 架构评审):...`  →  `P1-24:...`
  `设计目标(2026-09-04 用户裁决):...` →  `设计目标:...`
括号里若含**实际内容**(逗号/冒号后有中文实词,如 `(2026-09-03 修正:热重载证据=…)`),
一律**不删**——正则分不清"过程注"与"句子成分",删了会读不通,故宁缺勿滥。

保留:ADR-NNNN / 基线 §X / 合同名 / issue 号(P1-NN)——这些是可追溯的决策来源。
只碰 .rs/.ts/.tsx 的注释行,不碰代码、字符串、.md。

用法:
  python scripts/clean_narrative.py            # 干跑,打印 diff
  python scripts/clean_narrative.py --stat      # 只看统计
  python scripts/clean_narrative.py --apply     # 落盘
"""
from __future__ import annotations

import argparse
import difflib
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

_PROC = (
    r"(?:架构评审|审计修复|回看收归|评审修复|复核批|用户裁决|用户要求|用户反馈|"
    r"N\+\d+\s*修复|收口|修复|评审|实测)?"
)
# 括号内 = 纯日期 或 日期 + 单个过程词;闭合括号前不得再有其它内容
TIGHT = re.compile(r"[（(]\s*\d{4}-\d{2}-\d{2}[\s,，]*" + _PROC + r"\s*[）)]")


def is_comment_line(line: str) -> bool:
    """裸 `*` 仅在后跟空白/`/`/行尾时算块注释续行(否则是 Rust 解引用 `*p = `)。"""
    s = line.lstrip()
    if s.startswith("//") or s.startswith("/*"):
        return True
    return re.match(r"^\*(?:\s|/|$)", s) is not None


def clean_line(line: str) -> str:
    if not is_comment_line(line):
        return line
    nl = "\n" if line.endswith("\n") else ""
    body = line.rstrip("\n")
    # 分离缩进 / 注释前缀 / 正文,只对正文做处理,保住缩进与前缀
    m = re.match(r"^(\s*)((?://[/!]?|/\*+|\*)\s?)(.*)$", body)
    if not m:
        return line
    indent, prefix, text = m.group(1), m.group(2), m.group(3)
    new_text = TIGHT.sub("", text)
    if new_text == text:
        return line
    new_text = re.sub(r"[（(]\s*[）)]", "", new_text)
    new_text = re.sub(r"[ \t]{2,}", " ", new_text)
    return f"{indent}{prefix}{new_text}{nl}"


def tracked_files() -> list[str]:
    out = subprocess.check_output(
        ["git", "ls-files", "*.rs", "*.ts", "*.tsx"], cwd=ROOT, text=True
    ).splitlines()
    return [f for f in out if "/target/" not in f and "node_modules" not in f]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true")
    ap.add_argument("--stat", action="store_true")
    args = ap.parse_args()

    files_changed = lines_changed = 0
    for rel in tracked_files():
        p = ROOT / rel
        try:
            # newline="" 关掉通用换行归一化:保住原行尾(CRLF 文件不得被转成 LF)
            original = p.read_text(encoding="utf-8", newline="")
        except Exception:
            continue
        lines = original.splitlines(keepends=True)
        cleaned = [clean_line(l) for l in lines]
        if cleaned == lines:
            continue
        new_text = "".join(cleaned)
        if new_text == original:
            continue
        files_changed += 1
        lines_changed += sum(1 for a, b in zip(lines, cleaned) if a != b)
        if not args.stat:
            sys.stdout.write("".join(difflib.unified_diff(
                lines, cleaned, fromfile=f"a/{rel}", tofile=f"b/{rel}", n=1,
            )))
            print()
        if args.apply:
            p.write_text(new_text, encoding="utf-8", newline="")

    print(f"\n=== {'已落盘' if args.apply else '干跑'}:{files_changed} 文件,{lines_changed} 行 ===")
    if not args.apply:
        print("复核 diff;确认后 --apply。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
