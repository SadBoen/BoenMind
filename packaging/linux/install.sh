#!/usr/bin/env bash
# BoenMind 服务器版一键安装脚本（systemd）
#
# 用法：
#   sudo bash install.sh            # web-server 与 install.sh 同目录（dist/ 子目录）
#   sudo bash install.sh /path/to/web-server
#
# 安装内容：
#   - 创建专用系统用户 boenmind 与数据目录 /var/lib/boenmind
#   - 拷贝 web-server 二进制到 /usr/local/bin/web-server
#   - 拷贝前端静态资源 dist/ 到 /var/lib/boenmind/dist
#   - 安装并启动 systemd 服务 boenmind（开机自启，监听 0.0.0.0:17321）
set -euo pipefail

# 发布流程会替换为实际版本号（v0.1.4 等）
VERSION="__VERSION__"
PORT=17321
DATA_DIR=/var/lib/boenmind
BIN_DEST=/usr/local/bin/web-server
SERVICE_NAME=boenmind
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_SRC="${1:-$SCRIPT_DIR/web-server}"
DIST_SRC="$SCRIPT_DIR/dist"
SERVICE_SRC="$SCRIPT_DIR/boenmind.service"

if [[ $EUID -ne 0 ]]; then
  echo "错误：请用 root 运行（sudo bash install.sh）" >&2
  exit 1
fi

if [[ ! -f "$BIN_SRC" ]]; then
  echo "错误：找不到二进制文件：$BIN_SRC" >&2
  exit 1
fi

if [[ ! -d "$DIST_SRC" ]]; then
  echo "错误：找不到前端静态资源目录：$DIST_SRC" >&2
  exit 1
fi

if [[ ! -f "$SERVICE_SRC" ]]; then
  echo "错误：找不到服务单元文件：$SERVICE_SRC" >&2
  exit 1
fi

echo "==> 创建专用用户与数据目录"
if ! id -u boenmind &>/dev/null; then
  useradd --system --home-dir "$DATA_DIR" --shell /usr/sbin/nologin boenmind
fi
mkdir -p "$DATA_DIR"
chown -R boenmind:boenmind "$DATA_DIR"

echo "==> 安装二进制到 $BIN_DEST"
install -m 0755 "$BIN_SRC" "$BIN_DEST"

echo "==> 安装前端静态资源到 $DATA_DIR/dist"
rm -rf "$DATA_DIR/dist"
mkdir -p "$DATA_DIR/dist"
cp -r "$DIST_SRC"/. "$DATA_DIR/dist/"
chown -R boenmind:boenmind "$DATA_DIR/dist"

echo "==> 安装 systemd 服务并启动"
install -m 0644 "$SERVICE_SRC" "/etc/systemd/system/$SERVICE_NAME.service"
systemctl daemon-reload
systemctl enable --now "$SERVICE_NAME"
systemctl restart "$SERVICE_NAME"

echo "==> 检查服务状态"
sleep 1
if systemctl is-active --quiet "$SERVICE_NAME"; then
  IP=$(hostname -I 2>/dev/null | awk '{print $1}')
  echo
  echo "✅ BoenMind ${VERSION} 已安装并启动！"
  echo
  echo "   浏览器访问：http://${IP:-服务器IP}:$PORT"
  echo "   服务管理：  systemctl status boenmind / systemctl restart boenmind"
  echo "   数据目录：  $DATA_DIR（boenmind.db、配置均在其下）"
  echo
  echo "   防火墙提示：无法访问时放行 $PORT 端口，例如："
  echo "     sudo ufw allow $PORT/tcp"
  echo
  echo "   安全提示：当前版本无登录认证，请仅在可信内网使用，"
  echo "   或通过反向代理（nginx / caddy）加访问密码或 HTTPS 后对外。"
else
  echo "❌ 服务启动失败，请查看日志：journalctl -u $SERVICE_NAME -e" >&2
  exit 1
fi