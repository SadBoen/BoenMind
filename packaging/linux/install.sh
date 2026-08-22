#!/usr/bin/env bash
# BoenMind 服务器版安装/升级脚本（systemd）
#
# 用法：
#   sudo bash install.sh            # 首次安装（web-server 与 install.sh 同目录，dist/ 子目录）
#   sudo bash install.sh /path/to/web-server
#   sudo bash install.sh --help
#
# 升级语义（已有 boenmind 服务时）：
#   - 备份数据库到 $DATA_DIR/backups/（时间戳命名，最多保留 $BACKUP_KEEP 份）
#   - 不覆盖已被修改的 systemd 单元（自定义 --bind 等保持生效）
#   - 升级失败时尝试回滚二进制
#
# 可用环境变量覆盖（缺省即为下方常量）：
#   BOENMIND_PORT / BOENMIND_DATA_DIR / BOENMIND_BIN_DEST / BOENMIND_BACKUP_KEEP
set -euo pipefail

# 发布流程会替换为实际版本号（v0.1.4 等）
VERSION="__VERSION__"
PORT="${BOENMIND_PORT:-17321}"
DATA_DIR="${BOENMIND_DATA_DIR:-/var/lib/boenmind}"
BIN_DEST="${BOENMIND_BIN_DEST:-/usr/local/bin/boenmind}"
SERVICE_NAME=boenmind
BACKUP_KEEP="${BOENMIND_BACKUP_KEEP:-7}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_SRC="${1:-$SCRIPT_DIR/web-server}"
DIST_SRC="$SCRIPT_DIR/dist"
SERVICE_SRC="$SCRIPT_DIR/boenmind.service"
SERVICE_FILE="/etc/systemd/system/$SERVICE_NAME.service"

log()  { echo "==> $*"; }
warn() { echo "警告：$*" >&2; }

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  sed -n '2,16p' "$0"
  exit 0
fi
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

IS_UPGRADE=false
if systemctl list-unit-files --type=service | grep -q "^${SERVICE_NAME}\.service"; then
  IS_UPGRADE=true
fi

# ---------- 备份（仅升级时） ----------
backup_db() {
  local db="$DATA_DIR/boenmind.db" bdir="$DATA_DIR/backups" stamp
  if [[ ! -f "$db" ]]; then return 0; fi
  mkdir -p "$bdir"
  stamp="$(date +%Y%m%d-%H%M%S)"
  if command -v sqlite3 >/dev/null 2>&1; then
    # Online Backup API：WAL 模式下也能拿到一致快照
    if sqlite3 "$db" ".backup '$bdir/boenmind-$stamp.db'" 2>/dev/null; then
      log "已备份数据库：$bdir/boenmind-$stamp.db"
    else
      warn "sqlite3 .backup 失败，改用文件拷贝兜底"
      cp -a "$db" "$bdir/boenmind-$stamp.db"
    fi
  else
    cp -a "$db" "$bdir/boenmind-$stamp.db"
    warn "本机无 sqlite3，文件拷贝可能不是一致快照（建议安装 sqlite3）"
  fi
  # 只保留最近 $BACKUP_KEEP 份
  ls -1t "$bdir"/boenmind-*.db 2>/dev/null | tail -n +$((BACKUP_KEEP + 1)) | xargs -r rm -f
  chown -R boenmind:boenmind "$bdir"
}

# 当前生效的旧二进制指向（用于回滚备份）
OLD_BIN_BAK=""
if $IS_UPGRADE && [[ -f "$BIN_DEST" ]]; then
  OLD_BIN_BAK="${BIN_DEST}.bak"
  cp -a "$BIN_DEST" "$OLD_BIN_BAK"
fi

echo "==> 创建专用用户与数据目录"
if ! id -u boenmind &>/dev/null; then
  useradd --system --home-dir "$DATA_DIR" --shell /usr/sbin/nologin boenmind
fi
mkdir -p "$DATA_DIR"
chown -R boenmind:boenmind "$DATA_DIR"

if $IS_UPGRADE; then
  log "检测到已有 ${SERVICE_NAME} 服务，进入升级模式"
  backup_db
else
  log "全新安装"
fi

echo "==> 安装二进制到 $BIN_DEST"
install -m 0755 "$BIN_SRC" "$BIN_DEST"

echo "==> 安装前端静态资源到 $DATA_DIR/dist"
rm -rf "$DATA_DIR/dist"
mkdir -p "$DATA_DIR/dist"
cp -r "$DIST_SRC"/. "$DATA_DIR/dist/"
chown -R boenmind:boenmind "$DATA_DIR/dist"

echo "==> 安装 systemd 服务并启动"
if [[ -f "$SERVICE_FILE" ]] && ! cmp -s "$SERVICE_FILE" "$SERVICE_SRC"; then
  warn "检测到 $SERVICE_FILE 与打包自带的单元不同（可能自定义过 --bind / 环境变量）"
  warn "为保护你的自定义配置，本次【不覆盖】现有单元；如需跟随官方单元，请手动："
  warn "  install -m 0644 '$SERVICE_SRC' '$SERVICE_FILE' && systemctl daemon-reload"
else
  install -m 0644 "$SERVICE_SRC" "$SERVICE_FILE"
fi
systemctl daemon-reload
systemctl enable --now "$SERVICE_NAME"
systemctl restart "$SERVICE_NAME"

echo "==> 检查服务状态"
sleep 1
if systemctl is-active --quiet "$SERVICE_NAME"; then
  # HTTP 探活：进程在但端口没监听时，is-active 会误报；顺带验证应用层可访问
  if command -v curl >/dev/null 2>&1; then
    if ! curl -fsS -o /dev/null --max-time 5 "http://127.0.0.1:$PORT/" 2>/dev/null; then
      warn "HTTP 探活失败：http://127.0.0.1:$PORT/（若返回 401/404 属正常，请人工确认）"
    fi
  fi
  echo
  echo "✅ BoenMind ${VERSION} 已安装并启动！"
  echo
  echo "   浏览器访问：http://服务器IP:$PORT"
  echo "   服务管理：  systemctl status boenmind / systemctl restart boenmind"
  echo "   数据目录：  $DATA_DIR（boenmind.db、config 均在其下；升级前自动备份到 backups/）"
  echo
  echo "   安全提示：服务器版默认未开启登录认证（--auth 可开启；开启后凭据/设置等"
  echo "   特权方法需登录）。若对外提供，建议开启 --auth，或通过反向代理（nginx/caddy）"
  echo "   加访问密码/HTTPS 后转发，并将服务绑定到 127.0.0.1（见 boenmind.service 注释）。"
else
  echo "❌ 服务启动失败，请查看日志：journalctl -u $SERVICE_NAME -e" >&2
  # 回滚二进制（升级失败时恢复旧版本）
  if [[ -n "$OLD_BIN_BAK" ]] && [[ -f "$OLD_BIN_BAK" ]]; then
    echo "尝试回滚二进制到升级前版本……" >&2
    install -m 0755 "$OLD_BIN_BAK" "$BIN_DEST"
    rm -f "$OLD_BIN_BAK"
    systemctl daemon-reload && systemctl restart "$SERVICE_NAME" || true
  fi
  exit 1
fi
rm -f "$OLD_BIN_BAK"