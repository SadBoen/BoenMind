#!/usr/bin/env bash
# BoenMind 统一打包脚本：
#   - 构建前端 dist（npm run build）
#   - 构建 web-server release 二进制（cargo build --release -p web-server）
#   - 产出 Windows 便携版 zip（web-server.exe + frontend/dist）
#   - 产出 Linux 服务器版 tar.gz（web-server + dist + packaging/linux 三件套）
#
# 用法：
#   bash scripts/package.sh              # 全部（本机默认 win）
#   bash scripts/package.sh --linux      # 仅 Linux 服务器包
#   bash scripts/package.sh --win        # 仅 Windows 便携包
#   bash scripts/package.sh --clean      # 仅清空 out/ 产物目录
#
# 产出目录：out/
#   out/boenmind-server_<ver>_linux-x86_64.tar.gz
#   out/BoenMind_<ver>_x64_portable.zip
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE="${1:---all}"
VER="$(node -p "require('./frontend/package.json').version")"
OUT="$ROOT/out"

if [[ "$MODE" == "--clean" ]]; then
  rm -rf "$OUT" && echo "已清空 $OUT" && exit 0
fi

echo "==> 版本：$VER"
mkdir -p "$OUT"

if [[ "$MODE" == "--all" || "$MODE" == "--win" || "$MODE" == "--linux" ]]; then
  echo "==> 构建前端 dist"
  (cd frontend && npm run build)
fi

if [[ "$MODE" == "--all" || "$MODE" == "--win" ]]; then
  echo "==> 构建 web-server release（Windows）"
  cargo build --release -p web-server
fi

if [[ "$MODE" == "--all" || "$MODE" == "--linux" ]]; then
  echo "==> 构建 web-server release（Linux 交叉编译需 cargo-zigbuild，本机无 Linux target 时跳过）"
  # 不强行交叉编译：CI 里用 cargo-zigbuild 处理（见 .github/workflows/release.yml）。
  # 本机（Windows）执行 --linux 只打包已构建好的 Linux 二进制（若存在）。
fi

if [[ "$MODE" == "--all" || "$MODE" == "--win" ]]; then
  echo "==> 打包 Windows 便携版"
  WIN_STAGE="$OUT/BoenMind-$VER-win-x64"
  rm -rf "$WIN_STAGE"
  mkdir -p "$WIN_STAGE/dist"
  cp target/release/web-server.exe "$WIN_STAGE/BoenMind.exe" 2>/dev/null || {
    echo "❌ 找不到 target/release/web-server.exe，请先 cargo build --release -p web-server" >&2
    exit 1
  }
  cp -r frontend/dist/. "$WIN_STAGE/dist/"
  # 便携版说明
  cat > "$WIN_STAGE/README.txt" <<'EOF'
BoenMind 便携版（免安装）
  解压后运行 BoenMind.exe，浏览器/桌面壳自动打开 127.0.0.1:17321。
  数据（boenmind.db）保存在程序目录同级的当前工作目录。
EOF
  (cd "$OUT" && powershell.exe -NoProfile -Command \
    "Compress-Archive -Path 'BoenMind-$VER-win-x64/*' -DestinationPath 'BoenMind_${VER}_x64_portable.zip' -Force" 2>/dev/null) || {
    # 无 powershell 时用 zip 命令兜底
    (cd "$WIN_STAGE" && rm -f "../BoenMind_${VER}_x64_portable.zip" && zip -qr "../BoenMind_${VER}_x64_portable.zip" .)
  }
  rm -rf "$WIN_STAGE"
  echo "   ✅ $OUT/BoenMind_${VER}_x64_portable.zip"
fi

if [[ "$MODE" == "--all" || "$MODE" == "--linux" ]]; then
  echo "==> 打包 Linux 服务器版"
  LINUX_BIN="${LINUX_BIN:-}"
  if [[ -z "$LINUX_BIN" ]] && [[ -f "$ROOT/target/x86_64-unknown-linux-gnu/release/web-server" ]]; then
    LINUX_BIN="$ROOT/target/x86_64-unknown-linux-gnu/release/web-server"
  fi
  if [[ -n "$LINUX_BIN" ]]; then
    LINUX_STAGE="$OUT/boenmind-server_${VER}_linux-x86_64"
    rm -rf "$LINUX_STAGE"
    mkdir -p "$LINUX_STAGE/dist"
    cp "$LINUX_BIN" "$LINUX_STAGE/web-server"
    cp -r frontend/dist/. "$LINUX_STAGE/dist/"
    cp packaging/linux/install.sh "$LINUX_STAGE/install.sh"
    cp packaging/linux/uninstall.sh "$LINUX_STAGE/uninstall.sh"
    cp packaging/linux/boenmind.service "$LINUX_STAGE/boenmind.service"
    cp packaging/linux/README.md "$LINUX_STAGE/README.md"
    sed -i "s/__VERSION__/${VER}/" "$LINUX_STAGE/install.sh"
    chmod +x "$LINUX_STAGE/install.sh" "$LINUX_STAGE/uninstall.sh"
    (cd "$OUT" && tar czf "boenmind-server_${VER}_linux-x86_64.tar.gz" "boenmind-server_${VER}_linux-x86_64")
    rm -rf "$LINUX_STAGE"
    echo "   ✅ $OUT/boenmind-server_${VER}_linux-x86_64.tar.gz"
  else
    echo "   ⚠️ 未找到 Linux 二进制（target/x86_64-unknown-linux-gnu/release/web-server），跳过 Linux 打包"
  fi
fi

echo "==> 完成，产物在 $OUT/"
ls -la "$OUT/"