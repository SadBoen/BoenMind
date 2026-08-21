#!/usr/bin/env bash
# BoenMind 服务器版卸载脚本（systemd）
#
# 用法：
#   sudo bash uninstall.sh          # 停止/禁用服务，卸载程序，保留数据（boenmind.db、backups、配置）
#   sudo bash uninstall.sh --purge  # 全部清除（含数据目录；不可恢复，慎用）
#   sudo bash uninstall.sh --help
#
# 可用环境变量覆盖（缺省即为下方常量）：
#   BOENMIND_DATA_DIR / BOENMIND_BIN_DEST
set -euo pipefail

DATA_DIR="${BOENMIND_DATA_DIR:-/var/lib/boenmind}"
BIN_DEST="${BOENMIND_BIN_DEST:-/usr/local/bin/boenmind}"
SERVICE_NAME=boenmind
SERVICE_FILE="/etc/systemd/system/$SERVICE_NAME.service"
PURGE=false

for arg in "$@"; do
  case "$arg" in
    --purge)     PURGE=true ;;
    --help|-h)
      sed -n '2,8p' "$0"
      exit 0 ;;
    *)
      echo "未知参数：$arg（支持 --purge / --help）" >&2
      exit 1 ;;
  esac
done

if [[ $EUID -ne 0 ]]; then
  echo "错误：请用 root 运行（sudo bash uninstall.sh）" >&2
  exit 1
fi

echo "==> 停止并禁用服务 $SERVICE_NAME"
systemctl disable --now "$SERVICE_NAME" 2>/dev/null || true
systemctl daemon-reload

echo "==> 删除 systemd 单元"
rm -f "$SERVICE_FILE"
rm -rf "/etc/systemd/system/$SERVICE_NAME.service.d"
systemctl daemon-reload

echo "==> 删除二进制 $BIN_DEST"
rm -f "$BIN_DEST" "${BIN_DEST}.bak"

echo "==> 删除专用用户 boenmind"
userdel boenmind 2>/dev/null || true

if $PURGE; then
  echo "==> 清除数据目录 $DATA_DIR（--purge）"
  rm -rf "$DATA_DIR"
  echo "✅ 已完全卸载（数据已删除，不可恢复）。"
else
  echo
  echo "✅ 已卸载程序与服务。数据目录保留：$DATA_DIR"
  echo "   （含 boenmind.db、backups/、配置）如需彻底删除，请运行：sudo bash uninstall.sh --purge"
fi