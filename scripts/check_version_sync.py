#!/usr/bin/env python3
"""版本号三处同步校验(issue #23)。

校验四处必须一致的版本号:
  1. runtime/Cargo.toml          [workspace.package] version(权威源)
  2. runtime/webapp/package.json "version"
  3. AGENTS.md                   「当前版本 = vX.Y.Z」状态行
  4. runtime/Cargo.lock          九个 workspace 成员包版本(锁文件随仓)

用法:
  python scripts/check_version_sync.py            # 全绿退出 0,不一致退出 1
  python scripts/check_version_sync.py --fix      # 以 Cargo.toml 为权威对齐
                                                  # package.json 与 AGENTS.md,
                                                  # 并经 cargo metadata 刷新 Cargo.lock
  --root <dir>                                    # 仓库根(默认脚本上一级)
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

WORKSPACE_MEMBERS = [
    "bm-cli",
    "bm-contract",
    "bm-core",
    "bm-judge",
    "bm-persist",
    "bm-providers",
    "bm-runtime",
    "bm-surface-http",
    "bm-testkit",
]

CARGO_VER_RE = re.compile(r'^version\s*=\s*"(\d+\.\d+\.\d+)"\s*$', re.M)
AGENTS_VER_RE = re.compile(r"当前版本 = v(\d+\.\d+\.\d+)")
LOCK_BLOCK_RE = re.compile(r'\[\[package\]\]\s*\nname = "([^"]+)"\s*\nversion = "([^"]+)"')


def read_workspace_version(cargo_toml: Path) -> str:
    text = cargo_toml.read_text(encoding="utf-8")
    # 只认 [workspace.package] 段内的 version,避免撞到依赖声明
    section = text.split("[workspace.package]", 1)
    if len(section) < 2:
        raise SystemExit(f"FATAL: {cargo_toml} 缺 [workspace.package] 段")
    body = section[1].split("[", 1)[0]
    m = CARGO_VER_RE.search(body)
    if not m:
        raise SystemExit(f"FATAL: {cargo_toml} [workspace.package] 缺 version")
    return m.group(1)


def read_package_json_version(path: Path) -> str:
    return json.loads(path.read_text(encoding="utf-8"))["version"]


def read_agents_version(path: Path) -> str:
    m = AGENTS_VER_RE.search(path.read_text(encoding="utf-8"))
    if not m:
        raise SystemExit(f"FATAL: {path} 找不到「当前版本 = vX.Y.Z」状态行")
    return m.group(1)


def read_lock_versions(cargo_lock: Path) -> dict[str, str]:
    blocks = dict(LOCK_BLOCK_RE.findall(cargo_lock.read_text(encoding="utf-8")))
    missing = [name for name in WORKSPACE_MEMBERS if name not in blocks]
    if missing:
        raise SystemExit(f"FATAL: {cargo_lock} 缺 workspace 成员: {missing}")
    return {name: blocks[name] for name in WORKSPACE_MEMBERS}


def apply_fix(root: Path, version: str) -> list[str]:
    actions: list[str] = []

    pkg = root / "runtime/webapp/package.json"
    if read_package_json_version(pkg) != version:
        data = json.loads(pkg.read_text(encoding="utf-8"))
        data["version"] = version
        pkg.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        actions.append(f"package.json -> {version}")

    agents = root / "AGENTS.md"
    if read_agents_version(agents) != version:
        text = agents.read_text(encoding="utf-8")
        text = AGENTS_VER_RE.sub(f"当前版本 = v{version}", text, count=1)
        agents.write_text(text, encoding="utf-8")
        actions.append(f"AGENTS.md -> {version}")

    # Cargo.lock 里 workspace 成员版本由 cargo 自身维护;metadata 只刷新成员版本,
    # 不升级第三方依赖(区别于 cargo update)
    lock = root / "runtime/Cargo.lock"
    if any(v != version for v in read_lock_versions(lock).values()):
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--manifest-path",
             str(root / "runtime/Cargo.toml")],
            check=True, capture_output=True,
        )
        actions.append("Cargo.lock 经 cargo metadata 刷新")

    return actions


def main() -> int:
    ap = argparse.ArgumentParser(description="版本号三处同步校验")
    ap.add_argument("--root", default=str(Path(__file__).resolve().parent.parent))
    ap.add_argument("--fix", action="store_true", help="以 Cargo.toml 为权威对齐其余三处")
    args = ap.parse_args()
    root = Path(args.root)

    version = read_workspace_version(root / "runtime/Cargo.toml")
    if args.fix:
        for action in apply_fix(root, version):
            print(f"FIXED: {action}")

    problems: list[str] = []

    pkg_ver = read_package_json_version(root / "runtime/webapp/package.json")
    if pkg_ver != version:
        problems.append(f"runtime/webapp/package.json: {pkg_ver} != {version}")

    agents_ver = read_agents_version(root / "AGENTS.md")
    if agents_ver != version:
        problems.append(f"AGENTS.md 状态行: v{agents_ver} != v{version}")

    for name, lock_ver in read_lock_versions(root / "runtime/Cargo.lock").items():
        if lock_ver != version:
            problems.append(f"runtime/Cargo.lock 成员 {name}: {lock_ver} != {version}")

    if problems:
        print(f"版本号失同步(权威 = runtime/Cargo.toml v{version}):")
        for p in problems:
            print(f"  - {p}")
        print("修复: python scripts/check_version_sync.py --fix")
        return 1

    print(f"版本同步 OK: v{version}(Cargo.toml / package.json / AGENTS.md / Cargo.lock ×{len(WORKSPACE_MEMBERS)})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
